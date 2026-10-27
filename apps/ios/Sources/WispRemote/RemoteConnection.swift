import Foundation
import Combine

@MainActor public protocol RemoteSocket: AnyObject {
    func send(_ text: String) async throws
    func receive() async throws -> String
    func close()
    var hostOffline: Bool { get }
}

@MainActor private final class SystemSocket: RemoteSocket {
    private let session: URLSession
    private let task: URLSessionWebSocketTask
    init(url: URL) {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.timeoutIntervalForRequest = 15
        session = URLSession(configuration: configuration, delegate: NoRedirects(), delegateQueue: nil)
        task = session.webSocketTask(with: url)
        task.maximumMessageSize = RemoteCrypto.hostFrameLimit
        task.resume()
    }
    func send(_ text: String) async throws { try await task.send(.string(text)) }
    func receive() async throws -> String {
        guard case .string(let text) = try await task.receive() else { throw RemoteError.invalidFrame }
        return text
    }
    var hostOffline: Bool { task.closeCode.rawValue == 4404 }
    func close() {
        task.cancel(with: .goingAway, reason: nil)
        session.invalidateAndCancel()
    }
    private final class NoRedirects: NSObject, URLSessionTaskDelegate {
        func urlSession(_ session: URLSession, task: URLSessionTask,
                        willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest,
                        completionHandler: @escaping (URLRequest?) -> Void) { completionHandler(nil) }
    }
}

/// One ordered send lane per authenticated connection. A reconnection creates
/// a fresh nonce/sequence; only read snapshots are refreshed by the UI.
@MainActor public final class RemoteConnection: ObservableObject {
    public enum State: Equatable { case idle, connecting, online, offline, retrying }
    @Published public private(set) var state: State = .idle
    @Published public private(set) var hostName = ""
    @Published public private(set) var hostVersion = ""
    @Published public private(set) var commands: Set<String> = []
    @Published public private(set) var diagnostic: String?
    public private(set) var readSessionID = UUID()
    public var authenticated: (() -> Void)?

    private let factory: (URL) -> any RemoteSocket
    private let requestTimeout: UInt64
    private let handshakeTimeout: UInt64
    private let retryDelay: UInt64
    private var pump: Task<Void, Never>?
    private var sendTail: Task<Void, Never>?
    private var socket: (any RemoteSocket)?
    private var generation = UUID()
    private var nonce: String?
    private var sequence: UInt64 = 0
    private var crypto: RemoteCrypto?
    private var localHandler: ((String, String?, [String: RemoteValue]) throws -> RemoteValue)?
    private struct Pending {
        let continuation: CheckedContinuation<RemoteValue, Error>
        let timeout: Task<Void, Never>
    }
    private var pending: [String: Pending] = [:]

    public init(requestTimeoutNanoseconds: UInt64 = 90_000_000_000,
                handshakeTimeoutNanoseconds: UInt64 = 12_000_000_000,
                retryDelayNanoseconds: UInt64 = 1_000_000_000,
                socketFactory: ((URL) -> any RemoteSocket)? = nil) {
        factory = socketFactory ?? { SystemSocket(url: $0) }
        requestTimeout = requestTimeoutNanoseconds
        handshakeTimeout = handshakeTimeoutNanoseconds
        retryDelay = retryDelayNanoseconds
    }

    public func supports(_ command: String) -> Bool { commands.contains(command) }

    /// In-process data source for the bundled demo. No URL, socket, handshake,
    /// retry loop or authentication callback is involved.
    public func useLocalService(name: String, commands: Set<String>,
                                handler: @escaping (String, String?, [String: RemoteValue]) throws -> RemoteValue) {
        disconnect()
        localHandler = handler
        hostName = name
        hostVersion = "demo-v1"
        self.commands = commands
        state = .online
    }

    public func connect(_ configuration: RemoteConfiguration) {
        disconnect()
        let generation = self.generation
        crypto = configuration.crypto
        state = .connecting
        pump = Task { [weak self] in
            guard let self else { return }
            var attempt: UInt64 = 0
            while !Task.isCancelled && self.generation == generation {
                let channel = factory(configuration.webSocketURL)
                socket = channel
                let deadline = Task {
                    do { try await Task.sleep(nanoseconds: self.handshakeTimeout) } catch { return }
                    channel.close()
                }
                do {
                    let hello = try configuration.crypto.open(try await channel.receive())
                    deadline.cancel()
                    guard self.generation == generation, !Task.isCancelled else { return }
                    guard hello["type"].string == "hello", !hello["nonce"].string.isEmpty,
                          case .array(let advertised) = hello["commands"],
                          advertised.allSatisfy({ !$0.string.isEmpty }) else { throw RemoteError.incompatibleResponse }
                    nonce = hello["nonce"].string
                    sequence = 0
                    commands = Set(advertised.map(\.string))
                    hostName = hello["name"].string
                    hostVersion = hello["version"].string
                    diagnostic = nil
                    readSessionID = UUID()
                    state = .online
                    attempt = 0
                    authenticated?()
                    while !Task.isCancelled && self.generation == generation {
                        let message = try configuration.crypto.open(try await channel.receive())
                        guard self.generation == generation, !Task.isCancelled else { return }
                        try receive(message)
                    }
                } catch {
                    deadline.cancel()
                    guard self.generation == generation, !Task.isCancelled else { return }
                    // Do not surface URLSession errors: they can contain URLs.
                    diagnostic = (error as? RemoteError)?.localizedDescription
                        ?? (channel.hostOffline ? "Wisp 主机离线，请保持主机联网并运行 Wisp。" : "连接不可用，正在重新连接。")
                    state = channel.hostOffline ? .offline : .retrying
                }
                channel.close()
                socket = nil
                nonce = nil
                commands = []
                failPending()
                sendTail?.cancel()
                sendTail = nil
                do { try await Task.sleep(nanoseconds: retryDelay * (1 << min(attempt, 3))) } catch { return }
                attempt = min(attempt + 1, 3)
            }
        }
    }

    public func disconnect() {
        generation = UUID()
        readSessionID = UUID()
        pump?.cancel()
        pump = nil
        sendTail?.cancel()
        sendTail = nil
        socket?.close()
        socket = nil
        nonce = nil
        crypto = nil
        localHandler = nil
        hostName = ""
        hostVersion = ""
        commands = []
        state = .idle
        diagnostic = nil
        failPending()
    }

    public func call(_ command: String, project: String? = nil,
                     args: [String: RemoteValue] = [:]) async throws -> RemoteValue {
        try Task.checkCancellation()
        guard state == .online else { throw RemoteError.disconnected }
        guard supports(command) else { throw RemoteError.unsupported }
        if let localHandler { return try localHandler(command, project, args) }
        guard let socket, let nonce, let crypto else { throw RemoteError.disconnected }
        guard pending.count < 64 else { throw RemoteError.service("请求过多，请稍后重试。") }
        let id = UUID().uuidString
        let generation = self.generation
        return try await withTaskCancellationHandler {
            try Task.checkCancellation()
            return try await withCheckedThrowingContinuation { continuation in
                let timeout = Task { [weak self] in
                    do { try await Task.sleep(nanoseconds: self?.requestTimeout ?? 0) } catch { return }
                    self?.finish(id, .failure(RemoteError.outcomeUnknown))
                }
                pending[id] = Pending(continuation: continuation, timeout: timeout)
                let previous = sendTail
                sendTail = Task { [weak self] in
                    await previous?.value
                    guard let self, !Task.isCancelled, self.generation == generation,
                          self.nonce == nonce, self.pending[id] != nil else { return }
                    do {
                        self.sequence += 1
                        let frame = try crypto.seal(.object([
                            "nonce": .string(nonce), "seq": .unsigned(self.sequence), "id": .string(id),
                            "command": .string(command), "project_id": project.map(RemoteValue.string) ?? .null,
                            "args": .object(args),
                        ]))
                        try await socket.send(frame)
                    } catch {
                        self.finish(id, .failure(error as? RemoteError ?? .disconnected))
                        // An oversized local request has not touched the socket.
                        if (error as? RemoteError) != .tooLarge { socket.close() }
                    }
                }
            }
        } onCancel: {
            Task { @MainActor [weak self] in self?.finish(id, .failure(CancellationError())) }
        }
    }

    private func receive(_ message: RemoteValue) throws {
        guard message["type"].string == "response" else { throw RemoteError.incompatibleResponse }
        let response = message["response"]
        guard response["schema"].string == "wisp.native-settings.v1", !response["id"].string.isEmpty else {
            throw RemoteError.incompatibleResponse
        }
        if response["error"] != .null {
            finish(response["id"].string, .failure(RemoteError.service(response["error"].string)))
        } else {
            finish(response["id"].string, .success(response["result"]))
        }
    }
    private func finish(_ id: String, _ result: Result<RemoteValue, Error>) {
        guard let request = pending.removeValue(forKey: id) else { return }
        request.timeout.cancel()
        request.continuation.resume(with: result)
    }
    private func failPending() {
        for id in Array(pending.keys) { finish(id, .failure(RemoteError.disconnected)) }
    }
}

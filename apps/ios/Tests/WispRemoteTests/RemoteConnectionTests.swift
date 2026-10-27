import XCTest
@testable import WispRemote

@MainActor private final class FakeSocket: RemoteSocket {
    var hostOffline = false
    var received: [RemoteValue] = []
    var frames: [String] = []
    var waiter: CheckedContinuation<String, Error>?
    var closed = false
    var reply = true
    let nonce: String
    init(nonce: String = "nonce-one", hello: Bool = true) throws {
        self.nonce = nonce
        if hello {
            frames = [try TestHostCrypto.seal(.object([
                "type": .string("hello"), "nonce": .string(nonce), "name": .string("Fake host"), "version": .string("1.19.0"),
                "commands": .array(["list_projects", "native_conversation_send"].map(RemoteValue.string)),
            ]))]
        }
    }
    func send(_ text: String) async throws {
        guard !closed else { throw RemoteError.disconnected }
        let request = try TestHostCrypto.open(text)
        received.append(request)
        // Suspend once: concurrent calls must still reach us in sequence.
        await Task.yield()
        if reply {
            push(try TestHostCrypto.seal(.object(["type": .string("response"), "response": .object([
                "schema": .string("wisp.native-settings.v1"), "id": request["id"], "result": .array([]), "error": .null,
            ])])))
        }
    }
    func receive() async throws -> String {
        guard !closed else { throw RemoteError.disconnected }
        if !frames.isEmpty { return frames.removeFirst() }
        return try await withCheckedThrowingContinuation { waiter = $0 }
    }
    func push(_ frame: String) {
        if let waiter { self.waiter = nil; waiter.resume(returning: frame) }
        else { frames.append(frame) }
    }
    func close() {
        closed = true
        waiter?.resume(throwing: RemoteError.disconnected)
        waiter = nil
    }
}

@MainActor final class RemoteConnectionTests: XCTestCase {
    private func waitUntil(_ condition: () -> Bool, file: StaticString = #filePath, line: UInt = #line) async throws {
        for _ in 0..<1000 {
            if condition() { return }
            try await Task.sleep(nanoseconds: 1_000_000)
        }
        XCTFail("Condition did not become true", file: file, line: line)
        throw RemoteError.outcomeUnknown
    }
    private func configuration() throws -> RemoteConfiguration {
        try RemoteConfiguration(address: "https://relay.example.test", code: testCode)
    }
    func testConcurrentRequestsAreSequencedAndCapabilitiesAreEnforced() async throws {
        let socket = try FakeSocket()
        let connection = RemoteConnection(socketFactory: { _ in socket })
        defer { connection.disconnect() }
        connection.connect(try configuration())
        try await waitUntil { connection.state == .online }
        async let first = connection.call("list_projects")
        async let second = connection.call("list_projects")
        _ = try await (first, second)
        XCTAssertEqual(socket.received.map { $0["seq"].uint }, [1, 2])
        XCTAssertTrue(socket.received.allSatisfy { $0["nonce"].string == "nonce-one" })
        do { _ = try await connection.call("shell"); XCTFail("Unadvertised command was sent") }
        catch { XCTAssertEqual(error as? RemoteError, .unsupported) }
        XCTAssertEqual(socket.received.count, 2)
    }
    func testLostMutationIsNotReplayedAfterReconnect() async throws {
        let first = try FakeSocket(); first.reply = false
        let second = try FakeSocket(nonce: "nonce-two")
        var count = 0
        let connection = RemoteConnection(retryDelayNanoseconds: 1_000_000, socketFactory: { _ in
            count += 1; return count == 1 ? first : second
        })
        defer { connection.disconnect() }
        connection.connect(try configuration())
        try await waitUntil { connection.state == .online }
        let mutation = Task { try await connection.call("native_conversation_send", project: "p", args: ["message": .string("hello")]) }
        try await waitUntil { first.received.count == 1 }
        first.close()
        do { _ = try await mutation.value; XCTFail("Lost reply should surface") }
        catch { XCTAssertEqual(error as? RemoteError, .disconnected) }
        try await waitUntil { count == 2 && connection.state == .online }
        XCTAssertTrue(second.received.isEmpty)
        _ = try await connection.call("list_projects")
        XCTAssertEqual(second.received[0]["seq"].uint, 1)
        XCTAssertEqual(second.received[0]["nonce"].string, "nonce-two")
    }
    func testRequestTimeoutAndCancellationFinishWithoutReplay() async throws {
        let socket = try FakeSocket(); socket.reply = false
        let connection = RemoteConnection(requestTimeoutNanoseconds: 30_000_000, socketFactory: { _ in socket })
        defer { connection.disconnect() }
        connection.connect(try configuration())
        try await waitUntil { connection.state == .online }
        do { _ = try await connection.call("list_projects"); XCTFail("Expected timeout") }
        catch { XCTAssertEqual(error as? RemoteError, .outcomeUnknown) }
        let request = Task { try await connection.call("list_projects") }
        try await waitUntil { socket.received.count == 2 }
        request.cancel()
        do { _ = try await request.value; XCTFail("Expected cancellation") }
        catch { XCTAssertTrue(error is CancellationError) }
        XCTAssertEqual(socket.received.count, 2)
    }
    func testUnauthenticatedSocketHasBoundedHandshakeAndSavesNothing() async throws {
        let socket = try FakeSocket(hello: false)
        let connection = RemoteConnection(handshakeTimeoutNanoseconds: 10_000_000,
            retryDelayNanoseconds: 5_000_000_000, socketFactory: { _ in socket })
        defer { connection.disconnect() }
        var authenticated = false
        connection.authenticated = { authenticated = true }
        connection.connect(try configuration())
        try await waitUntil { socket.closed }
        XCTAssertFalse(authenticated)
        XCTAssertNotEqual(connection.state, .online)
        XCTAssertTrue(socket.received.isEmpty)
    }

    func testLocalServiceUsesNoSocketOrAuthenticationAndEnforcesCapabilities() async throws {
        let socket = try FakeSocket()
        var socketCount = 0
        var authenticated = false
        let connection = RemoteConnection(socketFactory: { _ in socketCount += 1; return socket })
        connection.authenticated = { authenticated = true }
        connection.useLocalService(name: "Offline demo", commands: ["list_projects"]) { command, project, args in
            XCTAssertEqual(command, "list_projects")
            XCTAssertNil(project)
            XCTAssertTrue(args.isEmpty)
            return .array([.object(["id": .string("demo")])])
        }
        let result = try await connection.call("list_projects")
        XCTAssertEqual(result.array.first?["id"].string, "demo")
        XCTAssertEqual(socketCount, 0)
        XCTAssertFalse(authenticated)
        do { _ = try await connection.call("shell"); XCTFail("Must reject unavailable commands") }
        catch { XCTAssertEqual(error as? RemoteError, .unsupported) }
        connection.disconnect()
        do { _ = try await connection.call("list_projects"); XCTFail("Local provider must be detached") }
        catch { XCTAssertEqual(error as? RemoteError, .disconnected) }
    }

    func testEnteringLocalModeClosesRemoteSocketAndRejectsPendingResponse() async throws {
        let socket = try FakeSocket(); socket.reply = false
        let connection = RemoteConnection(socketFactory: { _ in socket })
        connection.connect(try configuration())
        try await waitUntil { connection.state == .online }
        let pending = Task { try await connection.call("list_projects") }
        try await waitUntil { socket.received.count == 1 }
        connection.useLocalService(name: "Offline demo", commands: ["demo_read"]) { _, _, _ in .string("local") }
        do { _ = try await pending.value; XCTFail("Old request must not complete in local mode") }
        catch { XCTAssertEqual(error as? RemoteError, .disconnected) }
        XCTAssertTrue(socket.closed)
        XCTAssertFalse(connection.supports("native_conversation_send"))
        let result = try await connection.call("demo_read")
        XCTAssertEqual(result, .string("local"))
        connection.disconnect()
    }
}

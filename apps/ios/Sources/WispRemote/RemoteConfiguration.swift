import Foundation
import CryptoKit

public struct RemoteConfiguration: Equatable, Sendable {
    public let relay: URL
    public let code: String

    /// Accepts the desktop's /remote#code/p/... link, or a relay base + code.
    /// The fragment is parsed locally and is never present in a network URL.
    public init(address: String, code: String = "") throws {
        guard var parts = URLComponents(string: address.trimmingCharacters(in: .whitespacesAndNewlines)),
              parts.scheme?.lowercased() == "https", let host = parts.host, !host.isEmpty,
              parts.user == nil, parts.password == nil, parts.query == nil,
              parts.port == nil || (1...65535).contains(parts.port!) else { throw RemoteError.invalidAddress }
        let linkCode = (parts.fragment ?? "").split(separator: "/").first.map(String.init) ?? ""
        let supplied = code.trimmingCharacters(in: .whitespacesAndNewlines)
        self.code = try Self.normalizeCode(supplied.isEmpty ? linkCode : supplied)
        parts.scheme = "https"
        parts.fragment = nil
        var path = parts.path
        while path.hasSuffix("/") { path.removeLast() }
        if path.hasSuffix("/remote") { path.removeLast("/remote".count) }
        parts.path = path + "/"
        guard let url = parts.url else { throw RemoteError.invalidAddress }
        relay = url
    }

    public static func normalizeCode(_ text: String) throws -> String {
        let code = text.lowercased().filter { $0 != "-" && !$0.isWhitespace }
        guard code.utf8.count == 32, code.utf8.allSatisfy({ (48...57).contains($0) || (97...102).contains($0) }) else {
            throw RemoteError.invalidCode
        }
        return code
    }

    public var webSocketURL: URL {
        var parts = URLComponents(url: relay.appendingPathComponent("v1/remote/client/\(crypto.sid)"), resolvingAgainstBaseURL: false)!
        parts.scheme = "wss"
        return parts.url!
    }
    public var crypto: RemoteCrypto { RemoteCrypto(normalizedCode: code) }
}

/// Byte-for-byte compatible with crates/wisp-sync/src/remote.rs and WebCrypto.
public struct RemoteCrypto: Sendable {
    public static let hostToClient = Data("wisp-remote/v1/h2c".utf8)
    public static let clientToHost = Data("wisp-remote/v1/c2h".utf8)
    public static let hostFrameLimit = 16 * 1024 * 1024
    public static let clientFrameLimit = 1024 * 1024
    public let sid: String
    private let key: SymmetricKey

    init(normalizedCode: String) {
        let characters = Array(normalizedCode)
        let secret = Data(stride(from: 0, to: characters.count, by: 2).map {
            UInt8(String(characters[$0...($0 + 1)]), radix: 16)!
        })
        let digest = SHA256.hash(data: Data("wisp-remote/sid/v1".utf8) + secret)
        sid = digest.prefix(16).map { String(format: "%02x", $0) }.joined()
        key = SymmetricKey(data: SHA256.hash(data: Data("wisp-remote/key/v1".utf8) + secret))
    }

    public func seal(_ value: RemoteValue) throws -> String {
        let data = try JSONEncoder().encode(value)
        guard data.count < Self.clientFrameLimit else { throw RemoteError.tooLarge }
        let box = try AES.GCM.seal(data, using: key, authenticating: Self.clientToHost)
        guard let combined = box.combined else { throw RemoteError.invalidFrame }
        let frame = combined.base64EncodedString()
        guard frame.utf8.count <= Self.clientFrameLimit else { throw RemoteError.tooLarge }
        return frame
    }

    public func open(_ frame: String) throws -> RemoteValue {
        guard frame.utf8.count <= Self.hostFrameLimit, let data = Data(base64Encoded: frame), data.count >= 28 else {
            throw RemoteError.invalidFrame
        }
        do {
            let box = try AES.GCM.SealedBox(combined: data)
            let plaintext = try AES.GCM.open(box, using: key, authenticating: Self.hostToClient)
            return try JSONDecoder().decode(RemoteValue.self, from: plaintext)
        } catch { throw RemoteError.invalidFrame }
    }
}

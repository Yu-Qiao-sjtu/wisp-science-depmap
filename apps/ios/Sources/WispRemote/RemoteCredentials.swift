import Foundation
import Security

public protocol RemoteCredentialStoring {
    func load() throws -> RemoteConfiguration?
    func save(_ configuration: RemoteConfiguration) throws
    func remove() throws
}

/// Connection codes grant host access. Nothing secret goes in UserDefaults,
/// SQLite, logs, URLs sent to the relay, or synchronizing keychain items.
public struct RemoteCredentials: RemoteCredentialStoring {
    private let service: String
    public init(service: String = "science.wisp.ios.remote") { self.service = service }
    private var query: [String: Any] {
        [kSecClass as String: kSecClassGenericPassword,
         kSecAttrService as String: service, kSecAttrAccount as String: "connection"]
    }
    public func load() throws -> RemoteConfiguration? {
        var query = query
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess, let data = result as? Data else { throw RemoteError.keychain(status) }
        let record = try JSONDecoder().decode(Record.self, from: data)
        return try RemoteConfiguration(address: record.relay, code: record.code)
    }
    public func save(_ configuration: RemoteConfiguration) throws {
        let data = try JSONEncoder().encode(Record(relay: configuration.relay.absoluteString, code: configuration.code))
        let attributes: [String: Any] = [
            kSecValueData as String: data,
            kSecAttrAccessible as String: kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
        ]
        let status = SecItemUpdate(query as CFDictionary, attributes as CFDictionary)
        if status == errSecItemNotFound {
            let added = SecItemAdd(query.merging(attributes) { _, new in new } as CFDictionary, nil)
            guard added == errSecSuccess else { throw RemoteError.keychain(added) }
        } else if status != errSecSuccess { throw RemoteError.keychain(status) }
    }
    public func remove() throws {
        let status = SecItemDelete(query as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else { throw RemoteError.keychain(status) }
    }
    private struct Record: Codable { let relay: String; let code: String }
}

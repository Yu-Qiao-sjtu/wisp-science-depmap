import Foundation

/// Reads the existing wisp-dto payloads without creating a second wire schema.
/// IDs stay strings and integer cursors never pass through a Double.
public enum RemoteValue: Codable, Equatable, Sendable {
    case null, bool(Bool), integer(Int64), unsigned(UInt64), number(Double)
    case string(String), array([RemoteValue]), object([String: RemoteValue])

    public init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() { self = .null }
        else if let value = try? c.decode(Bool.self) { self = .bool(value) }
        else if let value = try? c.decode(Int64.self) { self = .integer(value) }
        else if let value = try? c.decode(UInt64.self) { self = .unsigned(value) }
        else if let value = try? c.decode(Double.self) { self = .number(value) }
        else if let value = try? c.decode(String.self) { self = .string(value) }
        else if let value = try? c.decode([RemoteValue].self) { self = .array(value) }
        else { self = .object(try c.decode([String: RemoteValue].self)) }
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self {
        case .null: try c.encodeNil()
        case .bool(let v): try c.encode(v)
        case .integer(let v): try c.encode(v)
        case .unsigned(let v): try c.encode(v)
        case .number(let v): try c.encode(v)
        case .string(let v): try c.encode(v)
        case .array(let v): try c.encode(v)
        case .object(let v): try c.encode(v)
        }
    }

    public subscript(_ key: String) -> Self {
        if case .object(let value) = self { return value[key] ?? .null }
        return .null
    }
    public var string: String { if case .string(let v) = self { return v }; return "" }
    public var bool: Bool { self == .bool(true) }
    public var array: [Self] { if case .array(let v) = self { return v }; return [] }
    public var uint: UInt64? {
        switch self {
        case .unsigned(let v): return v
        case .integer(let v): return UInt64(exactly: v)
        default: return nil
        }
    }
    public var display: String {
        if case .string(let v) = self { return v }
        guard let data = try? JSONEncoder().encode(self) else { return "" }
        return String(decoding: data, as: UTF8.self)
    }
}

public enum RemoteError: LocalizedError, Equatable {
    case invalidAddress, invalidCode, invalidFrame, incompatibleResponse, disconnected
    case unsupported, tooLarge, outcomeUnknown, service(String), keychain(Int32)

    public var errorDescription: String? {
        switch self {
        case .invalidAddress: return "请输入 HTTPS 中继地址或网页远程访问链接，地址中不能包含用户名、密码或查询参数。"
        case .invalidCode: return "联机码应为 32 位十六进制字符，可包含分隔短横线。"
        case .invalidFrame: return "无法验证远程连接，请检查联机码。"
        case .incompatibleResponse: return "Wisp 主机返回了不兼容的数据，请更新主机。"
        case .disconnected: return "连接已断开。如果刚才执行了操作，请重新连接并确认结果后再重试。"
        case .unsupported: return "当前 Wisp 主机不支持此功能。"
        case .tooLarge: return "内容超过远程连接的单帧大小限制。"
        case .outcomeUnknown: return "请求超时，操作可能已执行。请刷新确认结果后再重试。"
        case .service(let message): return message
        case .keychain: return "无法访问设备钥匙串，请解锁设备后重试。"
        }
    }
}

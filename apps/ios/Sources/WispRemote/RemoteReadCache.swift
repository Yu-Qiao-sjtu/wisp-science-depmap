import Foundation
import CryptoKit

/// Bounded, disposable read cache. All disk work runs off the main actor;
/// credentials, mutation requests and pending approvals are never replayed.
public actor RemoteReadCache {
    public static let shared = RemoteReadCache(directory: FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
        .appendingPathComponent("WispRemoteReads-v1", isDirectory: true))
    public struct Entry: Codable, Equatable, Sendable {
        public let value: RemoteValue
        public let savedAt: Date
    }
    private struct Record: Codable { let scope: String; let key: String; let entry: Entry }
    private struct Metadata: Codable { let scope: String; let bytes: Int; let savedAt: Date; var accessedAt: Date }
    private let directory: URL
    private let capacity: Int
    private let lifetime: TimeInterval
    private let now: @Sendable () -> Date
    private var index: [String: Metadata]?
    private var memory: [String: Entry] = [:]
    private var revision: UInt64 = 0
    private let entryLimit = 8 * 1024 * 1024
    private static let commands: Set<String> = ["list_projects", "remote_sessions", "native_conversation_snapshot",
        "native_conversation_inbox", "native_conversation_panel_file_directory", "native_conversation_panel_file_read",
        "native_conversation_image", "host_metadata"]
    public static func accepts(_ command: String) -> Bool { commands.contains(command) }
    public func currentRevision() -> UInt64 { revision }

    public init(directory: URL, capacity: Int = 50 * 1024 * 1024, lifetime: TimeInterval = 30 * 24 * 3600,
                now: @escaping @Sendable () -> Date = { Date() }) {
        self.directory = directory
        self.capacity = max(0, capacity)
        self.lifetime = lifetime
        self.now = now
    }
    public static func scope(_ configuration: RemoteConfiguration) -> String {
        digest(Data((configuration.relay.absoluteString + "\n" + configuration.crypto.sid).utf8))
    }
    public static func key(scope: String, command: String, project: String? = nil, args: [String: RemoteValue] = [:]) -> String {
        let encoder = JSONEncoder(); encoder.outputFormatting = .sortedKeys
        let value: RemoteValue = .object(["scope": .string(scope), "command": .string(command),
            "project": project.map(RemoteValue.string) ?? .null, "args": .object(args)])
        return digest((try? encoder.encode(value)) ?? Data())
    }
    private static func digest(_ data: Data) -> String { SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined() }

    public func read(scope: String, command: String, project: String? = nil, args: [String: RemoteValue] = [:]) -> Entry? {
        guard Self.commands.contains(command) else { return nil }
        loadIndex()
        guard index != nil else { return nil }
        let key = Self.key(scope: scope, command: command, project: project, args: args)
        guard let metadata = index?[key], metadata.scope == scope else { return nil }
        guard now().timeIntervalSince(metadata.savedAt) < lifetime else { remove(key); saveIndex(); return nil }
        index?[key]?.accessedAt = now()
        if let hit = memory[key] { return hit }
        guard let data = boundedRead(directory.appendingPathComponent(key), limit: entryLimit),
              let record = try? JSONDecoder().decode(Record.self, from: data), record.key == key, record.scope == scope else {
            remove(key); saveIndex(); return nil
        }
        remember(record.entry, key: key)
        return record.entry
    }
    public func store(_ value: RemoteValue, scope: String, command: String, project: String? = nil,
                      args: [String: RemoteValue] = [:], expectedRevision: UInt64? = nil) {
        guard Self.commands.contains(command), expectedRevision == nil || expectedRevision == revision else { return }
        loadIndex()
        guard index != nil else { return }
        let key = Self.key(scope: scope, command: command, project: project, args: args)
        // Live snapshots often poll unchanged every few seconds. Persist at most
        // once per minute in that case, without changing network freshness checks.
        if let hit = memory[key], hit.value == value, now().timeIntervalSince(hit.savedAt) < 60 { return }
        let entry = Entry(value: value, savedAt: now())
        guard let data = try? JSONEncoder().encode(Record(scope: scope, key: key, entry: entry)),
              data.count <= min(capacity, entryLimit) else { remove(key); saveIndex(); return }
        do {
            try prepareDirectory()
            try write(data, to: directory.appendingPathComponent(key))
            index?[key] = Metadata(scope: scope, bytes: data.count, savedAt: entry.savedAt, accessedAt: entry.savedAt)
            remember(entry, key: key)
            prune()
            saveIndex()
        } catch { /* Cache failures must not fail a successful remote read. */ }
    }
    public func clear(scope: String? = nil) {
        revision &+= 1
        loadIndex()
        for (key, metadata) in index ?? [:] where scope == nil || metadata.scope == scope { remove(key) }
        saveIndex()
    }
    public func size() -> Int { loadIndex(); prune(); saveIndex(); return (index ?? [:]).values.reduce(0) { $0 + $1.bytes } }

    private func loadIndex() {
        guard index == nil else { return }
        let data = boundedRead(directory.appendingPathComponent("index.json"), limit: 1024 * 1024)
        // An existing index may be temporarily unreadable while iOS locks its
        // protected files. Do not interpret that as corruption and delete it.
        if data == nil && FileManager.default.fileExists(atPath: directory.appendingPathComponent("index.json").path) { return }
        let loaded = data.flatMap { try? JSONDecoder().decode([String: Metadata].self, from: $0) } ?? [:]
        index = loaded.filter { key, value in
            key.count == 64 && key.utf8.allSatisfy { (48...57).contains($0) || (97...102).contains($0) }
                && value.bytes >= 0 && value.bytes <= entryLimit
        }
        // Recover from interrupted writes / corrupt manifests; only this private
        // cache directory is scanned, never user files or scientific datasets.
        for url in (try? FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil)) ?? [] {
            if url.lastPathComponent != "index.json" && index?[url.lastPathComponent] == nil { try? FileManager.default.removeItem(at: url) }
        }
        prune()
    }
    private func prune() {
        for (key, metadata) in index ?? [:] where now().timeIntervalSince(metadata.savedAt) >= lifetime { remove(key) }
        var bytes = (index ?? [:]).values.reduce(0) { $0 + $1.bytes }
        for (key, metadata) in (index ?? [:]).sorted(by: { $0.value.accessedAt < $1.value.accessedAt }) {
            if bytes <= capacity && (index?.count ?? 0) <= 500 { break }
            remove(key); bytes -= metadata.bytes
        }
    }
    private func remember(_ entry: Entry, key: String) {
        memory[key] = entry
        var bytes = memory.keys.reduce(0) { $0 + (index?[$1]?.bytes ?? 0) }
        for old in memory.keys.sorted(by: { (index?[$0]?.accessedAt ?? .distantPast) < (index?[$1]?.accessedAt ?? .distantPast) }) {
            if bytes <= 12 * 1024 * 1024 { break }
            memory.removeValue(forKey: old); bytes -= index?[old]?.bytes ?? 0
        }
    }
    private func remove(_ key: String) {
        index?.removeValue(forKey: key); memory.removeValue(forKey: key)
        try? FileManager.default.removeItem(at: directory.appendingPathComponent(key))
    }
    private func prepareDirectory() throws {
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        var url = directory
        var values = URLResourceValues(); values.isExcludedFromBackup = true
        try url.setResourceValues(values)
    }
    private func saveIndex() {
        guard let index, let data = try? JSONEncoder().encode(index) else { return }
        do { try prepareDirectory(); try write(data, to: directory.appendingPathComponent("index.json")) } catch { }
    }
    private func write(_ data: Data, to url: URL) throws {
        #if os(iOS)
        try data.write(to: url, options: [.atomic, .completeFileProtection])
        #else
        try data.write(to: url, options: .atomic)
        #endif
    }
    private func boundedRead(_ url: URL, limit: Int) -> Data? {
        guard let file = try? FileHandle(forReadingFrom: url) else { return nil }
        defer { try? file.close() }
        guard let data = try? file.read(upToCount: limit + 1), data.count <= limit else { return nil }
        return data
    }
}

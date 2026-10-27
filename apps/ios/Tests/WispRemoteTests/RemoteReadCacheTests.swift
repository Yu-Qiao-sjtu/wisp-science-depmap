import XCTest
@testable import WispRemote

final class RemoteReadCacheTests: XCTestCase {
    private func directory() throws -> URL {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return url
    }
    func testReadsSurviveRelaunchAndAreIsolatedByServerProjectSessionAndPath() async throws {
        let root = try directory()
        let first = RemoteReadCache(directory: root)
        let config = try RemoteConfiguration(address: "https://one.example.test", code: testCode)
        let other = try RemoteConfiguration(address: "https://two.example.test", code: testCode)
        let scope = RemoteReadCache.scope(config)
        XCTAssertNotEqual(scope, RemoteReadCache.scope(other))
        let args: [String: RemoteValue] = ["session_id": .string("s"), "path": .string("report.md")]
        let value: RemoteValue = .object(["text": .string("cached report")])
        await first.store(value, scope: scope, command: "native_conversation_panel_file_read", project: "p", args: args)
        let relaunched = RemoteReadCache(directory: root)
        let hit = await relaunched.read(scope: scope, command: "native_conversation_panel_file_read", project: "p", args: args)
        XCTAssertEqual(hit?.value, value)
        for (server, project, session, path) in [(RemoteReadCache.scope(other), "p", "s", "report.md"),
            (scope, "other", "s", "report.md"), (scope, "p", "other", "report.md"), (scope, "p", "s", "other.md")] {
            let miss = await relaunched.read(scope: server, command: "native_conversation_panel_file_read", project: project,
                args: ["session_id": .string(session), "path": .string(path)])
            XCTAssertNil(miss)
        }
        let text = try String(contentsOf: root.appendingPathComponent("index.json"), encoding: .utf8)
        XCTAssertFalse(text.contains(config.code))
        XCTAssertFalse(text.contains(config.relay.absoluteString))
        XCTAssertFalse(text.contains("report.md"))
    }
    func testExpiredOversizedCorruptAndMutationEntriesAreNotReturned() async throws {
        let root = try directory()
        let time = Date(timeIntervalSince1970: 1000)
        let cache = RemoteReadCache(directory: root, capacity: 2048, lifetime: 60, now: { time })
        await cache.store(.string("valid"), scope: "a", command: "list_projects")
        let expired = RemoteReadCache(directory: root, capacity: 2048, lifetime: 60, now: { time.addingTimeInterval(61) })
        let old = await expired.read(scope: "a", command: "list_projects")
        XCTAssertNil(old)
        await cache.store(.string(String(repeating: "x", count: 3000)), scope: "b", command: "list_projects")
        let large = await cache.read(scope: "b", command: "list_projects")
        XCTAssertNil(large)
        await cache.store(.string("must not persist"), scope: "a", command: "native_conversation_send")
        let mutation = await cache.read(scope: "a", command: "native_conversation_send")
        XCTAssertNil(mutation)
        await cache.store(.string("file"), scope: "a", command: "remote_sessions")
        let key = RemoteReadCache.key(scope: "a", command: "remote_sessions")
        try Data("broken json".utf8).write(to: root.appendingPathComponent(key))
        let damaged = RemoteReadCache(directory: root, now: { time })
        let corrupt = await damaged.read(scope: "a", command: "remote_sessions")
        XCTAssertNil(corrupt)
    }
    func testCapacityClearAndInFlightWriteInvalidation() async throws {
        let cache = RemoteReadCache(directory: try directory(), capacity: 1400)
        for scope in ["a", "b", "c"] {
            await cache.store(.string(String(repeating: scope, count: 500)), scope: scope, command: "list_projects")
        }
        let bytes = await cache.size()
        XCTAssertLessThanOrEqual(bytes, 1400)
        let newest = await cache.read(scope: "c", command: "list_projects")
        XCTAssertNotNil(newest)
        let revision = await cache.currentRevision()
        await cache.clear(scope: "c")
        await cache.store(.string("late network reply"), scope: "c", command: "list_projects", expectedRevision: revision)
        let cleared = await cache.read(scope: "c", command: "list_projects")
        XCTAssertNil(cleared)
        await cache.clear()
        let remaining = await cache.size()
        XCTAssertEqual(remaining, 0)
    }
    func testKeyCanonicalizesArgumentOrderAndSeparatesCommands() {
        let left: [String: RemoteValue] = ["session_id": .string("s"), "path": .string("a")]
        let right: [String: RemoteValue] = ["path": .string("a"), "session_id": .string("s")]
        XCTAssertEqual(RemoteReadCache.key(scope: "a", command: "read", args: left), RemoteReadCache.key(scope: "a", command: "read", args: right))
        XCTAssertNotEqual(RemoteReadCache.key(scope: "a", command: "read", args: left), RemoteReadCache.key(scope: "a", command: "list", args: left))
    }
}

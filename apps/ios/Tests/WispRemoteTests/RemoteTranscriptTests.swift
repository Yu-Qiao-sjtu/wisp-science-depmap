import XCTest
@testable import WispRemote

final class RemoteTranscriptTests: XCTestCase {
    private func item(_ role: String, _ text: String = "", tool: String = "", ok: Bool? = nil) -> RemoteValue {
        .object(["role": .string(role), "text": .string(text), "tool_name": .string(tool),
                 "ok": ok.map(RemoteValue.bool) ?? .null])
    }

    func testToolStepsAndChangedPathsCollapseWithoutHidingTheAnswer() {
        let items = [
            item("user", "绘制表达热图"), item("reasoning", "检查数据"), item("assistant", "准备绘图"),
            item("tool", "done", tool: "shell", ok: true),
            item("file_changed", "figures/heatmap.png"), item("file_changed", "figures/heatmap.pdf"),
            item("acp_tool", "missing", tool: "read", ok: false),
            item("file_changed", "figures/heatmap.png"),
            item("assistant", "# 分析完成\n\n[查看报告](results/report.md)"), item("usage", "{}"),
        ]
        let groups = RemoteTranscript.groups(items)
        XCTAssertEqual(groups.map(\.isProcess), [false, true, false, false])
        XCTAssertEqual(groups[1].toolCount, 2)
        XCTAssertEqual(groups[1].changedFileCount, 2, "Count distinct paths, not write notifications")
        XCTAssertEqual(groups[1].failureCount, 1)
        XCTAssertEqual(groups[2].items, [items[8]])
        XCTAssertEqual(groups.flatMap(\.items), items, "Grouping must preserve every item and its order")
    }

    func testQuestionsPlansWarningsAndUnknownRolesRemainVisibleAndSeparateTurns() {
        for role in ["question", "plan", "system", "user", "queued_user", "review", "future-role"] {
            let boundary = item(role, "需要你查看")
            let items = [item("tool", tool: "read"), boundary, item("tool", tool: "edit")]
            let groups = RemoteTranscript.groups(items)
            XCTAssertEqual(groups.map(\.isProcess), [true, false, true], role)
            XCTAssertEqual(groups[1].items, [boundary], role)
        }
    }

    func testResultToolsAndAssistantResourcesAreNotFoldedAway() {
        for tool in ["attempt_completion", "monitor_run", "wisp_monitor_run", "generate_image", "generate_video"] {
            let items = [item("reasoning"), item("tool", tool: tool, ok: true), item("file_changed", "figure.png")]
            XCTAssertEqual(RemoteTranscript.groups(items).map(\.isProcess), [true, false, true], tool)
        }
        let resource: RemoteValue = .object(["role": .string("assistant"), "text": .string("![图](fig.png)"),
                                           "resources": .array([.object(["id": .string("image-a")])])])
        XCTAssertEqual(RemoteTranscript.groups([resource, item("tool", tool: "read")]).map(\.isProcess), [false, true])
    }

    func testCurrentCommentaryRemainsReadableWhilePastWorkStaysGrouped() {
        let items = [item("user", "上一轮"), item("assistant", "旧进度"), item("tool", tool: "read"),
                     item("assistant", "旧回答"), item("user", "这一轮"), item("assistant", "正在绘图"),
                     item("tool", tool: "shell"), item("file_changed", "figure.png")]
        let live = RemoteTranscript.groups(items, running: true)
        XCTAssertEqual(live.map(\.isProcess), [false, true, false, false, false, true])
        XCTAssertEqual(live[4].items, [items[5]])
        let finished = RemoteTranscript.groups(items)
        XCTAssertEqual(finished.last?.items, Array(items[5...]))
        XCTAssertTrue(RemoteTranscript.groups([], running: true).isEmpty)
    }

    func testGroupIdentitySurvivesAppendedStepsAndEarlierPages() {
        let live = [item("user", "当前问题"), item("tool", tool: "shell")]
        let original = RemoteTranscript.groups(live)
        let extended = RemoteTranscript.groups(live + [item("tool", tool: "read"), item("file_changed", "data.csv")])
        XCTAssertEqual(original.last?.id, extended.last?.id)
        let older = [item("user", "旧问题"), item("tool", tool: "read"), item("assistant", "旧回答")]
        let paged = RemoteTranscript.groups(older + live, indexOffset: -older.count)
        XCTAssertEqual(Array(paged.suffix(original.count)), original)
        XCTAssertEqual(Set(paged.map(\.id)).count, paged.count)
    }
}

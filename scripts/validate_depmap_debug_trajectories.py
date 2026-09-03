#!/usr/bin/env python3
"""Offline regression checks for exported DepMap Agent debug trajectories.

This script does not run the Rust application or an LLM. It proves two things:
the exported HTML still contains the audited failure signatures, and the current
source tree contains the corresponding guard contracts. Executable/model
behavior must be re-tested after a later authorized build.
"""

from __future__ import annotations

import argparse
import html
import json
from dataclasses import dataclass
from pathlib import Path
import re
import sys


@dataclass(frozen=True)
class Signature:
    name: str
    pattern: str
    minimum: int = 1
    regex: bool = False

    def count(self, text: str) -> int:
        if self.regex:
            return len(re.findall(self.pattern, text, flags=re.IGNORECASE | re.DOTALL))
        return text.count(self.pattern)


TRAJECTORY_SIGNATURES = {
    "wisp_debug-2": [
        Signature("unnormalized Breast Cancer lineage", '"lineage":"Breast Cancer"'),
        Signature("false lineage coverage gaps", "NOT_COMPUTED"),
        Signature("unsupported bimodality inference", "双峰"),
        Signature("unsupported receptor/subtype inference", "ER+"),
        Signature(
            "broad spill-directory grep",
            r'grep \{[^\n]*"path":"D:\\\\New-PHD\\\\depmap\\\\\.wisp\\\\tool-output"',
            regex=True,
        ),
        Signature("unrelated TP53 contamination", "TP53"),
    ],
    "wisp_debug-3": [
        Signature("historical Run preload", "depmap_project_runs"),
        Signature("repeated empty depmap_query", "depmap_query {}", minimum=5),
        Signature("invented APC anchor", '"gene":"APC"'),
        Signature("unnormalized Colorectal lineage", '"lineage":"Colorectal"'),
        Signature("mean difference mislabeled as rho", "ρ"),
        Signature("new Wilcoxon work called queryable", "Wilcoxon"),
        Signature("unsupported named drug", "venetoclax"),
    ],
    "wisp_debug-4": [
        Signature("historical Run preload", "depmap_project_runs"),
        Signature("Workflow capability failure", "capability is disabled or unavailable: depmap_read"),
        Signature("manual Workflow reconstruction", "手工执行同等流程"),
        Signature("repeated empty depmap_query", "depmap_query {}", minimum=5),
        Signature("stale lineage rank", "17/33"),
        Signature("stale lineage FDR", "FDR=0.70"),
        Signature("file presence promoted to computability", "10 个 coverage_gap 均可本地计算"),
        Signature("damaging events mislabeled pathogenic", "致病突变"),
        Signature("non-significant list seeded mechanism", "蛋白酶体"),
        Signature("unsupported drug mechanism", "HSP90"),
        Signature("clinical scope silently narrowed", "HCC"),
        Signature("unrequested control lineage", "Biliary Tract"),
        Signature("unsupported named cell lines", "HepG2"),
        Signature("unapproved report write", "results/reports/ATF5_liver_cancer_topic_design.md"),
    ],
    "wisp_debug-5": [
        Signature("Workflow capability failure", "capability is disabled or unavailable: depmap_read"),
        Signature("forbidden fallback after Workflow block", "已按规程改为"),
        Signature("current-turn-only claim", "所有数值均来自本轮查询返回"),
        Signature("stale prior-session FDR", "FDR=0.70"),
        Signature("stale prior-session P value", "p=0.58"),
        Signature("cross-module only-significant contradiction", "唯一通过 FDR"),
        Signature("unsupported high subgroup", "ATF5-high"),
        Signature("eligibility promoted to infeasibility", "突变/CNV 路线不可行"),
        Signature("descriptive cutoff promoted to categorical claim", "不是肝癌的直接依赖"),
        Signature("clinical scope silently narrowed", "HCC"),
        Signature("unapproved report write", "results/reports/ATF5_liver_topic_design.md"),
    ],
    "wisp_debug-6": [
        Signature(
            "novelty task hit the hard wall-time",
            "delegated Agent timed out after 600 seconds",
        ),
        Signature(
            "downstream tasks blocked by novelty failure",
            "novelty_landscape did not succeed",
        ),
        Signature("duplicate ad-hoc mechanism search", "lit-mechanism"),
        Signature("duplicate ad-hoc therapy search", "lit-therapy"),
        Signature("parent manually delegated after Workflow approval", "delegate_tasks"),
    ],
    "wisp_debug-7": [
        Signature("cancer-only request tried anchor-dependent top mode", '"lineage":"Breast","mode":"top"'),
        Signature("schema learned by repeated missing-source failures", "mode requires non-empty 'source'", minimum=3),
        Signature("ordinary exploration escalated to Workflow", "start_workflow"),
        Signature("incorrect left-side Agents guidance", "本对话左侧的 **Agents 面板**"),
        Signature("delegated literature task hit fixed 600-second wall", "delegated Agent timed out after 600 seconds"),
        Signature("manual delegation appeared after Workflow execution", "delegate_tasks"),
    ],
    "wisp_debug-8-不同描述压力测试": [
        Signature("cancer-only ranking tried top without module", '"lineage":"Breast","limit":15,"mode":"top"'),
        Signature("top mode then failed without source", "mode requires non-empty 'source'"),
        Signature("lineage event mode used for dependency ranking", '"lineage":"Breast","limit":10,"mode":"lineage"'),
        Signature("core mode attempted without gene", "mode requires non-empty 'gene'"),
        Signature("precomputed ranking was misclassified as new computation", "No raw matrix is opened. Ranking = user-approved new computation on one precomputed table."),
        Signature("repair Run v2", "Breast selective dependency top-10 (26Q1) v2 contract"),
        Signature("repair Run v3", "Breast selective dependency top-10 (26Q1) v3"),
        Signature("repair Run v4", "Breast selective dependency top-10 (26Q1) v4 final"),
    ],
    "wisp_debug-9": [
        Signature("request correctly entered cancer direction routing", '"intent":"cancer_direction_discovery"'),
        Signature("direction request repeatedly used dependency ranking", '"mode":"lineage_dependency"', minimum=5),
        Signature("remote contract rejected the wrong mode", "422 Unprocessable Entity", minimum=5),
        Signature("schema was probed with incomplete source queries", "mode requires non-empty 'source'", minimum=1),
        Signature("provider advertised the unused direction endpoint", "lineage_directions", minimum=1),
        Signature("guessed SOX2 anchor", "SOX2", minimum=1),
        Signature("guessed NANOG anchor", "NANOG", minimum=1),
        Signature("guessed SALL4 anchor", "SALL4", minimum=1),
        Signature("guessed YAP1 anchor", "YAP1", minimum=1),
    ],
    "wisp_debug-10": [
        Signature("direction request inherited the rejected provider contract", "422 Unprocessable Entity", minimum=5),
        Signature("continuous correlation promoted to TF-high screen", "TF-high synthetic dependency screen"),
        Signature("negative search promoted to nobody-has-done-it", "没人做", minimum=2),
        Signature("negative search promoted to unique novelty", "唯一明确空白"),
        Signature("provisional citation was later corrected", "实为 SMARCA2 降解剂论文"),
        Signature("query-only support mapping used remote Runs", "run_in_context", minimum=3),
        Signature("guessed remote knowledge path failed", "No such file or directory"),
        Signature("installed modules promoted to direct support", "五个章节全部有直接数据支撑"),
    ],
}


SOURCE_GUARDS = [
    ("src-tauri/src/depmap_agent.rs", "canonical_lineage_label"),
    ("src-tauri/src/depmap_agent.rs", "fn core_focus"),
    ("src-tauri/src/depmap_agent.rs", '"focus": {"core": focus_core}'),
    ("src-tauri/src/depmap_agent.rs", '"semantics": query_semantics'),
    ("src-tauri/src/specialists.rs", "call `start_workflow` before any evidence query"),
    ("src-tauri/src/specialists.rs", "do not repeat the identical call"),
    ("src-tauri/src/specialists.rs", "Do not relabel `damaging_mutation_n`"),
    ("src-tauri/src/specialists.rs", "model-grouping proxy"),
    ("src-tauri/src/quick_actions.rs", '"manual_fallback_allowed": false'),
    ("src-tauri/src/quick_actions.rs", ".stop_turn()"),
    ("src-tauri/src/quick_actions.rs", "Exact recent user requests (newest first; verbatim; do not broaden or narrow them)"),
    ("src-tauri/src/quick_actions.rs", "mode=lineage_catalog"),
    ("src-tauri/src/quick_actions.rs", "non-significant top list is a null result"),
    ("skills/depmap-knowledge-query/SKILL.md", "Skill text and model memory are not literature evidence"),
    ("crates/wisp-cli/eval-suites/depmap-agent-v1.yaml", "registered-topic-workflow-routes-before-evidence"),
    ("crates/wisp-cli/eval-suites/depmap-agent-v1.yaml", "nonsignificant-top-list-remains-null"),
    ("src-tauri/src/delegation_runtime.rs", "available_skills: resources"),
    ("src-tauri/src/depmap_agent.rs", "compact_evidence_result"),
    ("src-tauri/src/depmap_agent.rs", "const MAX_EVIDENCE_LIMIT: i64 = 3"),
    ("skills/depmap-knowledge-query/SKILL.md", "Continuous expression-to-dependency"),
    ("src-tauri/src/quick_actions.rs", "Built-in templates ship unlimited budgets"),
    ("src-tauri/src/quick_actions.rs", "do not manually duplicate"),
    ("crates/wisp-store/src/sessions.rs", "frame_message_activity"),
    ("ui/src/agent_workflows.rs", "activity_messages"),
    ("src-tauri/src/specialists.rs", "`mode=lineage_dependency`"),
    ("src-tauri/src/specialists.rs", "must not be escalated to a new Run"),
    ("src-tauri/src/depmap_agent.rs", '"intent":"cancer_dependency_ranking"'),
    ("src-tauri/src/depmap_agent.rs", '| "lineage_directions" => &["lineage"]'),
    ("src-tauri/src/depmap_agent.rs", '"mode": "lineage_directions"'),
    ("src-tauri/src/depmap_agent.rs", '"retry_same_mode": false'),
    ("crates/wisp-cli/eval-suites/depmap-agent-v1.yaml", "liver-cancer-direction-discovery-without-anchor"),
    ("src-tauri/src/depmap_agent.rs", '"study_support_mapping"'),
    ("src-tauri/src/specialists.rs", "exactly one of four buckets"),
    ("src-tauri/src/specialists.rs", "Keep a claim ledger"),
    ("src-tauri/src/specialists.rs", "not found within the searched"),
    ("src-tauri/src/specialists.rs", "continuous expression-dependency correlation is not"),
    ("src-tauri/src/quick_actions.rs", '"claim_ledger"'),
    ("crates/wisp-cli/eval-suites/depmap-agent-v1.yaml", "liver-study-support-mapping-separates-existing-from-proposed"),
    ("crates/wisp-cli/eval-suites/depmap-agent-v1.yaml", "continuous-correlation-is-not-tf-high-synthetic-lethality"),
    ("ui/src/main.rs", 'matches!(name.as_str(), "delegate_tasks" | "start_workflow")'),
    ("crates/wisp-core/src/delegation.rs", "pub timeout_secs: Option<u64>"),
]


def normalized_html(path: Path) -> str:
    raw = path.read_text(encoding="utf-8")
    return html.unescape(raw).replace("&nbsp;", " ")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--trajectory-dir",
        type=Path,
        default=Path(r"D:\wisp_agent"),
        help="Directory containing wisp_debug-2 through wisp_debug-8",
    )
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=Path(__file__).resolve().parents[1],
    )
    args = parser.parse_args()

    result: dict[str, object] = {
        "schema": "wisp.depmap-debug-offline-regression.v1",
        "scope": "trajectory-signatures-and-source-contracts",
        "compiled_executable_tested": False,
        "trajectories": {},
        "source_guards": [],
    }
    failed = False

    trajectory_results: dict[str, object] = {}
    for filename, signatures in TRAJECTORY_SIGNATURES.items():
        path = args.trajectory_dir / filename
        if not path.is_file():
            trajectory_results[filename] = {"status": "missing", "path": str(path)}
            failed = True
            continue
        text = normalized_html(path)
        checks = []
        for signature in signatures:
            count = signature.count(text)
            passed = count >= signature.minimum
            checks.append(
                {
                    "name": signature.name,
                    "count": count,
                    "minimum": signature.minimum,
                    "passed": passed,
                }
            )
            failed |= not passed
        trajectory_results[filename] = {
            "status": "passed" if all(item["passed"] for item in checks) else "failed",
            "path": str(path),
            "bytes": path.stat().st_size,
            "checks": checks,
        }
    result["trajectories"] = trajectory_results

    guard_results = []
    for relative, marker in SOURCE_GUARDS:
        path = args.repo_root / relative
        source = path.read_text(encoding="utf-8") if path.is_file() else ""
        # Rust prompt constants use trailing backslashes to join source lines.
        # Remove only that source-level continuation for semantic marker checks.
        source = source.replace("\\\r\n", "").replace("\\\n", "")
        present = marker in source
        guard_results.append(
            {"file": relative, "marker": marker, "passed": present}
        )
        failed |= not present
    result["source_guards"] = guard_results
    result["status"] = "passed" if not failed else "failed"
    result["remaining_gate"] = (
        "Build the Windows EXE only when requested, then replay the audited prompts to test actual model behavior."
    )
    print(json.dumps(result, ensure_ascii=False, indent=2))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""Classify DepMap PR impact and verify deployment contract attestations.

Ordinary PR checks are deliberately offline.  The only network-capable command
is ``probe``; it calls ``depmap_status`` through an already-authorized MCP URL
and writes a portable, path-free attestation for deployment/release gates.
"""

from __future__ import annotations

import argparse
import asyncio
from dataclasses import asdict, dataclass
from datetime import datetime, timedelta, timezone
import json
import os
from pathlib import Path
import re
import subprocess
import sys
from typing import Any, Iterable


ATTESTATION_SCHEMA = "wisp.depmap-contract-attestation.v1"
CLIENT_CONTRACT_PATH = "src-tauri/src/depmap_agent.rs"
PROVIDER_CONTRACT_PATH = "services/depmap_api/app.py"
LIVE_ATTESTATION_PATH = ".github/depmap-live-contract.json"
UNUSABLE_IDENTITIES = {"catalog-missing", "catalog-unreadable"}

PROVIDER_PREFIXES = (
    "services/depmap_api/",
    "services/depmap_mcp/",
)
PROVIDER_FILES = {
    "scripts/build_depmap_query_index.py",
}
CLIENT_FILES = {
    CLIENT_CONTRACT_PATH,
}

ISSUE_LINK_RE = re.compile(
    r"(?im)\b(?:close[sd]?|fix(?:e[sd])?|resolve[sd]?|refs?|relate[sd]?\s+to)"
    r"\s*:?[ \t]*(?:[\w.-]+/[\w.-]+)?#\d+\b"
)
CLIENT_MIN_RE = re.compile(
    r"\bDEPMAP_QUERY_CONTRACT_MIN\s*:\s*u64\s*=\s*(\d+)\s*;"
)
CLIENT_MAX_RE = re.compile(
    r"\bDEPMAP_QUERY_CONTRACT_MAX\s*:\s*u64\s*=\s*(\d+)\s*;"
)
PROVIDER_VERSION_RE = re.compile(r"\bQUERY_CONTRACT_VERSION\s*=\s*(\d+)\b")


class GateError(RuntimeError):
    """A typed, user-actionable gate failure."""


@dataclass(frozen=True)
class ContractRange:
    minimum: int
    maximum: int

    def contains(self, version: int) -> bool:
        return self.minimum <= version <= self.maximum

    def is_superset_of(self, other: "ContractRange") -> bool:
        return self.minimum <= other.minimum and self.maximum >= other.maximum


@dataclass(frozen=True)
class ContractAssessment:
    compatible: bool
    code: str
    server_version: int | None
    client_minimum: int
    client_maximum: int
    server_build_identity: str | None
    capability_catalog_digest: str | None
    catalog_build_identity: str | None
    reasons: tuple[str, ...]


@dataclass(frozen=True)
class PrImpact:
    classification: str
    issue_linked: bool
    merge_allowed: bool
    merge_gate: str
    live_gate: str
    base_client: ContractRange
    head_client: ContractRange
    base_provider_version: int
    head_provider_version: int
    provider_paths_changed: bool
    client_paths_changed: bool
    reasons: tuple[str, ...]


def _one_match(pattern: re.Pattern[str], text: str, label: str) -> int:
    matches = pattern.findall(text)
    if len(matches) != 1:
        raise GateError(f"expected exactly one {label}, found {len(matches)}")
    return int(matches[0])


def parse_client_range(source: str) -> ContractRange:
    result = ContractRange(
        _one_match(CLIENT_MIN_RE, source, "DEPMAP_QUERY_CONTRACT_MIN"),
        _one_match(CLIENT_MAX_RE, source, "DEPMAP_QUERY_CONTRACT_MAX"),
    )
    if result.minimum > result.maximum:
        raise GateError("client contract minimum exceeds maximum")
    return result


def parse_provider_version(source: str) -> int:
    return _one_match(PROVIDER_VERSION_RE, source, "QUERY_CONTRACT_VERSION")


def pr_links_issue(body: str | None) -> bool:
    return bool(body and ISSUE_LINK_RE.search(body))


def _is_provider_path(path: str) -> bool:
    normalized = path.replace("\\", "/")
    return normalized in PROVIDER_FILES or normalized.startswith(PROVIDER_PREFIXES)


def _is_client_path(path: str) -> bool:
    return path.replace("\\", "/") in CLIENT_FILES


def classify_pr(
    *,
    body: str | None,
    changed_paths: Iterable[str],
    base_client: ContractRange,
    head_client: ContractRange,
    base_provider_version: int,
    head_provider_version: int,
    live_assessment: ContractAssessment | None = None,
) -> PrImpact:
    paths = tuple(path.replace("\\", "/") for path in changed_paths)
    issue_linked = pr_links_issue(body)
    provider_changed = any(_is_provider_path(path) for path in paths)
    client_changed = any(_is_client_path(path) for path in paths)
    client_boundary_changed = head_client != base_client
    provider_boundary_changed = head_provider_version != base_provider_version
    reasons: list[str] = []

    if not issue_linked:
        classification = "not_issue_fix"
        merge_allowed = True
        merge_gate = "ordinary_ci"
        live_gate = "none"
    elif not provider_changed and not client_changed:
        classification = "local_only"
        merge_allowed = True
        merge_gate = "ordinary_ci"
        live_gate = "none"
    elif client_boundary_changed and provider_boundary_changed:
        classification = "mixed_provider_client_boundary"
        merge_allowed = False
        merge_gate = "split_pr"
        live_gate = "not_applicable"
        reasons.append(
            "provider and client contract boundaries moved in one issue-fix PR"
        )
    elif client_boundary_changed or client_changed:
        if head_client == base_client:
            classification = "client_current_contract"
            merge_allowed = head_client.contains(head_provider_version)
            merge_gate = "offline_comparator_and_ci"
            live_gate = "release"
            if not merge_allowed:
                reasons.append("repository provider version is outside the client range")
        elif head_client.is_superset_of(base_client):
            classification = "contract_expansion_client"
            merge_allowed = head_client.contains(base_provider_version)
            merge_gate = "offline_comparator_and_ci"
            live_gate = "none_until_provider_deploy"
            if not merge_allowed:
                reasons.append("expanded client range dropped the current provider version")
        else:
            classification = "contract_removal_client"
            merge_gate = "fresh_live_attestation"
            live_gate = "pr_and_release"
            repository_compatible = head_client.contains(head_provider_version)
            live_compatible = bool(live_assessment and live_assessment.compatible)
            merge_allowed = repository_compatible and live_compatible
            if not repository_compatible:
                reasons.append(
                    "repository provider version is outside the contracted client range"
                )
            if not live_compatible:
                reasons.append(
                    "contract removal requires a fresh compatible live attestation"
                )
    else:
        if head_provider_version == base_provider_version:
            classification = "provider_backward_compatible"
            merge_allowed = head_client.contains(head_provider_version)
            merge_gate = "offline_service_suites"
            live_gate = "deployment_and_release"
            if not merge_allowed:
                reasons.append("provider version is outside the client range")
        elif head_provider_version > base_provider_version:
            classification = "contract_expansion_provider"
            merge_allowed = head_client.contains(base_provider_version) and head_client.contains(
                head_provider_version
            )
            merge_gate = "offline_service_suites"
            live_gate = "deployment_and_release"
            if not merge_allowed:
                reasons.append(
                    "expand the client to accept both current and next provider versions first"
                )
        else:
            classification = "provider_contract_rollback"
            merge_allowed = False
            merge_gate = "explicit_migration_plan"
            live_gate = "deployment_and_release"
            reasons.append("provider contract rollback is not an in-place compatible change")

    return PrImpact(
        classification=classification,
        issue_linked=issue_linked,
        merge_allowed=merge_allowed,
        merge_gate=merge_gate,
        live_gate=live_gate,
        base_client=base_client,
        head_client=head_client,
        base_provider_version=base_provider_version,
        head_provider_version=head_provider_version,
        provider_paths_changed=provider_changed,
        client_paths_changed=client_changed,
        reasons=tuple(reasons),
    )


def _nonempty_string(value: Any) -> str | None:
    if not isinstance(value, str):
        return None
    stripped = value.strip()
    return stripped or None


def status_evidence(document: Any) -> dict[str, Any]:
    if not isinstance(document, dict):
        raise GateError("depmap_status payload must be a JSON object")
    if document.get("schema") == ATTESTATION_SCHEMA:
        document = document.get("status")
        if not isinstance(document, dict):
            raise GateError("attestation status must be a JSON object")
    structured = document.get("structuredContent") or document.get("structured_content")
    if isinstance(structured, dict):
        document = structured
    evidence = document.get("evidence")
    if isinstance(evidence, dict):
        return evidence
    return document


def assess_status(document: Any, client: ContractRange) -> ContractAssessment:
    evidence = status_evidence(document)
    raw_version = evidence.get("query_contract_version")
    version = raw_version if isinstance(raw_version, int) and not isinstance(raw_version, bool) else None
    server_build = _nonempty_string(evidence.get("server_build_identity"))
    capability_digest = _nonempty_string(evidence.get("capability_catalog_digest"))
    catalog_build = _nonempty_string(evidence.get("catalog_build_identity"))
    reasons: list[str] = []
    if version is None:
        reasons.append("query_contract_version is missing or not an integer")
    elif not client.contains(version):
        reasons.append(
            f"server contract {version} is outside client range "
            f"{client.minimum}..={client.maximum}"
        )
    for label, identity in (
        ("server_build_identity", server_build),
        ("capability_catalog_digest", capability_digest),
        ("catalog_build_identity", catalog_build),
    ):
        if identity is None:
            reasons.append(f"{label} is missing")
        elif identity.lower() in UNUSABLE_IDENTITIES:
            reasons.append(f"{label} is unusable ({identity})")
    compatible = not reasons
    code = "COMPATIBLE" if compatible else (
        "INCOMPATIBLE_PROVIDER"
        if version is not None and version > client.maximum
        else "STALE_CONTRACT"
    )
    return ContractAssessment(
        compatible=compatible,
        code=code,
        server_version=version,
        client_minimum=client.minimum,
        client_maximum=client.maximum,
        server_build_identity=server_build,
        capability_catalog_digest=capability_digest,
        catalog_build_identity=catalog_build,
        reasons=tuple(reasons),
    )


def portable_attestation_status(document: Any) -> dict[str, Any]:
    """Keep deployment identities only; never persist provider-local metadata."""
    evidence = status_evidence(document)
    return {
        "evidence": {
            key: evidence.get(key)
            for key in (
                "query_contract_version",
                "server_build_identity",
                "capability_catalog_digest",
                "catalog_build_identity",
            )
        }
    }


def parse_observed_at(document: Any) -> datetime:
    if not isinstance(document, dict) or document.get("schema") != ATTESTATION_SCHEMA:
        raise GateError(f"expected {ATTESTATION_SCHEMA} attestation")
    raw = document.get("observed_at")
    if not isinstance(raw, str):
        raise GateError("attestation observed_at is missing")
    try:
        parsed = datetime.fromisoformat(raw.replace("Z", "+00:00"))
    except ValueError as error:
        raise GateError("attestation observed_at is invalid") from error
    if parsed.tzinfo is None:
        raise GateError("attestation observed_at must include a timezone")
    return parsed.astimezone(timezone.utc)


def require_fresh_attestation(
    document: Any,
    *,
    now: datetime,
    max_age_hours: float,
) -> None:
    observed = parse_observed_at(document)
    if observed > now + timedelta(minutes=5):
        raise GateError("attestation observed_at is in the future")
    if now - observed > timedelta(hours=max_age_hours):
        raise GateError(
            f"live DepMap attestation is older than {max_age_hours:g} hours"
        )


def _git(repo: Path, *args: str) -> str:
    completed = subprocess.run(
        ["git", *args],
        cwd=repo,
        check=False,
        capture_output=True,
        text=True,
        encoding="utf-8",
    )
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip()
        raise GateError(f"git {' '.join(args)} failed: {detail}")
    return completed.stdout


def _git_file(repo: Path, revision: str, relative: str) -> str:
    return _git(repo, "show", f"{revision}:{relative}")


def _event_pr_body(path: Path) -> str | None:
    event = json.loads(path.read_text(encoding="utf-8"))
    pull_request = event.get("pull_request")
    if not isinstance(pull_request, dict):
        raise GateError("GitHub event does not contain pull_request")
    body = pull_request.get("body")
    return body if isinstance(body, str) else None


def inspect_pr(
    repo: Path,
    event: Path,
    base: str,
    head: str,
    effective: str | None = None,
) -> PrImpact:
    merge_base = _git(repo, "merge-base", base, head).strip()
    if not merge_base:
        raise GateError("git merge-base returned no revision")
    paths = [
        line.strip()
        for line in _git(repo, "diff", "--name-only", merge_base, head).splitlines()
        if line.strip()
    ]
    base_client = parse_client_range(_git_file(repo, base, CLIENT_CONTRACT_PATH))
    effective_revision = effective or head
    head_client = parse_client_range(
        _git_file(repo, effective_revision, CLIENT_CONTRACT_PATH)
    )
    base_provider = parse_provider_version(
        _git_file(repo, base, PROVIDER_CONTRACT_PATH)
    )
    head_provider = parse_provider_version(
        _git_file(repo, effective_revision, PROVIDER_CONTRACT_PATH)
    )
    live_assessment = None
    attestation_path = repo / LIVE_ATTESTATION_PATH
    if attestation_path.is_file():
        document = json.loads(attestation_path.read_text(encoding="utf-8"))
        try:
            require_fresh_attestation(
                document,
                now=datetime.now(timezone.utc),
                max_age_hours=72,
            )
            live_assessment = assess_status(document, head_client)
        except GateError:
            live_assessment = None
    return classify_pr(
        body=_event_pr_body(event),
        changed_paths=paths,
        base_client=base_client,
        head_client=head_client,
        base_provider_version=base_provider,
        head_provider_version=head_provider,
        live_assessment=live_assessment,
    )


def _jsonable(value: Any) -> Any:
    if hasattr(value, "__dataclass_fields__"):
        return _jsonable(asdict(value))
    if isinstance(value, dict):
        return {key: _jsonable(item) for key, item in value.items()}
    if isinstance(value, (list, tuple)):
        return [_jsonable(item) for item in value]
    return value


def _emit(report: Any, github_output: Path | None = None) -> None:
    payload = _jsonable(report)
    print(json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True))
    if github_output is not None:
        with github_output.open("a", encoding="utf-8") as handle:
            for key in ("classification", "merge_allowed", "merge_gate", "live_gate"):
                value = payload[key]
                handle.write(f"{key}={str(value).lower() if isinstance(value, bool) else value}\n")
    summary_path = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary_path:
        with Path(summary_path).open("a", encoding="utf-8") as handle:
            handle.write("## DepMap contract gate\n\n")
            handle.write(f"```json\n{json.dumps(payload, indent=2, sort_keys=True)}\n```\n")


def _load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as error:
        raise GateError(f"required JSON file not found: {path}") from error
    except json.JSONDecodeError as error:
        raise GateError(f"invalid JSON in {path}: {error}") from error


async def _probe_status(url: str) -> dict[str, Any]:
    try:
        from mcp import ClientSession
        from mcp.client.streamable_http import streamable_http_client
    except ImportError as error:
        raise GateError(
            "live probe requires services/depmap_mcp/requirements.txt"
        ) from error

    async with streamable_http_client(url) as streams:
        read_stream, write_stream = streams[0], streams[1]
        async with ClientSession(read_stream, write_stream) as session:
            await session.initialize()
            result = await session.call_tool("depmap_status", {})
    if getattr(result, "isError", False) or getattr(result, "is_error", False):
        raise GateError("depmap_status returned an MCP tool error")
    structured = getattr(result, "structuredContent", None)
    if structured is None:
        structured = getattr(result, "structured_content", None)
    if isinstance(structured, dict):
        return structured
    for block in getattr(result, "content", ()):
        text = getattr(block, "text", None)
        if isinstance(text, str):
            try:
                parsed = json.loads(text)
            except json.JSONDecodeError:
                continue
            if isinstance(parsed, dict):
                return parsed
    raise GateError("depmap_status returned no structured JSON payload")


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)

    pr = sub.add_parser("pr", help="classify and gate one pull request offline")
    pr.add_argument("--repo-root", type=Path, default=Path.cwd())
    pr.add_argument("--event", type=Path, required=True)
    pr.add_argument("--base", required=True)
    pr.add_argument("--head", required=True)
    pr.add_argument(
        "--effective",
        help="effective merge revision (GitHub pull_request GITHUB_SHA)",
    )
    pr.add_argument("--github-output", type=Path)

    compare = sub.add_parser("compare", help="compare status JSON with client range")
    compare.add_argument("--repo-root", type=Path, default=Path.cwd())
    compare.add_argument("--status-file", type=Path, required=True)
    compare.add_argument("--require-attestation", action="store_true")
    compare.add_argument("--max-age-hours", type=float, default=72)

    probe = sub.add_parser("probe", help="contact one MCP endpoint and attest status")
    probe.add_argument("--repo-root", type=Path, default=Path.cwd())
    probe.add_argument("--url", default="http://127.0.0.1:18877/mcp")
    probe.add_argument("--output", type=Path, required=True)
    return parser


def main(argv: list[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        repo = args.repo_root.resolve()
        client = parse_client_range(
            (repo / CLIENT_CONTRACT_PATH).read_text(encoding="utf-8")
        )
        if args.command == "pr":
            report = inspect_pr(
                repo,
                args.event.resolve(),
                args.base,
                args.head,
                args.effective,
            )
            _emit(report, args.github_output)
            return 0 if report.merge_allowed else 1
        if args.command == "compare":
            document = _load_json(args.status_file.resolve())
            if args.require_attestation:
                require_fresh_attestation(
                    document,
                    now=datetime.now(timezone.utc),
                    max_age_hours=args.max_age_hours,
                )
            assessment = assess_status(document, client)
            _emit(assessment)
            return 0 if assessment.compatible else 1
        status = asyncio.run(_probe_status(args.url))
        assessment = assess_status(status, client)
        if not assessment.compatible:
            _emit(assessment)
            return 1
        attestation = {
            "schema": ATTESTATION_SCHEMA,
            "observed_at": datetime.now(timezone.utc)
            .isoformat(timespec="seconds")
            .replace("+00:00", "Z"),
            "status": portable_attestation_status(status),
        }
        output = args.output.resolve()
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(
            json.dumps(attestation, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        _emit(assessment)
        return 0
    except (GateError, OSError, json.JSONDecodeError) as error:
        print(f"depmap contract gate: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())

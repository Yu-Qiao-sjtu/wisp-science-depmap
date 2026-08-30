"""Bounded, read-only MCP access to precomputed DepMap 26Q1 evidence.

This module deliberately does not expose filesystem reads, arbitrary SQL/R,
or analysis jobs.  Every tool maps to the validated query contract in
``services.depmap_api.app`` and returns a deterministic evidence envelope.
"""

from __future__ import annotations

import argparse
import asyncio
import hashlib
import json
import os
from collections.abc import Awaitable, Callable
from pathlib import Path
from typing import Any, Literal

from fastapi import HTTPException
from mcp.server.fastmcp import FastMCP
from mcp.types import ToolAnnotations

from services.depmap_api.app import (
    EVIDENCE_STATUSES,
    LINEAGE_NETWORK_FAMILIES,
    QueryRequest,
    Settings,
    run_bounded_query,
    resolve_lineage_term,
    verify_installation,
)


Runner = Callable[[Settings, dict[str, Any]], Awaitable[dict[str, Any]]]
Section = Literal["core", "networks", "cnv", "pathways", "drugs"]
READ_ONLY = ToolAnnotations(
    readOnlyHint=True,
    destructiveHint=False,
    idempotentHint=True,
    openWorldHint=False,
)
DEFAULT_SECTIONS: tuple[Section, ...] = (
    "core",
    "networks",
    "cnv",
    "pathways",
    "drugs",
)
GLOBAL_GENE_MODULES = (
    "effect_correlation",
    "expression_correlation",
    "expression_dependency",
    "damaging_mutation_dependency",
    "custom_missense_mutation_dependency",
    "hotspot_mutation_dependency",
    "cnv_amplification_dependency",
)


def _default_query_script() -> Path:
    return Path(__file__).resolve().parents[2] / "skills" / "depmap-knowledge-query" / "scripts" / "query_depmap_kb.R"


def settings_from_env() -> Settings:
    """Load MCP settings without requiring an HTTP API bearer token."""

    default_root = Path(r"D:\New-PHD\depmap_0823\knowledge")
    default_rscript = Path(r"C:\Program Files\R\R-4.6.1\bin\x64\Rscript.exe")
    return Settings(
        knowledge_root=Path(
            os.environ.get("DEPMAP_KNOWLEDGE_ROOT", str(default_root))
        ).expanduser().resolve(),
        query_script=Path(
            os.environ.get("DEPMAP_QUERY_SCRIPT", str(_default_query_script()))
        ).expanduser().resolve(),
        api_token=os.environ.get(
            "DEPMAP_API_TOKEN", "local-mcp-internal-read-only-token"
        ),
        release=os.environ.get("DEPMAP_RELEASE", "26Q1"),
        timeout_seconds=float(os.environ.get("DEPMAP_QUERY_TIMEOUT_SECONDS", "120")),
        max_concurrency=max(1, int(os.environ.get("DEPMAP_MAX_CONCURRENCY", "2"))),
        rscript=os.environ.get(
            "RSCRIPT", str(default_rscript if default_rscript.is_file() else "Rscript")
        ),
    )


def _metric_semantics(query: dict[str, Any]) -> dict[str, str]:
    mode = query["mode"]
    module = query.get("module") or query.get("family")
    if mode in {"pair", "top"} and module in {
        "effect_correlation",
        "expression_correlation",
        "expression_dependency",
    }:
        return {
            "metric": "correlation",
            "interpretation": "signed precomputed correlation; sign is not causal",
        }
    if mode == "lineage_network":
        return {
            "metric": "correlation",
            "interpretation": "signed within-lineage correlation; sign is not causal",
        }
    if mode in {"lineage", "lineage_cnv"} or (
        mode in {"pair", "top"}
        and module
        in {
            "damaging_mutation_dependency",
            "custom_missense_mutation_dependency",
            "hotspot_mutation_dependency",
            "cnv_amplification_dependency",
        }
    ):
        return {
            "metric": "mean_difference",
            "interpretation": "event-group minus control-group dependency; inspect returned group definitions and sign",
        }
    if mode in {"drug", "lineage_drug"}:
        return {
            "metric": "pearson_r_vs_prism_auc",
            "interpretation": "positive means higher feature values associate with higher PRISM AUC (lower sensitivity)",
        }
    if mode == "enrichment":
        return {
            "metric": "enrichment_z",
            "interpretation": "signed precomputed pathway/TF enrichment score",
        }
    if mode == "core":
        return {
            "metric": "descriptive_summary",
            "interpretation": "precomputed gene-level and lineage-level descriptive fields",
        }
    if mode == "lineage_catalog":
        return {
            "metric": "module_availability",
            "interpretation": "coverage and eligibility only; absence is not negative biological evidence",
        }
    if mode == "lineage_directions":
        return {
            "metric": "family_specific_shortlists",
            "interpretation": "fixed-filter selection over precomputed sparse rows; metrics remain separate and ranks are hypothesis-generating",
        }
    return {
        "metric": "provider_fields",
        "interpretation": "use the returned field names and provenance; no causal claim",
    }


def _canonical_json(value: Any) -> str:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))


class DepMapEvidenceService:
    """Scientific orchestration layer shared by MCP transports and tests."""

    def __init__(self, settings: Settings, runner: Runner = run_bounded_query) -> None:
        self.settings = settings
        self.runner = runner
        self.semaphore = asyncio.Semaphore(settings.max_concurrency)
        self.qa = verify_installation(settings)

    def _portable(self, value: Any) -> Any:
        root = str(self.settings.knowledge_root)
        if isinstance(value, dict):
            return {key: self._portable(item) for key, item in value.items()}
        if isinstance(value, list):
            return [self._portable(item) for item in value]
        if isinstance(value, str) and value.lower().startswith(root.lower()):
            relative = value[len(root) :].lstrip("\\/").replace("\\", "/")
            return f"depmap://{self.settings.release}/{relative}"
        return value

    def _envelope(
        self,
        *,
        tool: str,
        request: dict[str, Any],
        evidence: Any,
    ) -> dict[str, Any]:
        portable = self._portable(evidence)
        identity = {
            "schema_version": 1,
            "release": self.settings.release,
            "tool": tool,
            "request": request,
            "evidence": portable,
        }
        digest = hashlib.sha256(_canonical_json(identity).encode("utf-8")).hexdigest()
        return {
            "schema_version": 1,
            "evidence_id": f"depmap-{self.settings.release.lower()}-{digest[:24]}",
            "release": self.settings.release,
            "source": "precomputed_depmap_knowledge",
            "read_only": True,
            "new_analysis_started": False,
            "request": request,
            "evidence": portable,
        }

    async def _execute(self, query: dict[str, Any]) -> dict[str, Any]:
        validated = QueryRequest.model_validate(query).bounded_dict()
        try:
            async with self.semaphore:
                result = await self.runner(self.settings, validated)
        except HTTPException as exc:
            return {
                "query": validated,
                "metric_semantics": _metric_semantics(validated),
                "status": "QUERY_ERROR",
                "reason": str(exc.detail),
                "http_status": exc.status_code,
            }
        except Exception as exc:  # keep a bundle honest when one bounded branch fails
            return {
                "query": validated,
                "metric_semantics": _metric_semantics(validated),
                "status": "QUERY_ERROR",
                "reason": f"{type(exc).__name__}: {exc}",
            }
        return {
            "query": validated,
            "metric_semantics": _metric_semantics(validated),
            "status": result.get("status", result.get("state", "FOUND")),
            "result": result,
        }

    async def _execute_many(self, queries: list[dict[str, Any]]) -> list[dict[str, Any]]:
        return list(await asyncio.gather(*(self._execute(query) for query in queries)))

    async def status(self) -> dict[str, Any]:
        evidence = {
            "status": "ready",
            "qa_status": self.qa.get("qa_status"),
            "module_count": self.qa.get("module_count"),
            "query_contract_version": 3,
            "lineage_resolution_contract_version": 1,
            "coverage_manifest_version": 2,
            "evidence_statuses": sorted(EVIDENCE_STATUSES),
            "tool_boundary": [
                "status_and_coverage",
                "lineage_resolution",
                "cancer_level_direction_discovery",
                "gene_evidence",
                "exact_gene_pair_evidence",
                "drug_gene_evidence",
            ],
        }
        return self._envelope(tool="depmap_status", request={}, evidence=evidence)

    async def resolve_lineage(
        self,
        term: str,
        candidate_lineages: list[str] | None = None,
    ) -> dict[str, Any]:
        evidence = resolve_lineage_term(term, candidate_lineages)
        return self._envelope(
            tool="depmap_resolve_lineage",
            request={"term": term, "candidate_lineages": candidate_lineages or []},
            evidence=evidence,
        )

    async def lineage_catalog(self, lineage: str) -> dict[str, Any]:
        item = await self._execute({"mode": "lineage_catalog", "lineage": lineage})
        return self._envelope(
            tool="depmap_lineage_catalog",
            request={"lineage": lineage},
            evidence=item,
        )

    async def lineage_directions(self, lineage: str, limit: int = 20) -> dict[str, Any]:
        if not 1 <= limit <= 50:
            raise ValueError("limit must be between 1 and 50")
        item = await self._execute(
            {"mode": "lineage_directions", "lineage": lineage, "limit": limit}
        )
        return self._envelope(
            tool="depmap_lineage_direction_discovery",
            request={"lineage": lineage, "limit": limit},
            evidence=item,
        )

    async def gene_evidence(
        self,
        gene: str,
        lineage: str | None = None,
        sections: list[Section] | None = None,
        limit: int = 5,
    ) -> dict[str, Any]:
        if not 1 <= limit <= 20:
            raise ValueError("limit must be between 1 and 20")
        selected = list(dict.fromkeys(sections or DEFAULT_SECTIONS))
        unknown = sorted(set(selected) - set(DEFAULT_SECTIONS))
        if unknown:
            raise ValueError(f"unknown sections: {', '.join(unknown)}")
        symbol = gene.strip().upper()
        if not symbol:
            raise ValueError("gene must be non-empty")
        queries: list[dict[str, Any]] = []
        if "core" in selected:
            queries.append({"mode": "core", "gene": symbol})
        if lineage:
            queries.append({"mode": "lineage_catalog", "lineage": lineage})
            if "networks" in selected:
                queries.extend(
                    {
                        "mode": "lineage_network",
                        "family": family,
                        "lineage": lineage,
                        "source": symbol,
                        "limit": limit,
                    }
                    for family in sorted(LINEAGE_NETWORK_FAMILIES)
                )
            if "cnv" in selected:
                queries.append(
                    {
                        "mode": "lineage_cnv",
                        "lineage": lineage,
                        "source": symbol,
                        "limit": limit,
                    }
                )
            if "pathways" in selected:
                queries.append(
                    {
                        "mode": "enrichment",
                        "lineage": lineage,
                        "source": symbol,
                        "limit": limit,
                    }
                )
            if "drugs" in selected:
                queries.extend(
                    {
                        "mode": "lineage_drug",
                        "omic": omic,
                        "lineage": lineage,
                        "target": symbol,
                        "limit": limit,
                    }
                    for omic in ("effect", "expression", "cnv")
                )
        elif "networks" in selected or "cnv" in selected:
            for module in GLOBAL_GENE_MODULES:
                if module == "cnv_amplification_dependency" and "cnv" not in selected:
                    continue
                if module != "cnv_amplification_dependency" and "networks" not in selected:
                    continue
                queries.append(
                    {"mode": "top", "module": module, "source": symbol, "limit": limit}
                )
        items = await self._execute_many(queries)
        failures = sum(item.get("status") == "QUERY_ERROR" for item in items)
        request = {
            "gene": symbol,
            "lineage": lineage,
            "sections": selected,
            "limit": limit,
        }
        evidence = {
            "query_count": len(items),
            "query_error_count": failures,
            "complete": failures == 0,
            "items": items,
            "coverage_note": "INELIGIBLE/NOT_COMPUTED/NOT_RETAINED are coverage states, not negative biological evidence",
        }
        return self._envelope(tool="depmap_gene_evidence", request=request, evidence=evidence)

    async def pair_evidence(
        self,
        source: str,
        target: str,
        lineage: str | None = None,
    ) -> dict[str, Any]:
        source_symbol = source.strip().upper()
        target_symbol = target.strip().upper()
        if not source_symbol or not target_symbol:
            raise ValueError("source and target must be non-empty")
        queries: list[dict[str, Any]] = [
            {
                "mode": "pair",
                "module": module,
                "source": source_symbol,
                "target": target_symbol,
            }
            for module in GLOBAL_GENE_MODULES
        ]
        if lineage:
            queries.extend(
                {
                    "mode": "lineage_network",
                    "family": family,
                    "lineage": lineage,
                    "source": source_symbol,
                    "target": target_symbol,
                }
                for family in sorted(LINEAGE_NETWORK_FAMILIES)
            )
            queries.append(
                {
                    "mode": "lineage_cnv",
                    "lineage": lineage,
                    "source": source_symbol,
                    "target": target_symbol,
                }
            )
            queries.extend(
                {
                    "mode": "lineage",
                    "event": event,
                    "lineage": lineage,
                    "source": source_symbol,
                    "target": target_symbol,
                }
                for event in ("damaging", "custom_missense", "hotspot")
            )
        items = await self._execute_many(queries)
        request = {"source": source_symbol, "target": target_symbol, "lineage": lineage}
        evidence = {
            "query_count": len(items),
            "query_error_count": sum(
                item.get("status") == "QUERY_ERROR" for item in items
            ),
            "items": items,
        }
        return self._envelope(tool="depmap_pair_evidence", request=request, evidence=evidence)

    async def drug_evidence(
        self,
        drug: str,
        gene: str,
        lineage: str | None = None,
        limit: int = 10,
    ) -> dict[str, Any]:
        if not 1 <= limit <= 20:
            raise ValueError("limit must be between 1 and 20")
        drug_name = drug.strip()
        symbol = gene.strip().upper()
        if not drug_name or not symbol:
            raise ValueError("drug and gene must be non-empty")
        if lineage:
            queries = [
                {
                    "mode": "lineage_drug",
                    "omic": omic,
                    "lineage": lineage,
                    "drug": drug_name,
                    "target": symbol,
                    "limit": limit,
                }
                for omic in ("effect", "expression", "cnv")
            ]
        else:
            queries = [
                {
                    "mode": "drug",
                    "drug": drug_name,
                    "target": symbol,
                    "omic": omic,
                }
                for omic in ("effect", "expression", "cnv")
            ]
        items = await self._execute_many(queries)
        request = {
            "drug": drug_name,
            "gene": symbol,
            "lineage": lineage,
            "limit": limit,
        }
        evidence = {
            "query_count": len(items),
            "query_error_count": sum(
                item.get("status") == "QUERY_ERROR" for item in items
            ),
            "items": items,
        }
        return self._envelope(tool="depmap_drug_evidence", request=request, evidence=evidence)


def build_mcp_server(
    settings: Settings | None = None,
    runner: Runner = run_bounded_query,
    *,
    host: str = "127.0.0.1",
    port: int = 8877,
) -> FastMCP:
    service = DepMapEvidenceService(settings or settings_from_env(), runner)
    mcp = FastMCP(
        name="wisp-depmap-26q1",
        instructions=(
            "Read-only access to precomputed DepMap 26Q1 evidence. Use coverage "
            "statuses literally, preserve metric semantics, cite evidence_id, and "
            "never describe NOT_RETAINED or INELIGIBLE as negative biology."
        ),
        host=host,
        port=port,
        streamable_http_path="/mcp",
        json_response=True,
        stateless_http=True,
        max_request_body_size=64 * 1024,
    )

    @mcp.tool(
        title="DepMap knowledge status",
        description="Verify the local DepMap release, QA state, and MCP evidence boundary.",
        annotations=READ_ONLY,
        structured_output=True,
    )
    async def depmap_status() -> dict[str, Any]:
        return await service.status()

    @mcp.tool(
        title="Resolve a cancer name to DepMap lineage candidates",
        description=(
            "Resolve an extracted cancer term to a canonical DepMap model-grouping "
            "proxy. Exact maintained aliases resolve automatically. For an unknown "
            "or broad term, the language model may propose candidate_lineages; this "
            "tool validates them but requires user confirmation before evidence query."
        ),
        annotations=READ_ONLY,
        structured_output=True,
    )
    async def depmap_resolve_lineage(
        term: str,
        candidate_lineages: list[str] | None = None,
    ) -> dict[str, Any]:
        return await service.resolve_lineage(term, candidate_lineages)

    @mcp.tool(
        title="DepMap lineage coverage",
        description="List precomputed modules that are eligible and complete for one cancer lineage.",
        annotations=READ_ONLY,
        structured_output=True,
    )
    async def depmap_lineage_catalog(lineage: str) -> dict[str, Any]:
        return await service.lineage_catalog(lineage)

    @mcp.tool(
        title="DepMap cancer-level direction discovery",
        description=(
            "Select auditable family-specific candidate shortlists for a cancer lineage "
            "without inventing an anchor gene. Uses only statistically filtered rows "
            "already retained in the precomputed knowledge base."
        ),
        annotations=READ_ONLY,
        structured_output=True,
    )
    async def depmap_lineage_direction_discovery(
        lineage: str,
        limit: int = 20,
    ) -> dict[str, Any]:
        return await service.lineage_directions(lineage, limit)

    @mcp.tool(
        title="DepMap gene evidence",
        description=(
            "Primary bounded gene query. Returns core, network, CNV, pathway/TF, and "
            "PRISM evidence for a gene, optionally inside one cancer lineage."
        ),
        annotations=READ_ONLY,
        structured_output=True,
    )
    async def depmap_gene_evidence(
        gene: str,
        lineage: str | None = None,
        sections: list[Section] | None = None,
        limit: int = 5,
    ) -> dict[str, Any]:
        return await service.gene_evidence(gene, lineage, sections, limit)

    @mcp.tool(
        title="DepMap exact gene-pair evidence",
        description=(
            "Retrieve precomputed global and optional lineage-specific evidence for an "
            "exact directed source-target gene pair."
        ),
        annotations=READ_ONLY,
        structured_output=True,
    )
    async def depmap_pair_evidence(
        source: str,
        target: str,
        lineage: str | None = None,
    ) -> dict[str, Any]:
        return await service.pair_evidence(source, target, lineage)

    @mcp.tool(
        title="DepMap drug-gene evidence",
        description=(
            "Retrieve precomputed PRISM associations between one drug and one gene, "
            "globally or within a cancer lineage."
        ),
        annotations=READ_ONLY,
        structured_output=True,
    )
    async def depmap_drug_evidence(
        drug: str,
        gene: str,
        lineage: str | None = None,
        limit: int = 10,
    ) -> dict[str, Any]:
        return await service.drug_evidence(drug, gene, lineage, limit)

    return mcp


def _is_loopback(host: str) -> bool:
    return host.strip().lower() in {"127.0.0.1", "localhost", "::1"}


def main(argv: list[str] | None = None) -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--transport",
        choices=("stdio", "streamable-http"),
        default="stdio",
    )
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=8877)
    parser.add_argument(
        "--allow-remote",
        action="store_true",
        help="Allow a non-loopback bind. Use only behind authentication/TLS.",
    )
    args = parser.parse_args(argv)
    if args.transport == "streamable-http" and not _is_loopback(args.host):
        if not args.allow_remote:
            parser.error("non-loopback HTTP requires --allow-remote")
    mcp = build_mcp_server(host=args.host, port=args.port)
    mcp.run(transport=args.transport)


if __name__ == "__main__":
    main()

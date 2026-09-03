"""Bounded, read-only MCP access to precomputed DepMap and TCGA evidence.

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
Section = Literal["core", "networks", "cnv", "pathways", "drugs", "tcga"]
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
    "tcga",
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
    if mode == "tcga_expression_survival":
        return {
            "metric": "tcga_expression_and_survival_association",
            "interpretation": (
                "patient-cohort primary-tumor expression and univariate survival "
                "association; this is independent from DepMap cell-line evidence "
                "and is not causal"
            ),
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
    if mode == "lineage_dependency":
        return {
            "metric": "gene_effect_lineage_vs_rest",
            "interpretation": (
                "effect_mean_difference is lineage mean Gene Effect minus the rest "
                "mean; negative means stronger lineage dependency. It is not logFC, "
                "and selective does not imply a housekeeping/common-essential exclusion."
            ),
        }
    if mode == "subtype":
        return {
            "metric": "within_lineage_subtype_gene_effect_difference",
            "interpretation": "negative effect_size means stronger dependency in the subtype-positive group; retained significance is BH FDR within one contrast",
        }
    if mode == "coamplification":
        return {
            "metric": "coamplification_dependency_difference",
            "interpretation": "negative effect means stronger dependency in coamplified source-positive models; lineage_adjusted controls for OncoTree lineage",
        }
    if mode == "true_love":
        return {
            "metric": "stable_mutual_rank1_negative_codependency",
            "interpretation": "reciprocal rank-1 negative correlation with the frozen FDR/stability contract; association is not proof of mechanism",
        }
    if mode == "synthetic_lethal":
        return {
            "metric": "observational_event_dependency_difference",
            "interpretation": "event-group minus control-group dependency across retained mutation/CNV evidence; hypothesis-generating, not causal synthetic lethality",
        }
    if mode == "three_d":
        return {
            "metric": "family_specific_3d_screen_evidence",
            "interpretation": "precomputed 3D/2D dependency evidence; preserve the returned cohort, contrast, covariate, and family-specific metric semantics",
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
        tcga_root = self.settings.knowledge_root / "depmap-26q1-tcga"
        tcga_qa_path = tcga_root / "qa.json"
        tcga_qa: dict[str, Any] | None = None
        if tcga_qa_path.is_file():
            try:
                tcga_qa = json.loads(tcga_qa_path.read_text(encoding="utf-8-sig"))
            except (OSError, json.JSONDecodeError):
                tcga_qa = {"status": "INVALID_QA"}
        full_root = self.settings.knowledge_root / "depmap-26q1-full"
        subtype_qa_path = full_root / "subtype_dependency" / "qa.json"
        coamp_qa_path = (
            full_root / "coamplification_dependency" / "lineage_adjusted" / "qa.json"
        )
        def read_qa(path: Path) -> dict[str, Any]:
            if not path.is_file():
                return {"status": "MODULE_UNAVAILABLE"}
            try:
                return json.loads(path.read_text(encoding="utf-8-sig"))
            except (OSError, json.JSONDecodeError):
                return {"status": "INVALID_QA"}
        subtype_qa = read_qa(subtype_qa_path)
        coamp_qa = read_qa(coamp_qa_path)
        evidence = {
            "status": "ready",
            "qa_status": self.qa.get("qa_status"),
            "module_count": self.qa.get("module_count"),
            "query_contract_version": 6,
            "lineage_resolution_contract_version": 1,
            "coverage_manifest_version": 4,
            "evidence_statuses": sorted(EVIDENCE_STATUSES),
            "tool_boundary": [
                "status_and_coverage",
                "lineage_resolution",
                "cancer_level_direction_discovery",
                "cancer_level_dependency_ranking",
                "gene_evidence",
                "exact_gene_pair_evidence",
                "drug_gene_evidence",
                "molecular_subtype_evidence",
                "coamplification_dependency_evidence",
                "true_love_gene_evidence",
                "observational_synthetic_lethal_evidence",
                "three_d_dependency_evidence",
                "tcga_gene_expression_survival",
            ],
            "data_sources": {
                "depmap": {
                    "installed": True,
                    "qa_status": self.qa.get("qa_status"),
                    "scope": "cell-line perturbation and molecular association evidence",
                },
                "tcga": {
                    "installed": tcga_qa_path.is_file(),
                    "qa_status": (tcga_qa or {}).get("status", "MODULE_UNAVAILABLE"),
                    "scope": "patient primary-tumor expression and survival association evidence",
                },
            },
            "analysis_modules": {
                "subtype_dependency": {
                    "installed": subtype_qa_path.is_file(),
                    "qa_status": subtype_qa.get("status", "MODULE_UNAVAILABLE"),
                    "scope": "within-parent-lineage frozen subtype contrasts",
                },
                "coamplification_dependency": {
                    "installed": coamp_qa_path.is_file(),
                    "qa_status": coamp_qa.get("status", "MODULE_UNAVAILABLE"),
                    "scope": "constrained observed high-confidence directional pairs",
                },
                "true_love_gene": {
                    "installed": (full_root / "true_love_gene" / "manifest.json").is_file(),
                    "scope": "stable mutual rank-1 negative dependency pairs",
                },
                "observational_synthetic_lethal": {
                    "installed": (full_root / "observational_synthetic_lethal_candidates" / "manifest.json").is_file(),
                    "scope": "retained event-to-dependency candidate evidence",
                },
                "three_d": {
                    "installed": (self.settings.knowledge_root / "depmap-26q1-3d" / "catalog.csv").is_file(),
                    "scope": "3D dependency profiles, contrasts, networks, pathway and omics evidence",
                },
            },
            "integration_rule": (
                "TCGA patient evidence and DepMap cell-line evidence remain separate "
                "metrics; no sample-level join or combined score is performed"
            ),
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

    async def lineage_dependencies(
        self,
        lineage: str,
        ranking: Literal["selective", "mean_dependency"] = "selective",
        limit: int = 10,
    ) -> dict[str, Any]:
        if ranking not in {"selective", "mean_dependency"}:
            raise ValueError("ranking must be selective or mean_dependency")
        if not 1 <= limit <= 100:
            raise ValueError("limit must be between 1 and 100")
        item = await self._execute(
            {
                "mode": "lineage_dependency",
                "lineage": lineage,
                "ranking": ranking,
                "limit": limit,
            }
        )
        canonical_lineage = item.get("query", {}).get("lineage", lineage)
        return self._envelope(
            tool="depmap_lineage_dependencies",
            request={
                "lineage": canonical_lineage,
                "ranking": ranking,
                "limit": limit,
            },
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
        if "tcga" in selected:
            tcga_query: dict[str, Any] = {
                "mode": "tcga_expression_survival",
                "gene": symbol,
                "endpoint": "OS",
                "limit": limit,
            }
            if lineage:
                tcga_query["lineage"] = lineage
            queries.append(tcga_query)
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

    async def tcga_expression_survival(
        self,
        gene: str,
        project: str | None = None,
        lineage: str | None = None,
        endpoint: str = "OS",
        limit: int = 20,
    ) -> dict[str, Any]:
        symbol = gene.strip().upper()
        if not symbol:
            raise ValueError("gene must be non-empty")
        normalized_endpoint = endpoint.strip().upper()
        if normalized_endpoint not in {"OS", "DSS", "DFI", "PFI"}:
            raise ValueError("endpoint must be one of OS, DSS, DFI, or PFI")
        if project and lineage:
            raise ValueError("project and lineage are alternative cohort selectors")
        if not 1 <= limit <= 100:
            raise ValueError("limit must be between 1 and 100")
        query: dict[str, Any] = {
            "mode": "tcga_expression_survival",
            "gene": symbol,
            "endpoint": normalized_endpoint,
            "limit": limit,
        }
        if project:
            query["project"] = project.strip().upper()
        if lineage:
            query["lineage"] = lineage
        item = await self._execute(query)
        request = {
            "gene": symbol,
            "project": query.get("project"),
            "lineage": lineage,
            "endpoint": normalized_endpoint,
            "limit": limit,
        }
        evidence = {
            "patient_evidence": item,
            "integration_rule": (
                "Interpret this TCGA patient-cohort association alongside, but never "
                "as the same metric as, DepMap cell-line evidence"
            ),
        }
        return self._envelope(
            tool="tcga_gene_expression_survival",
            request=request,
            evidence=evidence,
        )

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

    async def subtype_evidence(
        self,
        gene: str | None = None,
        lineage: str | None = None,
        contrast_id: str | None = None,
        limit: int = 20,
    ) -> dict[str, Any]:
        if not 1 <= limit <= 100:
            raise ValueError("limit must be between 1 and 100")
        query: dict[str, Any] = {"mode": "subtype", "limit": limit}
        if gene:
            query["gene"] = gene.strip().upper()
        if lineage:
            query["lineage"] = lineage
        if contrast_id:
            query["contrast"] = contrast_id.strip()
        item = await self._execute(query)
        validated = item.get("query", query)
        request = {
            "gene": validated.get("gene"),
            "lineage": validated.get("lineage"),
            "contrast_id": validated.get("contrast"),
            "limit": limit,
        }
        return self._envelope(
            tool="depmap_subtype_evidence", request=request, evidence=item
        )

    async def coamplification_evidence(
        self,
        source: str,
        partner: str | None = None,
        target: str | None = None,
        layer: Literal["exhaustive_high_confidence", "lineage_adjusted"] = "lineage_adjusted",
        limit: int = 20,
    ) -> dict[str, Any]:
        if not 1 <= limit <= 100:
            raise ValueError("limit must be between 1 and 100")
        source_symbol = source.strip().upper()
        if not source_symbol:
            raise ValueError("source must be non-empty")
        query: dict[str, Any] = {
            "mode": "coamplification",
            "source": source_symbol,
            "layer": layer,
            "limit": limit,
        }
        if partner:
            query["partner"] = partner.strip().upper()
        if target:
            query["target"] = target.strip().upper()
        item = await self._execute(query)
        validated = item.get("query", query)
        request = {
            "source": validated["source"],
            "partner": validated.get("partner"),
            "target": validated.get("target"),
            "layer": validated["layer"],
            "limit": limit,
        }
        return self._envelope(
            tool="depmap_coamplification_evidence", request=request, evidence=item
        )

    async def true_love_evidence(
        self,
        gene: str | None = None,
        partner: str | None = None,
        limit: int = 20,
    ) -> dict[str, Any]:
        if not 1 <= limit <= 100:
            raise ValueError("limit must be between 1 and 100")
        query: dict[str, Any] = {"mode": "true_love", "limit": limit}
        if gene:
            query["gene"] = gene.strip().upper()
        if partner:
            query["partner"] = partner.strip().upper()
        item = await self._execute(query)
        validated = item.get("query", query)
        request = {
            "gene": validated.get("gene"),
            "partner": validated.get("partner"),
            "limit": limit,
        }
        return self._envelope(tool="depmap_true_love_evidence", request=request, evidence=item)

    async def synthetic_lethal_evidence(
        self,
        source: str | None = None,
        target: str | None = None,
        event: Literal["damaging_mutation", "custom_missense_mutation", "hotspot_mutation", "cnv_amplification"] | None = None,
        limit: int = 20,
    ) -> dict[str, Any]:
        if not source and not target:
            raise ValueError("source, target, or both are required")
        if not 1 <= limit <= 100:
            raise ValueError("limit must be between 1 and 100")
        query: dict[str, Any] = {"mode": "synthetic_lethal", "limit": limit}
        if source:
            query["source"] = source.strip().upper()
        if target:
            query["target"] = target.strip().upper()
        if event:
            query["event"] = event
        item = await self._execute(query)
        validated = item.get("query", query)
        request = {key: validated.get(key) for key in ("source", "target", "event")}
        request["limit"] = limit
        return self._envelope(tool="depmap_synthetic_lethal_evidence", request=request, evidence=item)

    async def three_d_evidence(
        self,
        family: Literal["dependency_profiles", "differential_dependency", "codependency", "true_love_gene", "omics_dependency", "lineage_dependency_enrichment"],
        gene: str | None = None,
        source: str | None = None,
        target: str | None = None,
        cohort: str | None = None,
        contrast: str | None = None,
        omic: Literal["expression", "cnv", "damaging", "hotspot"] | None = None,
        limit: int = 20,
    ) -> dict[str, Any]:
        if not 1 <= limit <= 100:
            raise ValueError("limit must be between 1 and 100")
        query: dict[str, Any] = {"mode": "three_d", "family": family, "limit": limit}
        for key, value in (("gene", gene), ("source", source), ("target", target)):
            if value:
                query[key] = value.strip().upper()
        for key, value in (("cohort", cohort), ("contrast", contrast), ("omic", omic)):
            if value:
                query[key] = value.strip()
        item = await self._execute(query)
        validated = item.get("query", query)
        request = {key: validated.get(key) for key in ("family", "gene", "source", "target", "cohort", "contrast", "omic")}
        request["limit"] = limit
        return self._envelope(tool="depmap_3d_evidence", request=request, evidence=item)

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
        name="wisp-depmap-tcga-26q1",
        instructions=(
            "Read-only access to local precomputed DepMap 26Q1 and TCGA expression/"
            "survival evidence. Keep TCGA patient-cohort metrics separate from DepMap "
            "cell-line metrics, use coverage statuses literally, preserve metric "
            "semantics, cite evidence_id, and never describe NOT_RETAINED, "
            "MODULE_UNAVAILABLE, or INELIGIBLE as negative biology."
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
        title="DepMap cancer lineage dependency ranking",
        description=(
            "Return a bounded ranking from the completed precomputed lineage-vs-rest "
            "CRISPR Gene Effect test without requiring a gene and without starting a "
            "new analysis. selective uses the precomputed one-sided Welch/BH result; "
            "mean_dependency is descriptive. Gene Effect mean difference is not logFC."
        ),
        annotations=READ_ONLY,
        structured_output=True,
    )
    async def depmap_lineage_dependencies(
        lineage: str,
        ranking: Literal["selective", "mean_dependency"] = "selective",
        limit: int = 10,
    ) -> dict[str, Any]:
        return await service.lineage_dependencies(lineage, ranking, limit)

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
            "Primary bounded gene query. Returns separate DepMap core, network, CNV, "
            "pathway/TF and PRISM evidence plus TCGA expression/OS evidence for a "
            "gene, optionally inside one cancer lineage."
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
        title="TCGA gene expression and survival evidence",
        description=(
            "Retrieve bounded precomputed primary-tumor expression and univariate "
            "OS/DSS/DFI/PFI association rows for one gene across all completed TCGA "
            "projects, one TCGA project, or projects mapped to one DepMap lineage. "
            "This patient evidence is never merged numerically with DepMap evidence."
        ),
        annotations=READ_ONLY,
        structured_output=True,
    )
    async def tcga_gene_expression_survival(
        gene: str,
        project: str | None = None,
        lineage: str | None = None,
        endpoint: str = "OS",
        limit: int = 20,
    ) -> dict[str, Any]:
        return await service.tcga_expression_survival(
            gene, project, lineage, endpoint, limit
        )

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

    @mcp.tool(
        title="DepMap molecular-subtype dependency evidence",
        description=(
            "Read the QA-complete subtype dependency module. With no gene or "
            "contrast, list eligible contrasts; with a gene, return its complete "
            "within-lineage subtype rows; with contrast_id and no gene, return only "
            "retained selective dependencies. Never infer an arbitrary subtype label."
        ),
        annotations=READ_ONLY,
        structured_output=True,
    )
    async def depmap_subtype_evidence(
        gene: str | None = None,
        lineage: str | None = None,
        contrast_id: str | None = None,
        limit: int = 20,
    ) -> dict[str, Any]:
        return await service.subtype_evidence(gene, lineage, contrast_id, limit)

    @mcp.tool(
        title="DepMap coamplification dependency evidence",
        description=(
            "Query constrained, observed source-partner double-amplification pairs "
            "and retained target dependencies from the QA-complete high-confidence "
            "or lineage-adjusted layer. A missing retained row is NOT_RETAINED, not "
            "negative biological evidence."
        ),
        annotations=READ_ONLY,
        structured_output=True,
    )
    async def depmap_coamplification_evidence(
        source: str,
        partner: str | None = None,
        target: str | None = None,
        layer: Literal["exhaustive_high_confidence", "lineage_adjusted"] = "lineage_adjusted",
        limit: int = 20,
    ) -> dict[str, Any]:
        return await service.coamplification_evidence(
            source, partner, target, layer, limit
        )

    @mcp.tool(
        title="DepMap True Love reciprocal dependency evidence",
        description=(
            "Query the completed strict mutual-rank-1 negative dependency screen, "
            "preferring its bootstrap-stable high-confidence layer. This is a "
            "codependency hypothesis and does not establish a causal mechanism."
        ),
        annotations=READ_ONLY,
        structured_output=True,
    )
    async def depmap_true_love_evidence(
        gene: str | None = None,
        partner: str | None = None,
        limit: int = 20,
    ) -> dict[str, Any]:
        return await service.true_love_evidence(gene, partner, limit)

    @mcp.tool(
        title="DepMap observational synthetic-lethal evidence",
        description=(
            "Query retained mutation/CNV event-to-target dependency candidates. "
            "The result is observational and hypothesis-generating; it must not be "
            "reported as experimentally proven synthetic lethality."
        ),
        annotations=READ_ONLY,
        structured_output=True,
    )
    async def depmap_synthetic_lethal_evidence(
        source: str | None = None,
        target: str | None = None,
        event: Literal["damaging_mutation", "custom_missense_mutation", "hotspot_mutation", "cnv_amplification"] | None = None,
        limit: int = 20,
    ) -> dict[str, Any]:
        return await service.synthetic_lethal_evidence(source, target, event, limit)

    @mcp.tool(
        title="DepMap 3D screening evidence",
        description=(
            "Query one completed 3D analysis family: dependency profiles, 3D-vs-2D "
            "contrasts, codependency, 3D True Love pairs, omics associations, or "
            "lineage/pathway enrichment. Selectors are validated against frozen catalogs."
        ),
        annotations=READ_ONLY,
        structured_output=True,
    )
    async def depmap_3d_evidence(
        family: Literal["dependency_profiles", "differential_dependency", "codependency", "true_love_gene", "omics_dependency", "lineage_dependency_enrichment"],
        gene: str | None = None,
        source: str | None = None,
        target: str | None = None,
        cohort: str | None = None,
        contrast: str | None = None,
        omic: Literal["expression", "cnv", "damaging", "hotspot"] | None = None,
        limit: int = 20,
    ) -> dict[str, Any]:
        return await service.three_d_evidence(
            family, gene, source, target, cohort, contrast, omic, limit
        )

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

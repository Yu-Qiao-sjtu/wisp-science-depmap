"""Bounded, read-only MCP access to precomputed DepMap and TCGA evidence.

This module deliberately does not expose filesystem reads, arbitrary SQL/R,
or analysis jobs.  Every tool maps to the validated query contract in
``services.depmap_api.app`` and returns a deterministic evidence envelope.
"""

from __future__ import annotations

import argparse
import asyncio
import csv
import hashlib
import json
import os
import re
import sqlite3
from collections.abc import Awaitable, Callable
from contextlib import closing
from pathlib import Path
from typing import Annotated, Any, Literal

from pydantic import Field

MAX_MODEL_EVIDENCE_BYTES = 96 * 1024
MAX_MODEL_LIST_ITEMS = 40
MAX_MODEL_STRING_CHARS = 4096


def _bounded_model_projection(value: Any) -> tuple[Any, dict[str, Any]]:
    """Bound model-facing evidence while retained artifacts remain addressable."""
    original_bytes = len(_canonical_json(value).encode("utf-8"))
    projected: Any = value
    omitted_items = 0
    truncated_strings = 0

    def project(item: Any, list_limit: int, string_limit: int) -> tuple[Any, int, int]:
        local_omitted = 0
        local_truncated = 0

        def visit(child: Any) -> Any:
            nonlocal local_omitted, local_truncated
            if isinstance(child, dict):
                return {key: visit(nested) for key, nested in child.items()}
            if isinstance(child, list):
                local_omitted += max(0, len(child) - list_limit)
                return [visit(nested) for nested in child[:list_limit]]
            if isinstance(child, str) and len(child) > string_limit:
                local_truncated += 1
                return child[:string_limit] + "…[truncated]"
            return child

        return visit(item), local_omitted, local_truncated

    # Tighten the projection in stages so scientific rows survive whenever possible.
    # Counters are recalculated from the original evidence for the final chosen pass.
    for list_limit, string_limit in (
        (MAX_MODEL_LIST_ITEMS, MAX_MODEL_STRING_CHARS),
        (20, 2048),
        (10, 1024),
        (5, 512),
        (2, 256),
        (1, 128),
    ):
        projected, omitted_items, truncated_strings = project(
            value, list_limit, string_limit
        )
        projected_bytes = len(_canonical_json(projected).encode("utf-8"))
        if projected_bytes <= MAX_MODEL_EVIDENCE_BYTES:
            break

    omitted_fields = 0
    if projected_bytes > MAX_MODEL_EVIDENCE_BYTES and isinstance(value, dict):
        # Preserve the scientific result rather than replacing it with provenance.
        # The fallback intentionally keeps a small, explicit schema and accounts for
        # every top-level field it omits.
        retained_keys = (
            "status",
            "state",
            "reason",
            "summary",
            "result",
            "rows",
            "recurrence",
            "metric_semantics",
        )
        reduced = {key: value[key] for key in retained_keys if key in value}
        omitted_fields = len(value) - len(reduced)
        projected, omitted_items, truncated_strings = project(reduced, 1, 128)
        projected["projection_notice"] = (
            "Evidence was reduced to a bounded scientific result; request a narrower "
            "query or follow a depmap:// evidence reference for more rows."
        )
        projected_bytes = len(_canonical_json(projected).encode("utf-8"))

    if projected_bytes > MAX_MODEL_EVIDENCE_BYTES:
        # A pathological mapping can still contain thousands of scalar fields. Keep
        # one compact scientific row/result and guarantee the advertised byte limit.
        compact: dict[str, Any] = {}
        if isinstance(value, dict):
            for key in ("status", "state", "reason", "summary"):
                if key in value:
                    compact[key], _, extra_truncated = project(value[key], 1, 64)
                    truncated_strings += extra_truncated
            scientific = value.get("result", value.get("rows"))
            if scientific is not None:
                compact["result"], extra_omitted, extra_truncated = project(
                    scientific, 1, 64
                )
                omitted_items += extra_omitted
                truncated_strings += extra_truncated
            omitted_fields = max(omitted_fields, len(value) - len(compact))
        compact["projection_notice"] = "Evidence exceeded the model budget; one scientific result was retained."
        projected = compact
        projected_bytes = len(_canonical_json(projected).encode("utf-8"))

    # The compact schema above is deliberately tiny; this final assertion protects
    # the API contract if it is changed later.
    if projected_bytes > MAX_MODEL_EVIDENCE_BYTES:
        projected = {
            "status": "BOUNDED",
            "projection_notice": "Evidence exceeded the model budget; submit a narrower query.",
        }
        omitted_fields = len(value) if isinstance(value, dict) else 1
        projected_bytes = len(_canonical_json(projected).encode("utf-8"))
    return projected, {
        "original_bytes": original_bytes,
        "projected_bytes": projected_bytes,
        "max_model_bytes": MAX_MODEL_EVIDENCE_BYTES,
        "max_list_items": MAX_MODEL_LIST_ITEMS,
        "omitted_items": omitted_items,
        "omitted_fields": omitted_fields,
        "truncated_strings": truncated_strings,
        "is_bounded_projection": omitted_items > 0
        or truncated_strings > 0
        or original_bytes != projected_bytes,
    }

from fastapi import HTTPException
from mcp.server.fastmcp import FastMCP
from mcp.types import ToolAnnotations

from services.depmap_api.app import (
    EVIDENCE_STATUSES,
    LINEAGE_NETWORK_FAMILIES,
    QUERY_CONTRACT_VERSION,
    QueryRequest,
    Settings,
    run_bounded_query,
    resolve_lineage_term,
    verify_installation,
)
from services.depmap_api.provider_schema import (
    limit_violation,
    tlg_scope_and_lineage,
    true_love_arg_violation,
)
from services.depmap_mcp.catalog_readers import CatalogReaderRegistry


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

# Small routing catalog loaded once per Agent session. It describes capabilities
# and never scans the scientific result matrices.
from services.depmap_mcp.capability_catalog import INTENT_CAPABILITIES



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
    scope = "lineage" if mode == "lineage_network" else "global"
    if mode == "tf_dependency":
        return {
            "metric": "pearson_correlation",
            "analysis_label": "inferred_tf_activity_to_crispr_dependency",
            "data_modality": "decoupler_ulm_tf_activity_vs_crispr_gene_effect",
            "relation_type": "predictive_association",
            "scope": "global",
            "cohort_policy": "1140_matched_expression_and_gene_effect_models",
            "interpretation": "negative means higher inferred TF activity associates with more negative Gene Effect (stronger dependency); this is observational and does not establish direct regulation or causality",
        }
    if mode == "mutation_anchor":
        return {
            "metric": "mutation_event_prevalence_and_analyzable_group_support",
            "analysis_label": "lineage_mutation_anchor_selection",
            "data_modality": "damaging_or_hotspot_mutation",
            "relation_type": "candidate_eligibility",
            "scope": "one_depmap_lineage",
            "cohort_policy": "Mut/WT thresholds recorded in the returned manifest",
            "interpretation": "candidate status means sufficient group support for downstream dependency testing; it is not a significant dependency association. Exact gene lookup returns Mut/WT counts and pass/fail criteria; do not infer a numeric bound from absence in a retained candidate list.",
        }
    if mode == "lineage_mutation_dependency":
        return {
            "metric": "mean_mutant_minus_mean_matrix_negative_gene_effect",
            "analysis_label": "lineage_official_mutation_dependency",
            "data_modality": "crispr_gene_effect",
            "relation_type": "observational_mutation_to_dependency",
            "scope": "one_depmap_lineage",
            "provider": "lineage_official_gene_effect_v2",
            "cohort_policy": "mutation-positive versus mutation-matrix-negative models in the same lineage, each group at least 5 complete cases per target",
            "interpretation": "negative delta_gene_effect means stronger dependency in mutant models. This is observational and hypothesis-generating; it is not causal synthetic lethality. Pan-cancer observational_synthetic_lethal is a separate provider.",
        }
    if mode == "biomarker_target":
        return {
            "metric": "modeling_eligibility_and_cached_validation_state",
            "analysis_label": "expression_to_dependency_predictive_biomarker_model",
            "data_modality": "baseline_expression_predicting_crispr_gene_effect",
            "relation_type": "predictive_model_eligibility",
            "scope": "pan_cancer",
            "cohort_policy": "matched_default_expression_and_gene_effect_models",
            "interpretation": "eligibility indicates sufficient coverage and Gene Effect variation; it is not evidence of predictive performance or clinical validity",
        }
    if module == "effect_correlation" and mode in {"pair", "top", "lineage_network"}:
        return {
            "metric": "correlation",
            "analysis_label": "gene_gene_codependency",
            "data_modality": "crispr_gene_effect",
            "relation_type": "codependency",
            "scope": scope,
            "cohort_policy": (
                "lineage_models_meeting_manifest_min_n"
                if scope == "lineage" else "all_available_gene_effect_models"
            ),
            "interpretation": "signed CRISPR Gene Effect profile correlation; positive supports similar dependency profiles, not causality or synthetic lethality",
        }
    if module == "expression_correlation" and mode in {"pair", "top", "lineage_network"}:
        return {
            "metric": "correlation",
            "analysis_label": "gene_gene_coexpression",
            "data_modality": "transcript_expression_log2_tpm_plus_1",
            "relation_type": "coexpression",
            "scope": scope,
            "cohort_policy": (
                "lineage_models_meeting_manifest_min_n"
                if scope == "lineage" else "all_default_expression_models"
            ),
            "interpretation": "signed gene-expression correlation; supports coexpression, not dependency, direct regulation, or causality",
        }
    if module == "expression_dependency" and mode in {"pair", "top", "lineage_network"}:
        return {
            "metric": "correlation",
            "analysis_label": "expression_dependency_association",
            "data_modality": "expression_vs_crispr_gene_effect",
            "relation_type": "predictive_association",
            "scope": scope,
            "cohort_policy": "matched_expression_and_gene_effect_models",
            "interpretation": "signed expression-to-CRISPR-dependency correlation; source is expression and target is Gene Effect, not a symmetric gene-gene relation",
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
    if mode in {"lineage_dependency", "pan_cancer_dependency"}:
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
        catalog = query.get("catalog") or "stable_negative_rank1"
        if catalog == "negative_r_lt_minus_0_3":
            return {"metric": "negative_gene_effect_correlation_below_minus_0_3", "interpretation": "negative co-dependency candidate derived from the exhaustive matrix; correlation alone is not proof of synthetic lethality"}
        if catalog == "positive_reciprocal_top20":
            return {"metric": "mutual_positive_top20_codependency", "interpretation": "both genes rank each other within their positive Gene Effect correlation Top20; this supports similar dependency profiles, not direct interaction"}
        return {"metric": "stable_mutual_rank1_negative_codependency", "interpretation": "reciprocal rank-1 negative correlation with the frozen FDR/stability contract; association is not proof of mechanism"}
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
        self.catalog_readers = CatalogReaderRegistry(
            settings.knowledge_root, settings.release
        )

    async def capabilities(self) -> dict[str, Any]:
        """Return the routing contract without touching result data."""
        capabilities = list(INTENT_CAPABILITIES)
        index = self.settings.knowledge_root / "depmap-26q1-query-index.sqlite"
        source = "code_fallback"
        if index.is_file():
            try:
                with closing(sqlite3.connect(f"file:{index.as_posix()}?mode=ro&immutable=1", uri=True)) as db:
                    rows = db.execute("SELECT payload_json FROM capability_catalog ORDER BY rowid").fetchall()
                loaded = []
                invalid_records = 0
                for row in rows:
                    try:
                        item = json.loads(row[0]) if row and row[0] else None
                    except (json.JSONDecodeError, TypeError):
                        item = None
                    if isinstance(item, dict):
                        loaded.append(item)
                    else:
                        invalid_records += 1
                if loaded:
                    capabilities, source = loaded, "sqlite_capability_catalog"
            except (sqlite3.Error, json.JSONDecodeError, OSError):
                invalid_records = 0
        else:
            invalid_records = 0
        return {
            "schema_version": 1,
            "release": self.settings.release,
            "state": "CAPABILITY_CATALOG",
            "capabilities": capabilities,
            "catalog_source": source,
            "catalog_status": "PARTIAL" if invalid_records else "FOUND",
            "invalid_record_count": invalid_records,
            "routing_policy": {
                "unknown_or_out_of_scope": "return_no_match",
                "missing_required_entity": "request_only_the_missing_field",
                "critical_direction_pairs": [
                    ["mutation_to_dependency", "dependency_to_mutation"]
                ],
                "critical_direction_ambiguity": "clarify_before_query",
                "model_confidence_is_not_a_calibrated_probability": True,
                "large_matrices_are_never_returned": True,
            },
        }

    async def artifacts(
        self, module: str | None = None, kind: str | None = None,
        path_contains: str | None = None, limit: int = 50,
    ) -> dict[str, Any]:
        index = self.settings.knowledge_root / "depmap-26q1-query-index.sqlite"
        clauses, params = ["1=1"], []
        if module:
            clauses.append("a.module=?"); params.append(module)
        if kind:
            clauses.append("f.artifact_kind=?"); params.append(kind)
        if path_contains:
            clauses.append("f.artifact_path LIKE ?"); params.append(f"%{path_contains}%")
        params.append(min(max(limit, 1), 100))
        with closing(sqlite3.connect(f"file:{index.as_posix()}?mode=ro&immutable=1", uri=True)) as db:
            db.row_factory = sqlite3.Row
            rows = [dict(row) for row in db.execute(
                f"SELECT f.artifact_path,f.artifact_kind,f.extension,f.size_bytes,f.integrity_method,f.integrity_value,a.module,a.analysis_unit,a.completion_state FROM artifact_catalog f JOIN analysis_catalog a ON a.analysis_id=f.analysis_id WHERE {' AND '.join(clauses)} ORDER BY a.module,f.artifact_path LIMIT ?", params
            )]
        return self._envelope(tool="depmap_artifact_catalog", request={"module":module,"kind":kind,"path_contains":path_contains,"limit":limit}, evidence={"status":"FOUND" if rows else "NOT_RETAINED","rows":rows})

    async def data_coverage(
        self,
        module: str | None = None,
        scope: str | None = None,
        lineage: str | None = None,
        modality: str | None = None,
        release: str | None = None,
        limit: int = 50,
    ) -> dict[str, Any]:
        index = self.settings.knowledge_root / "depmap-26q1-query-index.sqlite"
        clauses, params = ["1=1"], []
        for column, value in (
            ("module", module), ("scope", scope), ("lineage", lineage),
            ("modality", modality), ("release", release),
        ):
            if value:
                clauses.append(f"{column}=?")
                params.append(value)
        limit = min(max(int(limit), 1), 100)
        params.append(limit)
        try:
            with closing(sqlite3.connect(f"file:{index.as_posix()}?mode=ro&immutable=1", uri=True)) as db:
                db.row_factory = sqlite3.Row
                rows = [dict(row) for row in db.execute(
                    f"SELECT c.analysis_id,c.module,c.release,c.scope,c.lineage,c.modality,c.model_count,"
                    f"model_set_fingerprint,tested_gene_count,retained_gene_count,gene_universe,"
                    f"cohort_definition,intersection_policy,event_definition,mutation_policy,"
                    f"threshold_definition,source_asset_fingerprint,storage_completeness,qa_state,generated_at,"
                    f"GROUP_CONCAT(rc.query_mode) AS reader_modes "
                    f"FROM coverage_registry c LEFT JOIN reader_coverage rc ON rc.analysis_id=c.analysis_id "
                    f"WHERE {' AND '.join('c.' + clause if clause != '1=1' else clause for clause in clauses)} "
                    f"GROUP BY c.analysis_id ORDER BY c.module,c.analysis_id LIMIT ?", params
                )]
        except sqlite3.Error as exc:
            return self._envelope(
                tool="depmap_data_coverage", request={"module": module},
                evidence={"status": "MODULE_UNAVAILABLE", "reason": f"coverage registry unavailable: {exc}", "rows": []},
            )
        return self._envelope(
            tool="depmap_data_coverage",
            request={"module":module,"scope":scope,"lineage":lineage,"modality":modality,"release":release,"limit":limit},
            evidence={"status":"FOUND" if rows else "NOT_RETAINED","rows":rows,"returned_count":len(rows)},
        )

    async def read_resource(
        self, uri: str, max_rows: int = 20, cursor: int = 0
    ) -> dict[str, Any]:
        max_rows = min(max(int(max_rows), 1), 100)
        cursor = max(int(cursor), 0)
        prefix = f"depmap://{self.settings.release}/"
        if not uri.startswith(prefix):
            raise ValueError(f"uri must start with {prefix}")
        relative = uri[len(prefix):]
        index = self.settings.knowledge_root / "depmap-26q1-query-index.sqlite"
        with closing(sqlite3.connect(f"file:{index.as_posix()}?mode=ro&immutable=1", uri=True)) as db:
            hit = db.execute("SELECT artifact_kind,size_bytes FROM artifact_catalog WHERE artifact_path=?", (relative,)).fetchone()
        if not hit:
            raise ValueError("resource is absent from the indexed catalog")
        path = (self.settings.knowledge_root / relative).resolve()
        if self.settings.knowledge_root not in path.parents or not path.is_file():
            raise ValueError("resource path is unavailable")
        if path.suffix.lower() in {".rds", ".parquet", ".db", ".sqlite"}:
            return self._envelope(tool="depmap_read_resource", request={"uri":uri}, evidence={"status":"FOUND","uri":uri,"artifact_kind":hit[0],"size_bytes":hit[1],"content":"binary artifact; use its registered scientific query adapter"})
        if path.name.endswith(".csv.gz") or path.suffix.lower() in {".csv", ".tsv"}:
            try:
                if path.name.endswith(".csv.gz"):
                    import gzip
                    handle_context = gzip.open(
                        path, "rt", encoding="utf-8-sig", newline=""
                    )
                    delimiter = ","
                else:
                    handle_context = path.open(
                        encoding="utf-8-sig", newline=""
                    )
                    delimiter = "\t" if path.suffix.lower() == ".tsv" else ","
                with handle_context as handle:
                    reader = csv.DictReader(handle, delimiter=delimiter)
                    rows = []
                    total_row_count = 0
                    for index, row in enumerate(reader):
                        if cursor <= index < cursor + max_rows:
                            rows.append(row)
                        total_row_count += 1
                returned_count = len(rows)
                next_cursor = (
                    cursor + returned_count
                    if cursor + returned_count < total_row_count
                    else None
                )
                return self._envelope(
                    tool="depmap_read_resource",
                    request={"uri": uri, "max_rows": max_rows, "cursor": cursor},
                    evidence={
                        "status": "FOUND" if total_row_count else "NOT_RETAINED",
                        "uri": uri,
                        "content": rows,
                        "rows": rows,
                        "returned_count": returned_count,
                        "total_row_count": total_row_count,
                        "truncated": next_cursor is not None,
                        "next_cursor": next_cursor,
                    },
                )
            except (OSError, EOFError, UnicodeError, csv.Error) as exc:
                return self._envelope(
                    tool="depmap_read_resource",
                    request={"uri": uri, "max_rows": max_rows, "cursor": cursor},
                    evidence={
                        "status": "ERROR",
                        "uri": uri,
                        "reason": f"indexed table could not be decoded: {type(exc).__name__}",
                        "rows": [],
                        "returned_count": 0,
                        "truncated": False,
                        "next_cursor": None,
                    },
                )
        else:
            text = path.read_text(encoding="utf-8-sig", errors="replace")
            if path.suffix.lower() == ".json" and len(text.encode("utf-8")) <= 65536:
                try:
                    content = json.loads(text)
                except json.JSONDecodeError:
                    content = text[:65536]
            else:
                content = text[:65536]
        return self._envelope(tool="depmap_read_resource", request={"uri":uri,"max_rows":max_rows,"cursor":cursor}, evidence={"status":"FOUND","uri":uri,"content":content})

    def _portable_string(self, value: str) -> str:
        root_variants = {
            str(self.settings.knowledge_root).rstrip("\\/"),
            self.settings.knowledge_root.as_posix().rstrip("/"),
        }
        safe = value
        for root in sorted((item for item in root_variants if item), key=len, reverse=True):
            pattern = re.compile(
                re.escape(root)
                + r"(?=$|[\\/])(?P<tail>(?:[\\/][^\s\"'<>|,;\]\)}]*)?)",
                re.IGNORECASE,
            )

            def replace_root(match: re.Match[str]) -> str:
                relative = match.group("tail").lstrip("\\/").replace("\\", "/")
                base = f"depmap://{self.settings.release}"
                return f"{base}/{relative}" if relative else base

            safe = pattern.sub(replace_root, safe)

        stripped = safe.strip()
        exact_absolute = (
            re.fullmatch(r"[A-Za-z]:[\\/].+", stripped)
            or re.fullmatch(r"\\\\[^\\/]+[\\/][^\\/]+(?:[\\/].*)?", stripped)
            or re.fullmatch(r"//[^/]+/[^/]+(?:/.*)?", stripped)
            or re.fullmatch(r"/[^/\r\n]+(?:/[^/\r\n]+)*", stripped)
        )
        if exact_absolute and not stripped.startswith(f"depmap://{self.settings.release}/"):
            indent = value[: len(value) - len(value.lstrip())]
            return indent + "<redacted:absolute-path>"

        patterns = (
            r"(?<![A-Za-z0-9:])[A-Za-z]:[\\/][^\s\"'<>|,;\]\)}]+",
            r"(?<![A-Za-z0-9:])\\\\[^\s\\/]+[\\/][^\s\\/]+(?:[\\/][^\s\"'<>|,;\]\)}]+)*",
            r"(?<![A-Za-z0-9:])//[^\s/]+/[^\s/]+(?:/[^\s\"'<>|,;\]\)}]+)*",
            r"(?<![A-Za-z0-9:/])/[^/\s\"'<>|,;\]\)}]+(?:/[^/\s\"'<>|,;\]\)}]+)*",
        )
        for pattern in patterns:
            safe = re.sub(pattern, "<redacted:absolute-path>", safe)
        return safe

    def _portable(self, value: Any) -> Any:
        if isinstance(value, dict):
            portable: dict[Any, Any] = {}
            for key, item in value.items():
                safe_key = self._portable_string(key) if isinstance(key, str) else key
                candidate = safe_key
                collision = 1
                while candidate in portable:
                    collision += 1
                    candidate = f"{safe_key}#{collision}"
                portable[candidate] = self._portable(item)
            return portable
        if isinstance(value, list):
            return [self._portable(item) for item in value]
        if isinstance(value, str):
            return self._portable_string(value)
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
        model_evidence, projection = _bounded_model_projection(portable)
        inventory_only = tool == "depmap_analysis_catalog"
        return {
            "schema_version": 1,
            "evidence_id": f"depmap-{self.settings.release.lower()}-{digest[:24]}",
            "release": self.settings.release,
            "source": "precomputed_depmap_knowledge",
            "read_only": True,
            "new_analysis_started": False,
            "request": request,
            "evidence": model_evidence,
            "model_projection": projection,
            "presentation_contract": {
                "answer_type": "analysis_inventory" if inventory_only else "scientific_result",
                "primary_content": (
                    "completed modules, analysis units, and coverage state"
                    if inventory_only
                    else "returned biological entities, estimates, sample counts, uncertainty, adjusted significance, and direction"
                ),
                "model_must_interpret": True,
                "provenance_is_supporting_metadata": True,
                "do_not_answer_with_paths_only": True,
                "do_not_promote_catalog_status_to_biological_result": True,
                "artifact_requested": False,
                "forbidden_write_globs": ["results/reports/**"],
                "disclosure": {
                    "default": [
                        "status_sentence",
                        "bounded_top_rows",
                        "filter_truncation_flags",
                    ],
                    "expanded": [
                        "manifest",
                        "evidence_id",
                        "provenance",
                    ],
                },
                "evidence_classes": ["depmap"],
                "literature_is_separate_evidence_class": True,
            },
        }

    async def _execute(self, query: dict[str, Any]) -> dict[str, Any]:
        validated = QueryRequest.model_validate(query).bounded_dict()
        try:
            async with self.semaphore:
                resolution, result = await self.catalog_readers.read(
                    self.settings, validated, self.runner
                )
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
        if not isinstance(result, dict):
            result = {
                "status": "MODULE_UNAVAILABLE",
                "reason": "the catalog reader returned an invalid result type",
            }
        return {
            "query": validated,
            "catalog_resolution": resolution.evidence(self.settings.release),
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
            "query_contract_version": QUERY_CONTRACT_VERSION,
            "lineage_resolution_contract_version": 1,
            "coverage_manifest_version": 5,
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
                "lineage_mutation_dependency_evidence",
                "three_d_dependency_evidence",
                "tcga_gene_expression_survival",
                "tf_activity_dependency_evidence",
                "predictive_biomarker_model_eligibility",
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
                "tf_activity_dependency": {
                    "installed": (
                        self.settings.knowledge_root
                        / "analysis-modules"
                        / "转录因子活性-CRISPR基因依赖相关性分析"
                        / "results"
                        / "tf_activity_dependency_26Q1_v2"
                        / "manifest.json"
                    ).is_file(),
                    "scope": "DoRothEA A-C/decoupleR ULM TF activity versus CRISPR Gene Effect",
                },
                "predictive_biomarker": {
                    "installed": (
                        self.settings.knowledge_root / "depmap-26q1-query-index.sqlite"
                    ).is_file(),
                    "scope": "target eligibility plus validated on-demand expression-to-Gene-Effect model cache",
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
        exclude_common_essential: bool = False,
        common_essential_source: Literal["depmap_26q1"] = "depmap_26q1",
        gene: str | None = None,
    ) -> dict[str, Any]:
        if ranking not in {"selective", "mean_dependency"}:
            raise ValueError("ranking must be selective or mean_dependency")
        request = {
            "lineage": lineage,
            "ranking": ranking,
            "exclude_common_essential": exclude_common_essential,
            "common_essential_source": common_essential_source,
            "limit": limit,
        }
        if gene:
            request["gene"] = gene.strip().upper()
        rejected = limit_violation("lineage_dependency", limit)
        if rejected is not None:
            return self._envelope(
                tool="depmap_lineage_dependencies", request=request, evidence=rejected
            )
        query: dict[str, Any] = {
            "mode": "lineage_dependency",
            "lineage": lineage,
            "ranking": ranking,
            "exclude_common_essential": exclude_common_essential,
            "common_essential_source": common_essential_source,
            "limit": limit,
        }
        if gene:
            query["gene"] = gene.strip().upper()
        item = await self._execute(query)
        canonical_lineage = item.get("query", {}).get("lineage", lineage)
        request = {
            "lineage": canonical_lineage,
            "ranking": ranking,
            "exclude_common_essential": exclude_common_essential,
            "common_essential_source": common_essential_source,
            "limit": limit,
        }
        if gene:
            request["gene"] = query["gene"]
        return self._envelope(
            tool="depmap_lineage_dependencies",
            request=request,
            evidence=item,
        )

    async def lineage_directions(self, lineage: str, limit: int = 20) -> dict[str, Any]:
        request = {"lineage": lineage, "limit": limit}
        rejected = limit_violation("lineage_directions", limit)
        if rejected is not None:
            return self._envelope(
                tool="depmap_lineage_direction_discovery",
                request=request,
                evidence=rejected,
            )
        item = await self._execute(
            {"mode": "lineage_directions", "lineage": lineage, "limit": limit}
        )
        return self._envelope(
            tool="depmap_lineage_direction_discovery",
            request={"lineage": lineage, "limit": limit},
            evidence=item,
        )

    async def pan_cancer_dependencies(
        self,
        ranking: Literal["selective", "mean_dependency"] = "selective",
        limit: int = 5,
        exclude_common_essential: bool = False,
        common_essential_source: Literal["depmap_26q1"] = "depmap_26q1",
        gene: str | None = None,
    ) -> dict[str, Any]:
        if ranking not in {"selective", "mean_dependency"}:
            raise ValueError("ranking must be selective or mean_dependency")
        rejected = limit_violation("pan_cancer_dependency", limit)
        if rejected is not None:
            return self._envelope(
                tool="depmap_pan_cancer_dependencies",
                request={"ranking": ranking, "limit": limit},
                evidence=rejected,
            )
        query = {
            "mode": "pan_cancer_dependency",
            "ranking": ranking,
            "exclude_common_essential": exclude_common_essential,
            "common_essential_source": common_essential_source,
            "limit": limit,
        }
        if gene:
            query["gene"] = gene.strip().upper()
        item = await self._execute(query)
        return self._envelope(
            tool="depmap_pan_cancer_dependencies", request=query, evidence=item
        )

    async def gene_evidence(
        self,
        gene: str,
        lineage: str | None = None,
        sections: list[Section] | None = None,
        limit: int = 5,
    ) -> dict[str, Any]:
        rejected = limit_violation("enrichment", limit)
        if rejected is not None:
            return self._envelope(
                tool="depmap_gene_evidence",
                request={"gene": gene, "lineage": lineage, "limit": limit},
                evidence=rejected,
            )
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
        rejected = limit_violation("tcga_expression_survival", limit)
        if rejected is not None:
            return self._envelope(
                tool="tcga_gene_expression_survival",
                request={"gene": symbol, "limit": limit},
                evidence=rejected,
            )
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

    async def tf_dependency_evidence(
        self,
        transcription_factor: str | None = None,
        target: str | None = None,
        limit: int = 20,
        view: Literal["universe", "ranking"] | None = None,
    ) -> dict[str, Any]:
        rejected = limit_violation("tf_dependency", limit)
        if rejected is not None:
            return self._envelope(
                tool="depmap_tf_activity_dependency",
                request={"limit": limit, "view": view},
                evidence=rejected,
            )
        query: dict[str, Any] = {"mode": "tf_dependency", "limit": limit}
        if view:
            query["view"] = view
        tf = transcription_factor.strip().upper() if transcription_factor else None
        if tf:
            query["source"] = tf
        if target:
            query["target"] = target.strip().upper()
        item = await self._execute(query)
        request = {"limit": limit, "view": query.get("view")}
        if tf:
            request["transcription_factor"] = tf
        if query.get("target"):
            request["target"] = query["target"]
        return self._envelope(
            tool="depmap_tf_dependency_evidence",
            request=request,
            evidence=item,
        )

    async def biomarker_model_evidence(self, target: str) -> dict[str, Any]:
        symbol = target.strip().upper()
        if not symbol:
            raise ValueError("target must be non-empty")
        item = await self._execute({"mode": "biomarker_target", "target": symbol})
        return self._envelope(
            tool="depmap_biomarker_model_evidence",
            request={"target_gene": symbol}, evidence=item,
        )

    async def analysis_catalog(self, module: str | None = None, limit: int = 100) -> dict[str, Any]:
        query: dict[str, Any] = {
            "mode": "analysis_catalog", "completion_state": "COMPLETE", "limit": limit,
        }
        if module:
            query["module"] = module.strip()
        item = await self._execute(query)
        return self._envelope(tool="depmap_analysis_catalog", request=query, evidence=item)

    async def mutation_anchor_evidence(
        self,
        lineage: str,
        event: Literal["damaging", "hotspot"] | None = None,
        anchor_tier: Literal["priority", "strict", "standard"] = "priority",
        include_common_essential: bool = False,
        limit: int = 20,
        gene: str | None = None,
    ) -> dict[str, Any]:
        query: dict[str, Any] = {
            "mode": "mutation_anchor", "lineage": lineage,
            "anchor_tier": anchor_tier,
            "include_common_essential": include_common_essential, "limit": limit,
        }
        if event:
            query["event"] = event
        if gene:
            query["gene"] = gene.strip().upper()
        item = await self._execute(query)
        canonical = item.get("query", {}).get("lineage", lineage)
        request = dict(query)
        request["lineage"] = canonical
        return self._envelope(tool="depmap_mutation_anchor_evidence", request=request, evidence=item)

    async def subtype_evidence(
        self,
        gene: str | None = None,
        lineage: str | None = None,
        contrast_id: str | None = None,
        limit: int = 20,
    ) -> dict[str, Any]:
        rejected = limit_violation("subtype", limit)
        if rejected is not None:
            return self._envelope(
                tool="depmap_subtype_evidence",
                request={"limit": limit},
                evidence=rejected,
            )
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
        rejected = limit_violation("coamplification", limit)
        if rejected is not None:
            return self._envelope(
                tool="depmap_coamplification_evidence",
                request={"source": source, "limit": limit},
                evidence=rejected,
            )
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
        catalog: Literal["stable_negative_rank1", "negative_r_lt_minus_0_3", "positive_reciprocal_top20"] = "stable_negative_rank1",
        coverage: Literal["all", "legacy", "quality"] | None = None,
        scope: Literal["lineage", "pancancer"] | None = None,
        lineage: str | None = None,
    ) -> dict[str, Any]:
        request = {
            "gene": gene.strip().upper() if gene else None,
            "partner": partner.strip().upper() if partner else None,
            "limit": limit,
            "catalog": catalog,
            "coverage": coverage,
            "scope": scope,
            "lineage": lineage,
        }
        rejected = true_love_arg_violation(
            catalog=catalog, coverage=coverage, limit=limit
        )
        if rejected is not None:
            return self._envelope(
                tool="depmap_true_love_evidence", request=request, evidence=rejected
            )
        scoped = tlg_scope_and_lineage(scope=scope, lineage=lineage)
        if isinstance(scoped, dict):
            return self._envelope(
                tool="depmap_true_love_evidence", request=request, evidence=scoped
            )
        resolved_scope, resolved_lineage = scoped
        query: dict[str, Any] = {
            "mode": "true_love",
            "limit": limit,
            "catalog": catalog,
            "scope": resolved_scope,
        }
        if gene:
            query["gene"] = gene.strip().upper()
        if partner:
            query["partner"] = partner.strip().upper()
        if coverage:
            query["coverage"] = coverage
        if resolved_lineage:
            query["lineage"] = resolved_lineage
        item = await self._execute(query)
        validated = item.get("query", query)
        request = {
            "gene": validated.get("gene"),
            "partner": validated.get("partner"),
            "limit": limit,
            "catalog": catalog,
            "coverage": coverage,
            "scope": validated.get("scope", resolved_scope),
            "lineage": validated.get("lineage"),
            "pair_definition": item.get("pair_definition"),
        }
        return self._envelope(tool="depmap_true_love_evidence", request=request, evidence=item)

    async def synthetic_lethal_evidence(
        self,
        source: str | None = None,
        target: str | None = None,
        event: Literal["damaging_mutation", "custom_missense_mutation", "hotspot_mutation", "cnv_amplification"] | None = None,
        limit: int = 20,
        lineage: str | None = None,
    ) -> dict[str, Any]:
        if not source and not target:
            raise ValueError("source, target, or both are required")
        rejected = limit_violation("synthetic_lethal", limit)
        if rejected is not None:
            return self._envelope(
                tool="depmap_synthetic_lethal_evidence",
                request={"limit": limit, "lineage": lineage},
                evidence=rejected,
            )
        if lineage:
            query: dict[str, Any] = {
                "mode": "lineage_mutation_dependency",
                "lineage": lineage,
                "limit": limit,
            }
            if source:
                query["source"] = source.strip().upper()
            if target:
                query["target"] = target.strip().upper()
            if event:
                query["event"] = event
        else:
            query = {"mode": "synthetic_lethal", "limit": limit}
            if source:
                query["source"] = source.strip().upper()
            if target:
                query["target"] = target.strip().upper()
            if event:
                query["event"] = event
        item = await self._execute(query)
        validated = item.get("query", query)
        request = {key: validated.get(key) for key in ("source", "target", "event", "lineage")}
        request["limit"] = limit
        request["provider"] = (
            "lineage_official_gene_effect_v2" if lineage else "observational_synthetic_lethal"
        )
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
        rejected = limit_violation("three_d", limit)
        if rejected is not None:
            return self._envelope(
                tool="depmap_3d_evidence",
                request={"family": family, "limit": limit},
                evidence=rejected,
            )
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
        rejected = limit_violation("lineage_drug", limit)
        if rejected is not None:
            return self._envelope(
                tool="depmap_drug_evidence",
                request={"drug": drug, "gene": gene, "limit": limit},
                evidence=rejected,
            )
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
        title="DepMap completed analysis catalog",
        description=(
            "List completed analysis units from the unified SQLite directory index. "
            "Returns knowledge-root-relative locations and metadata only; it does not scan matrices."
        ),
        annotations=READ_ONLY,
        structured_output=True,
    )
    async def depmap_analysis_catalog(module: str | None = None, limit: int = 100) -> dict[str, Any]:
        return await service.analysis_catalog(module, limit)

    @mcp.tool(title="DepMap indexed artifact catalog", description="Query indexed result, data, script, manifest, and matrix-block artifacts by module, kind, or relative-path fragment.", annotations=READ_ONLY, structured_output=True)
    async def depmap_artifact_catalog(module: str | None = None, kind: str | None = None, path_contains: str | None = None, limit: int = 50) -> dict[str, Any]:
        return await service.artifacts(module, kind, path_contains, limit)

    @mcp.tool(title="DepMap data coverage registry", description="Query release-scoped cohort, model/gene-universe, event-definition, completeness, and QA metadata without exposing model identities or server paths.", annotations=READ_ONLY, structured_output=True)
    async def depmap_data_coverage(module: str | None = None, scope: str | None = None, lineage: str | None = None, modality: str | None = None, release: str | None = None, limit: int = 50) -> dict[str, Any]:
        return await service.data_coverage(module, scope, lineage, modality, release, limit)

    @mcp.tool(title="Read an indexed depmap resource", description="Resolve one depmap://26Q1 URI through the artifact index and return a bounded text/table preview or binary metadata. Arbitrary server paths are rejected.", annotations=READ_ONLY, structured_output=True)
    async def depmap_read_resource(
        uri: str, max_rows: int = 20, cursor: int = 0
    ) -> dict[str, Any]:
        return await service.read_resource(uri, max_rows, cursor)

    @mcp.tool(
        title="DepMap lineage mutation-anchor candidates",
        description=(
            "Return actual selectable mutation-anchor rows for one DepMap lineage, "
            "or exact eligibility for one gene. The response includes event type, "
            "Mut/WT counts, pass/fail for each threshold, and a machine-readable "
            "rejection reason. Candidate status indicates analyzable group support, "
            "not a dependency association. Do not infer mut_n from absence in a "
            "retained candidate list."
        ),
        annotations=READ_ONLY,
        structured_output=True,
    )
    async def depmap_mutation_anchor_evidence(
        lineage: str,
        event: Literal["damaging", "hotspot"] | None = None,
        anchor_tier: Literal["priority", "strict", "standard"] = "priority",
        include_common_essential: bool = False,
        limit: int = 20,
        gene: str | None = None,
    ) -> dict[str, Any]:
        return await service.mutation_anchor_evidence(
            lineage, event, anchor_tier, include_common_essential, limit, gene
        )

    @mcp.tool(
        title="DepMap analysis capability catalog",
        description=(
            "List supported scientific intents, required entities, representative "
            "Chinese utterances, confusable directions, and the matching bounded MCP "
            "tool. Use this lightweight catalog when routing is uncertain. It never "
            "scans or returns analysis matrices."
        ),
        annotations=READ_ONLY,
        structured_output=True,
    )
    async def depmap_capabilities() -> dict[str, Any]:
        return await service.capabilities()

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
            "Query the completed lineage-vs-rest CRISPR Gene Effect table. With gene, "
            "match the exact row before limit and return FOUND, NOT_RETAINED, or "
            "NOT_TESTED. Without gene, return a bounded ranking. selective uses the "
            "precomputed one-sided Welch/BH result; mean_dependency is descriptive. "
            "Gene Effect mean difference is not logFC. exclude_common_essential joins "
            "the DepMap 26Q1 common-essential sidecar once; it is not a housekeeping list."
        ),
        annotations=READ_ONLY,
        structured_output=True,
    )
    async def depmap_lineage_dependencies(
        lineage: str,
        ranking: Literal["selective", "mean_dependency"] = "selective",
        exclude_common_essential: bool = False,
        common_essential_source: Literal["depmap_26q1"] = "depmap_26q1",
        limit: int = 10,
        gene: str | None = None,
    ) -> dict[str, Any]:
        return await service.lineage_dependencies(
            lineage,
            ranking,
            limit,
            exclude_common_essential=exclude_common_essential,
            common_essential_source=common_essential_source,
            gene=gene,
        )

    @mcp.tool(
        title="DepMap pan-cancer dependency summary",
        description=(
            "Query every completed lineage dependency table in one request. With gene, "
            "return FOUND/NOT_RETAINED/NOT_TESTED per lineage from the full table. "
            "Without gene, the limit applies per lineage after ranking the full "
            "retained set. Recurrence is not inferred from truncated pages."
        ),
        annotations=READ_ONLY,
        structured_output=True,
    )
    async def depmap_pan_cancer_dependencies(
        ranking: Literal["selective", "mean_dependency"] = "selective",
        exclude_common_essential: bool = False,
        common_essential_source: Literal["depmap_26q1"] = "depmap_26q1",
        limit: int = 5,
        gene: str | None = None,
    ) -> dict[str, Any]:
        return await service.pan_cancer_dependencies(
            ranking,
            limit,
            exclude_common_essential=exclude_common_essential,
            common_essential_source=common_essential_source,
            gene=gene,
        )

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
            "exact source-target gene pair. Keeps expression coexpression, CRISPR Gene "
            "Effect co-dependency, and expression-to-dependency evidence separately "
            "labeled; use it when the user's measurement is unspecified."
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
        title="DepMap TF activity to CRISPR dependency evidence",
        description=(
            "Query the completed TF-activity module. view=universe pages the frozen TF "
            "list (tf_order) without reconstructing DoRothEA. Omit transcription_factor "
            "for bulk ranking with matched_row_count. With a TF, return exact pair or "
            "that TF's bounded ranking. FDR is BH-adjusted within each TF among pairs "
            "with at least 800 observations."
        ),
        annotations=READ_ONLY,
        structured_output=True,
    )
    async def depmap_tf_dependency_evidence(
        transcription_factor: str | None = None,
        target: str | None = None,
        limit: int = 20,
        view: Literal["universe", "ranking"] | None = None,
    ) -> dict[str, Any]:
        return await service.tf_dependency_evidence(
            transcription_factor, target, limit, view
        )

    @mcp.tool(
        title="DepMap expression biomarker model eligibility",
        description=(
            "Check one CRISPR Gene Effect target's indexed coverage/variation eligibility "
            "for expression-based nested LASSO and random-forest modeling, and report "
            "whether a validated cached model already exists. This read-only tool does "
            "not start a new model run or claim clinical validity."
        ),
        annotations=READ_ONLY,
        structured_output=True,
    )
    async def depmap_biomarker_model_evidence(target_gene: str) -> dict[str, Any]:
        return await service.biomarker_model_evidence(target_gene)

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
        title="DepMap True Love Gene (TLG) evidence",
        description=(
            "Query one completed TLG catalog: bootstrap-stable mutual-rank-1 negative "
            "pairs, the TM00 r < -0.3 negative co-dependency candidates, or positive "
            "reciprocal Top20 neighbors. Derived catalogs default to all finite pairs; "
            "pair_n and quality annotations are returned so users can filter. These "
            "are codependency hypotheses and do not prove "
            "synthetic lethality or a causal mechanism."
        ),
        annotations=READ_ONLY,
        structured_output=True,
    )
    async def depmap_true_love_evidence(
        gene: str | None = None,
        partner: str | None = None,
        limit: int = 20,
        catalog: Literal["stable_negative_rank1", "negative_r_lt_minus_0_3", "positive_reciprocal_top20"] = "stable_negative_rank1",
        coverage: Annotated[
            Literal["all", "legacy", "quality"] | None,
            Field(
                default=None,
                description=(
                    "Valid only for catalogs negative_r_lt_minus_0_3 and "
                    "positive_reciprocal_top20. Omit for stable_negative_rank1."
                ),
            ),
        ] = None,
        scope: Literal["lineage", "pancancer"] | None = None,
        lineage: str | None = None,
    ) -> dict[str, Any]:
        return await service.true_love_evidence(
            gene, partner, limit, catalog, coverage, scope, lineage
        )

    @mcp.tool(
        title="DepMap observational synthetic-lethal evidence",
        description=(
            "Query retained mutation/CNV event-to-target dependency candidates. "
            "Without lineage this is the pan-cancer observational synthetic-lethal "
            "catalog. With lineage it reads the completed lineage official Gene Effect "
            "mutation-positive versus matrix-negative analysis, never a top-N ranking "
            "as proof of absence. The result is observational and hypothesis-generating; "
            "it must not be reported as experimentally proven synthetic lethality."
        ),
        annotations=READ_ONLY,
        structured_output=True,
    )
    async def depmap_synthetic_lethal_evidence(
        source: str | None = None,
        target: str | None = None,
        event: Literal["damaging_mutation", "custom_missense_mutation", "hotspot_mutation", "cnv_amplification"] | None = None,
        limit: int = 20,
        lineage: str | None = None,
    ) -> dict[str, Any]:
        return await service.synthetic_lethal_evidence(source, target, event, limit, lineage)

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

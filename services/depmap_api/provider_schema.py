"""Provider-schema contract: advertised arguments equal runtime validators.

MCP JSON Schema and QueryRequest share these maxima and catalog-conditional
fields. Invalid combinations are scientific envelopes, not validator tracebacks.
"""

from __future__ import annotations

from typing import Any

TLG_COVERAGE_CATALOGS = frozenset(
    {"negative_r_lt_minus_0_3", "positive_reciprocal_top20"}
)

# Per-mode page maxima. MCP tools advertise the same integers.
MODE_LIMIT_MAX: dict[str, int] = {
    "analysis_catalog": 100,
    "mutation_anchor": 20,
    "lineage_mutation_dependency": 100,
    "lineage_dependency": 100,
    "pan_cancer_dependency": 20,
    "lineage_directions": 50,
    "lineage_network": 100,
    "lineage_cnv": 100,
    "lineage_drug": 20,
    "enrichment": 20,
    "subtype": 100,
    "coamplification": 100,
    "true_love": 100,
    "synthetic_lethal": 100,
    "three_d": 100,
    "tcga_expression_survival": 100,
    "tf_dependency": 100,
    "biomarker_target": 20,
    "top": 20,
}

TOOL_LIMIT_MAX: dict[str, int] = {
    "depmap_analysis_catalog": 100,
    "depmap_artifact_catalog": 100,
    "depmap_data_coverage": 100,
    "depmap_lineage_dependencies": 100,
    "depmap_pan_cancer_dependencies": 20,
    "depmap_lineage_direction_discovery": 50,
    "depmap_gene_evidence": 20,
    "depmap_mutation_anchor_evidence": 20,
    "depmap_lineage_mutation_dependency": 100,
    "depmap_true_love_evidence": 100,
    "depmap_synthetic_lethal_evidence": 100,
    "depmap_3d_evidence": 100,
    "depmap_tf_activity_dependency": 100,
    "depmap_subtype_evidence": 100,
    "depmap_coamplification_evidence": 100,
    "depmap_drug_evidence": 20,
    "tcga_gene_expression_survival": 100,
}


def tlg_coverage_is_advertised(catalog: str | None) -> bool:
    return (catalog or "stable_negative_rank1") in TLG_COVERAGE_CATALOGS


def schema_violation(
    *,
    reason: str,
    mode: str | None = None,
    **fields: Any,
) -> dict[str, Any]:
    payload: dict[str, Any] = {
        "status": "INELIGIBLE",
        "schema_error": True,
        "reason": reason,
        "rows": [],
    }
    if mode is not None:
        payload["mode"] = mode
    for key, value in fields.items():
        if value is not None:
            payload[key] = value
    return payload


def limit_violation(mode: str, limit: int) -> dict[str, Any] | None:
    maximum = MODE_LIMIT_MAX.get(mode, 100)
    if 1 <= limit <= maximum:
        return None
    return schema_violation(
        mode=mode,
        limit=limit,
        limit_max=maximum,
        reason=f"limit must be between 1 and {maximum} for mode {mode}",
    )


def true_love_arg_violation(
    *,
    catalog: str | None,
    coverage: str | None,
    limit: int,
) -> dict[str, Any] | None:
    resolved = catalog or "stable_negative_rank1"
    if coverage is not None and not tlg_coverage_is_advertised(resolved):
        return schema_violation(
            mode="true_love",
            catalog=resolved,
            coverage=coverage,
            reason="true_love coverage applies only to derived threshold or positive-reciprocal catalogs",
        )
    return limit_violation("true_love", limit)

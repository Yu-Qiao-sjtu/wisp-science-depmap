//! Shared model-facing contract for the native DepMap query tool.
//!
//! Desktop execution and deterministic evaluation both consume this exact
//! schema. Runtime readers remain host-owned, but their advertised modes and
//! tool-selection guidance must not drift behind a case-local fixture.

use serde_json::{json, Value};
use wisp_llm::ToolSchema;

pub const DEPMAP_QUERY_TOOL_NAME: &str = "depmap_query";
pub const DEPMAP_QUERY_DESCRIPTION: &str = "Query the active project's precomputed DepMap knowledge provider through a flat model-compatible schema. This tool is read-only and keeps full matrices out of context. Use mode=lineage_catalog for cancer-only availability, mode=lineage_dependency only for a cancer's dependency-gene ranking, mode=model_gene_effect for bounded canonical ModelID rows for one exact gene, mode=cross_platform_validation for precomputed Broad/Sanger/RNAi validation of one exact gene, and mode=lineage_directions for a cancer-only research-direction request without an anchor gene. Use mode=status only when provider health is actually needed. Sparse results distinguish FOUND, NOT_RETAINED, INELIGIBLE, NOT_COMPUTED, and MODULE_UNAVAILABLE. Never repeat an empty-argument or rejected mode call and never start raw-data analysis from a coverage gap.";

pub const DEPMAP_MAX_TOP_LIMIT: i64 = 100;
pub const DEPMAP_MATRIX_MODULES: &[&str] = &[
    "effect_correlation",
    "expression_correlation",
    "expression_dependency",
    "damaging_mutation_dependency",
    "custom_missense_mutation_dependency",
    "hotspot_mutation_dependency",
    "cnv_amplification_dependency",
];
pub const DEPMAP_LINEAGE_EVENTS: &[&str] = &["damaging", "custom_missense", "hotspot"];
pub const DEPMAP_DRUG_OMICS: &[&str] = &["effect", "expression", "cnv"];
pub const DEPMAP_LINEAGE_NETWORK_FAMILIES: &[&str] = &[
    "effect_correlation",
    "expression_correlation",
    "expression_dependency",
];
pub const DEPMAP_LINEAGE_DEPENDENCY_RANKINGS: &[&str] = &["selective", "mean_dependency"];

pub fn depmap_query_schema() -> Value {
    json!({
        "type":"object",
        "description":"Flat model-compatible schema. Runtime validation enforces the fields required by each mode.",
        "properties": {
            "mode": {"type":"string","enum":[
                "status","catalog","lineage_catalog","lineage_dependency","lineage_directions","model_gene_effect","cross_platform_validation","core","pair","top",
                "lineage","pathway","drug","lineage_network","lineage_cnv",
                "lineage_drug","enrichment","tcga_expression_survival"
            ]},
            "gene": {"type":"string"},
            "model_id": {"type":"string","description":"Optional exact canonical ACH-###### ModelID for model_gene_effect."},
            "gene_effect_at_or_below": {"type":"number","description":"Optional declared descriptive Chronos Gene Effect threshold for model_gene_effect; no cutoff is applied when omitted."},
            "scope": {"type":"string","enum":["global","lineage"],"description":"For cross_platform_validation: global (default) or one canonical lineage."},
            "module": {"type":"string","enum":DEPMAP_MATRIX_MODULES},
            "source": {"type":"string"},
            "target": {"type":"string"},
            "limit": {"type":"integer","minimum":1,"maximum":DEPMAP_MAX_TOP_LIMIT},
            "event": {"type":"string","enum":DEPMAP_LINEAGE_EVENTS},
            "lineage": {"type":"string"},
            "pathway": {"type":"string"},
            "drug": {"type":"string"},
            "omic": {"type":"string","enum":DEPMAP_DRUG_OMICS},
            "family": {"type":"string","enum":DEPMAP_LINEAGE_NETWORK_FAMILIES},
            "ranking": {"type":"string","enum":DEPMAP_LINEAGE_DEPENDENCY_RANKINGS,"description":"For lineage_dependency: selective (default; one-sided FDR-significant lineage-vs-rest effects ordered by precomputed rank) or mean_dependency (descriptive lowest lineage mean Gene Effect)."},
            "exclude_common_essential": {"type":"boolean","description":"For lineage_dependency: exclude genes labelled common-essential by the selected versioned source."},
            "common_essential_source": {"type":"string","enum":["depmap_26q1"]},
            "collection": {"type":"string"},
            "term": {"type":"string"},
            "reciprocal": {"type":"boolean"},
            "project": {"type":"string","description":"Optional TCGA project code, for example TCGA-BRCA or BRCA"},
            "endpoint": {"type":"string","enum":["OS","DSS","DFI","PFI"]}
        },
        "required":["mode"],
        "additionalProperties":false
    })
}

pub fn depmap_query_tool_schema() -> ToolSchema {
    ToolSchema::new(
        DEPMAP_QUERY_TOOL_NAME,
        DEPMAP_QUERY_DESCRIPTION,
        depmap_query_schema(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_schema_carries_selection_guidance_and_closed_arguments() {
        let schema = depmap_query_tool_schema();
        assert!(schema.function.description.contains("NOT_RETAINED"));
        assert_eq!(schema.function.parameters["additionalProperties"], false);
        assert_eq!(schema.function.parameters["required"], json!(["mode"]));
    }
}

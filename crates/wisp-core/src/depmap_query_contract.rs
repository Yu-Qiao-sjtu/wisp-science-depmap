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

/// Validate and normalize the flat model-facing arguments before a provider
/// sees them. This is shared by desktop execution and deterministic evals so
/// a scripted trajectory cannot accept a call that production rejects.
/// Host-specific lineage alias canonicalization is intentionally applied by
/// the desktop after this contract-level validation.
pub fn validate_depmap_query_arguments(args: &Value) -> Result<Value, String> {
    let mode = required_string(args, "mode")?;
    let mut query = serde_json::Map::new();
    query.insert("mode".into(), Value::String(mode.clone()));
    let required: &[&str] = match mode.as_str() {
        "status" | "catalog" => &[],
        "lineage_catalog" | "lineage_dependency" | "lineage_directions" => &["lineage"],
        "core" | "model_gene_effect" | "cross_platform_validation" => &["gene"],
        "pair" => &["module", "source", "target"],
        "top" => &["module", "source"],
        "lineage" => &["event", "lineage", "source", "target"],
        "pathway" => &["pathway", "target"],
        "drug" => &["omic", "drug", "target"],
        "lineage_network" => &["family", "lineage", "source"],
        "lineage_cnv" => &["lineage", "source"],
        "lineage_drug" => &["omic", "lineage"],
        "enrichment" => &["lineage", "source"],
        "tcga_expression_survival" => &["gene"],
        _ => return Err(format!("unsupported query mode '{mode}'")),
    };
    for key in required {
        query.insert((*key).into(), Value::String(required_string(args, key)?));
    }
    if matches!(mode.as_str(), "pair" | "top") {
        require_allowed(&query, "module", DEPMAP_MATRIX_MODULES)?;
    }
    if mode == "lineage" {
        require_allowed(&query, "event", DEPMAP_LINEAGE_EVENTS)?;
    }
    if mode == "drug" {
        require_allowed(&query, "omic", DEPMAP_DRUG_OMICS)?;
    }
    if mode == "lineage_network" {
        require_allowed(&query, "family", DEPMAP_LINEAGE_NETWORK_FAMILIES)?;
    }
    if mode == "lineage_dependency" {
        let ranking = args
            .get("ranking")
            .and_then(Value::as_str)
            .unwrap_or("selective")
            .trim();
        if !DEPMAP_LINEAGE_DEPENDENCY_RANKINGS.contains(&ranking) {
            return Err(format!("unsupported ranking '{ranking}'"));
        }
        query.insert("ranking".into(), Value::String(ranking.to_string()));
        query.insert(
            "exclude_common_essential".into(),
            Value::Bool(
                args.get("exclude_common_essential")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            ),
        );
        let source = args
            .get("common_essential_source")
            .and_then(Value::as_str)
            .unwrap_or("depmap_26q1")
            .trim();
        if source != "depmap_26q1" {
            return Err("common_essential_source must be depmap_26q1".into());
        }
        query.insert(
            "common_essential_source".into(),
            Value::String(source.to_string()),
        );
    }
    if mode == "model_gene_effect" {
        copy_optional_string(args, &mut query, "lineage");
        if let Some(model_id) = args
            .get("model_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let canonical = model_id.to_ascii_uppercase();
            let valid = canonical.len() == 10
                && canonical.starts_with("ACH-")
                && canonical[4..]
                    .chars()
                    .all(|character| character.is_ascii_digit());
            if !valid {
                return Err("model_id must be a canonical ACH-###### ModelID".into());
            }
            query.insert("model_id".into(), Value::String(canonical));
        }
        if let Some(threshold) = args.get("gene_effect_at_or_below").and_then(Value::as_f64) {
            if !threshold.is_finite() {
                return Err("gene_effect_at_or_below must be finite".into());
            }
            query.insert("gene_effect_at_or_below".into(), json!(threshold));
        }
    }
    if mode == "cross_platform_validation" {
        let lineage = args
            .get("lineage")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let scope = args
            .get("scope")
            .and_then(Value::as_str)
            .unwrap_or(if lineage.is_some() {
                "lineage"
            } else {
                "global"
            });
        if !["global", "lineage"].contains(&scope) {
            return Err("scope must be global or lineage".into());
        }
        if scope == "lineage" && lineage.is_none() {
            return Err("cross_platform_validation scope=lineage requires lineage".into());
        }
        if scope == "global" && lineage.is_some() {
            return Err("cross_platform_validation scope=global does not take lineage".into());
        }
        query.insert("scope".into(), Value::String(scope.into()));
        if let Some(lineage) = lineage {
            query.insert("lineage".into(), Value::String(lineage.to_string()));
        }
    }
    if mode == "lineage_drug" {
        require_allowed(&query, "omic", DEPMAP_DRUG_OMICS)?;
        if args
            .get("drug")
            .and_then(Value::as_str)
            .is_none_or(|value| value.trim().is_empty())
            && args
                .get("target")
                .and_then(Value::as_str)
                .is_none_or(|value| value.trim().is_empty())
        {
            return Err("lineage_drug requires non-empty 'drug', 'target', or both".into());
        }
    }
    for key in ["target", "drug", "collection", "term"] {
        copy_optional_string(args, &mut query, key);
    }
    if mode == "tcga_expression_survival" {
        copy_optional_string(args, &mut query, "lineage");
        if let Some(project) = args
            .get("project")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let project = project.to_ascii_uppercase();
            query.insert(
                "project".into(),
                Value::String(if project.starts_with("TCGA-") {
                    project
                } else {
                    format!("TCGA-{project}")
                }),
            );
        }
        let endpoint = args
            .get("endpoint")
            .and_then(Value::as_str)
            .unwrap_or("OS")
            .trim()
            .to_ascii_uppercase();
        if !["OS", "DSS", "DFI", "PFI"].contains(&endpoint.as_str()) {
            return Err("endpoint must be one of OS, DSS, DFI, or PFI".into());
        }
        query.insert("endpoint".into(), Value::String(endpoint));
    }
    if let Some(reciprocal) = args.get("reciprocal").and_then(Value::as_bool) {
        query.insert("reciprocal".into(), Value::Bool(reciprocal));
    }
    if matches!(
        mode.as_str(),
        "top"
            | "lineage_dependency"
            | "model_gene_effect"
            | "lineage_directions"
            | "lineage_network"
            | "lineage_cnv"
            | "lineage_drug"
            | "enrichment"
            | "tcga_expression_survival"
    ) {
        let limit = args.get("limit").and_then(Value::as_i64).unwrap_or(20);
        if !(1..=DEPMAP_MAX_TOP_LIMIT).contains(&limit) {
            return Err(format!(
                "limit must be between 1 and {DEPMAP_MAX_TOP_LIMIT}"
            ));
        }
        query.insert("limit".into(), json!(limit));
    }
    Ok(Value::Object(query))
}

fn required_string(args: &Value, key: &str) -> Result<String, String> {
    let value = args
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("mode requires non-empty '{key}'"))?;
    if value.chars().count() > 256 || value.chars().any(char::is_control) {
        return Err(format!("'{key}' must be at most 256 printable characters"));
    }
    Ok(value)
}

fn copy_optional_string(args: &Value, query: &mut serde_json::Map<String, Value>, key: &str) {
    if let Some(value) = args
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        query.insert(key.into(), Value::String(value.to_string()));
    }
}

fn require_allowed(
    query: &serde_json::Map<String, Value>,
    key: &str,
    allowed: &[&str],
) -> Result<(), String> {
    let value = query.get(key).and_then(Value::as_str).unwrap_or_default();
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(format!("unsupported {key} '{value}'"))
    }
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

    #[test]
    fn shared_validator_enforces_mode_specific_contract() {
        assert!(validate_depmap_query_arguments(&json!({"mode":"lineage_network"})).is_err());
        assert!(validate_depmap_query_arguments(&json!({
            "mode":"lineage_network",
            "family":"effect_correlation",
            "lineage":"Lung",
            "source":"KRAS"
        }))
        .is_ok());
        assert!(validate_depmap_query_arguments(&json!({
            "mode":"pair",
            "module":"unknown",
            "source":"KRAS",
            "target":"NRAS"
        }))
        .is_err());
    }
}

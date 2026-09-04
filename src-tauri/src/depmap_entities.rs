use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::OnceLock;

const REGISTRY_JSON: &str =
    include_str!("../../skills/depmap-knowledge-query/references/scientific-entity-registry.json");

#[derive(Debug, Deserialize)]
struct ScientificEntityRegistry {
    schema_version: u32,
    release: String,
    purpose: String,
    entity_types: Value,
    cancer_lineages: Vec<CancerLineage>,
    #[serde(default)]
    ambiguous_cancer_terms: Vec<AmbiguousCancerTerm>,
}

#[derive(Debug, Deserialize)]
struct CancerLineage {
    id: String,
    label: String,
    #[serde(default)]
    aliases: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct AmbiguousCancerTerm {
    term: String,
    #[serde(default)]
    aliases: Vec<String>,
    candidate_ids: Vec<String>,
}

static REGISTRY: OnceLock<Result<ScientificEntityRegistry, String>> = OnceLock::new();

fn registry() -> Result<&'static ScientificEntityRegistry, String> {
    match REGISTRY.get_or_init(|| {
        let registry: ScientificEntityRegistry = serde_json::from_str(REGISTRY_JSON)
            .map_err(|error| format!("invalid scientific entity registry: {error}"))?;
        validate_registry(&registry)?;
        Ok(registry)
    }) {
        Ok(registry) => Ok(registry),
        Err(error) => Err(error.clone()),
    }
}

fn validate_registry(registry: &ScientificEntityRegistry) -> Result<(), String> {
    if registry.schema_version != 1 {
        return Err(format!(
            "unsupported scientific entity registry schema {}",
            registry.schema_version
        ));
    }
    if registry.release.trim().is_empty()
        || registry.purpose.trim().is_empty()
        || !registry.entity_types.is_object()
    {
        return Err("scientific entity registry metadata is incomplete".into());
    }
    let mut ids = HashSet::new();
    let mut labels = HashSet::new();
    let mut aliases = HashSet::new();
    for lineage in &registry.cancer_lineages {
        if lineage.id.trim().is_empty()
            || lineage.label.trim().is_empty()
            || !ids.insert(lineage.id.as_str())
            || !labels.insert(lineage.label.as_str())
            || !aliases.insert(alias_key(&lineage.label))
            || lineage
                .aliases
                .iter()
                .any(|alias| alias.trim().is_empty() || !aliases.insert(alias_key(alias)))
        {
            return Err(
                "scientific entity registry contains duplicate or empty cancer entities".into(),
            );
        }
    }
    for term in &registry.ambiguous_cancer_terms {
        if term.term.trim().is_empty()
            || term.aliases.iter().any(|alias| alias.trim().is_empty())
            || term.candidate_ids.is_empty()
            || term
                .candidate_ids
                .iter()
                .any(|identifier| !ids.contains(identifier.as_str()))
        {
            return Err(format!("invalid ambiguous cancer term '{}'", term.term));
        }
    }
    Ok(())
}

fn alias_key(value: &str) -> String {
    value
        .chars()
        .filter(|character| {
            !character.is_whitespace()
                && !matches!(
                    character,
                    '-' | '_' | '/' | ',' | '，' | '、' | '(' | ')' | '（' | '）'
                )
        })
        .flat_map(char::to_lowercase)
        .collect()
}

pub(crate) fn canonical_lineage_label(value: &str) -> String {
    let requested = value.trim();
    let Ok(registry) = registry() else {
        return requested.to_string();
    };
    let unicode_key = alias_key(requested);
    if let Some(lineage) = registry.cancer_lineages.iter().find(|lineage| {
        alias_key(&lineage.label) == unicode_key
            || lineage
                .aliases
                .iter()
                .any(|alias| alias_key(alias) == unicode_key)
    }) {
        return lineage.label.clone();
    }

    let lower = requested.to_ascii_lowercase();
    let without_suffix = [" cancer", " carcinoma", " tumors", " tumor", " lineage"]
        .iter()
        .find_map(|suffix| lower.strip_suffix(suffix).map(str::trim))
        .filter(|candidate| !candidate.is_empty())
        .unwrap_or(requested);
    let candidate_key = alias_key(without_suffix);
    if let Some(lineage) = registry.cancer_lineages.iter().find(|lineage| {
        alias_key(&lineage.label) == candidate_key
            || lineage
                .aliases
                .iter()
                .any(|alias| alias_key(alias) == candidate_key)
    }) {
        return lineage.label.clone();
    }
    requested.to_string()
}

pub(crate) fn recognized_canonical_lineage(value: &str) -> Option<String> {
    let registry = registry().ok()?;
    let canonical = canonical_lineage_label(value);
    if registry
        .cancer_lineages
        .iter()
        .any(|lineage| lineage.label == canonical)
    {
        return Some(canonical);
    }
    let mut candidates = value
        .split(['/', '|', ',', ';', '(', ')', '[', ']'])
        .filter_map(|part| {
            let candidate = canonical_lineage_label(part.trim());
            registry
                .cancer_lineages
                .iter()
                .any(|lineage| lineage.label == candidate)
                .then_some(candidate)
        })
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.dedup();
    (candidates.len() == 1).then(|| candidates.remove(0))
}

pub(crate) fn resolved_cancer_entity(original: &str, canonical: &str) -> Value {
    let identifier = registry()
        .ok()
        .and_then(|registry| {
            registry
                .cancer_lineages
                .iter()
                .find(|lineage| lineage.label == canonical)
        })
        .map(|lineage| lineage.id.clone())
        .unwrap_or_else(|| format!("depmap-lineage:{canonical}"));
    json!({
        "schema_version":"wisp.entity-resolution.v1",
        "entity_type":"cancer",
        "original_term":original,
        "status":"RESOLVED",
        "selected":{
            "canonical_id":identifier,
            "label":canonical,
            "namespace":"depmap-lineage",
            "matched_by":"registry_label_or_alias",
            "metadata":{"relation":"model_grouping_proxy"}
        },
        "requires_user_confirmation":false,
        "is_scientific_evidence":false
    })
}

pub(crate) fn registry_summary() -> Result<Value, String> {
    let registry = registry()?;
    Ok(json!({
        "schema_version":registry.schema_version,
        "release":registry.release,
        "source":"depmap-knowledge-query/references/scientific-entity-registry.json",
        "entity_types":registry.entity_types.as_object().map(|value| value.keys().cloned().collect::<Vec<_>>()).unwrap_or_default(),
        "cancer_lineage_count":registry.cancer_lineages.len()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_valid_and_resolves_exact_aliases() {
        let summary = registry_summary().unwrap();
        assert_eq!(summary["schema_version"], 1);
        assert_eq!(summary["cancer_lineage_count"], 34);
        assert_eq!(canonical_lineage_label("肝癌"), "Liver");
        assert_eq!(
            canonical_lineage_label("ovarian carcinoma"),
            "Ovary Fallopian Tube"
        );
        assert_eq!(
            recognized_canonical_lineage("结直肠癌"),
            Some("Bowel".into())
        );
        assert_eq!(recognized_canonical_lineage("未知肿瘤"), None);
    }

    #[test]
    fn entity_resolution_is_control_data() {
        let resolution = resolved_cancer_entity("肝癌", "Liver");
        assert_eq!(
            resolution["selected"]["canonical_id"],
            "depmap-lineage:Liver"
        );
        assert_eq!(resolution["is_scientific_evidence"], false);
    }
}

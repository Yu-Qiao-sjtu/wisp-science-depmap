use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::sync::OnceLock;

const REGISTRY_JSON: &str =
    include_str!("../../skills/depmap-knowledge-query/references/agent-capability-registry.json");

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CapabilityRegistry {
    pub schema_version: u32,
    pub release: String,
    pub purpose: String,
    #[serde(default)]
    pub concepts: BTreeMap<String, Vec<ConceptDefinition>>,
    pub routes: Vec<RouteCapability>,
    pub query_capabilities: Vec<QueryCapability>,
    #[serde(default)]
    pub tool_capabilities: Vec<ToolCapability>,
    #[serde(default)]
    pub entity_resolvers: BTreeMap<String, Value>,
    pub result_states: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ConceptDefinition {
    pub id: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub precomputed_status: Option<String>,
    #[serde(default)]
    pub available_inputs: Vec<String>,
    pub proposed_analysis: Option<String>,
    pub direction_focus: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub question_tags: Vec<String>,
    #[serde(default)]
    pub entity_sets: Vec<String>,
    #[serde(default)]
    pub proxy_evidence: Vec<Value>,
    pub relation_predicate: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct RouteCondition {
    #[serde(default)]
    present: Vec<String>,
    #[serde(default)]
    absent: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RequiredAny {
    fields: Vec<String>,
    missing_label: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RouteCapability {
    pub id: String,
    pub intent: String,
    #[serde(default)]
    when: RouteCondition,
    #[serde(default)]
    required_all: Vec<String>,
    #[serde(default)]
    required_any: Vec<RequiredAny>,
    pub execution_level: String,
    #[serde(default)]
    pub requires_approval: bool,
    pub strategy: String,
    pub allowed_next_tools: Vec<String>,
    pub recommended_query: Option<Value>,
    #[serde(default)]
    pub candidate_capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct QueryCapability {
    pub id: String,
    pub mode: String,
    #[serde(default)]
    pub required_arguments: Vec<String>,
    #[serde(default)]
    pub required_any: Vec<Vec<String>>,
    pub default_limit: Option<i64>,
    pub scope: String,
    pub metric: String,
    #[serde(default)]
    pub allowed_claims: Vec<String>,
    #[serde(default)]
    pub forbidden_claims: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ToolCapability {
    pub id: String,
    pub tool: String,
    #[serde(default)]
    pub required_arguments: Vec<String>,
    pub scope: String,
    pub metric: String,
    #[serde(default)]
    pub allowed_claims: Vec<String>,
    #[serde(default)]
    pub forbidden_claims: Vec<String>,
}

static REGISTRY: OnceLock<Result<CapabilityRegistry, String>> = OnceLock::new();

pub(crate) fn registry() -> Result<&'static CapabilityRegistry, String> {
    match REGISTRY.get_or_init(|| {
        let registry: CapabilityRegistry = serde_json::from_str(REGISTRY_JSON)
            .map_err(|error| format!("invalid DepMap capability registry: {error}"))?;
        validate_registry(&registry)?;
        Ok(registry)
    }) {
        Ok(registry) => Ok(registry),
        Err(error) => Err(error.clone()),
    }
}

fn validate_registry(registry: &CapabilityRegistry) -> Result<(), String> {
    if registry.schema_version != 2 {
        return Err(format!(
            "unsupported DepMap capability registry schema {}",
            registry.schema_version
        ));
    }
    if registry.release.trim().is_empty() || registry.purpose.trim().is_empty() {
        return Err("capability registry release and purpose must be non-empty".into());
    }
    for required in [
        "phenotypes",
        "molecular_focus",
        "mechanisms",
        "evidence_sources",
        "requested_outputs",
        "execution_policies",
    ] {
        let concepts = registry
            .concepts
            .get(required)
            .ok_or_else(|| format!("missing DepMap concept category '{required}'"))?;
        let mut ids = HashSet::new();
        if concepts.is_empty()
            || concepts
                .iter()
                .any(|concept| concept.id.trim().is_empty() || !ids.insert(&concept.id))
        {
            return Err(format!(
                "concept category '{required}' must contain unique non-empty entries"
            ));
        }
    }
    let mut route_ids = HashSet::new();
    for route in &registry.routes {
        if !route_ids.insert(route.id.as_str()) {
            return Err(format!("duplicate route capability '{}'", route.id));
        }
        if route.intent.trim().is_empty()
            || route.execution_level.trim().is_empty()
            || route.strategy.trim().is_empty()
            || route.allowed_next_tools.is_empty()
        {
            return Err(format!("route capability '{}' is incomplete", route.id));
        }
    }
    let mut modes = HashSet::new();
    for capability in &registry.query_capabilities {
        if !modes.insert(capability.mode.as_str()) {
            return Err(format!("duplicate query mode '{}'", capability.mode));
        }
        if capability.id.trim().is_empty()
            || capability.scope.trim().is_empty()
            || capability.metric.trim().is_empty()
        {
            return Err(format!(
                "query capability '{}' is incomplete",
                capability.mode
            ));
        }
    }
    if !modes.contains("status") {
        return Err("query capability registry must include status".into());
    }
    let mut tools = HashSet::new();
    for capability in &registry.tool_capabilities {
        if !tools.insert(capability.tool.as_str()) {
            return Err(format!("duplicate tool capability '{}'", capability.tool));
        }
        if capability.id.trim().is_empty()
            || capability.scope.trim().is_empty()
            || capability.metric.trim().is_empty()
        {
            return Err(format!(
                "tool capability '{}' is incomplete",
                capability.tool
            ));
        }
    }
    for route in &registry.routes {
        if let Some(tool) = route
            .recommended_query
            .as_ref()
            .and_then(|query| query.get("tool"))
            .and_then(Value::as_str)
        {
            if !route
                .allowed_next_tools
                .iter()
                .any(|allowed| allowed == tool)
            {
                return Err(format!(
                    "route '{}' recommends tool '{}' outside allowed_next_tools",
                    route.id, tool
                ));
            }
        }
        for reference in &route.candidate_capabilities {
            let valid = reference
                .strip_prefix("query:")
                .is_some_and(|mode| modes.contains(mode))
                || reference
                    .strip_prefix("tool:")
                    .is_some_and(|tool| tools.contains(tool));
            if !valid {
                return Err(format!(
                    "route '{}' references unknown candidate capability '{}'",
                    route.id, reference
                ));
            }
        }
    }
    Ok(())
}

fn has_bound_value(value: &Value) -> bool {
    match value {
        Value::String(value) => !value.trim().is_empty(),
        Value::Array(values) => !values.is_empty(),
        Value::Null => false,
        _ => true,
    }
}

fn has_entity(entities: &BTreeMap<String, Value>, name: &str) -> bool {
    entities.get(name).is_some_and(has_bound_value)
}

fn condition_matches(route: &RouteCapability, entities: &BTreeMap<String, Value>) -> bool {
    route
        .when
        .present
        .iter()
        .all(|name| has_entity(entities, name))
        && route
            .when
            .absent
            .iter()
            .all(|name| !has_entity(entities, name))
}

pub(crate) fn resolve_route(
    intent: &str,
    entities: &BTreeMap<String, Value>,
) -> Result<&'static RouteCapability, String> {
    registry()?
        .routes
        .iter()
        .find(|route| route.intent == intent && condition_matches(route, entities))
        .ok_or_else(|| format!("unsupported DepMap intent '{intent}'"))
}

pub(crate) fn missing_fields(
    route: &RouteCapability,
    entities: &BTreeMap<String, Value>,
) -> Vec<String> {
    let mut missing = route
        .required_all
        .iter()
        .filter(|field| !has_entity(entities, field))
        .cloned()
        .collect::<Vec<_>>();
    for group in &route.required_any {
        if !group.fields.iter().any(|field| has_entity(entities, field)) {
            missing.push(group.missing_label.clone());
        }
    }
    missing
}

pub(crate) fn route_intents() -> Result<Vec<String>, String> {
    let mut intents = registry()?
        .routes
        .iter()
        .map(|route| route.intent.clone())
        .collect::<Vec<_>>();
    intents.sort();
    intents.dedup();
    Ok(intents)
}

pub(crate) fn concept_ids(category: &str) -> Result<Vec<String>, String> {
    registry()?
        .concepts
        .get(category)
        .map(|items| items.iter().map(|item| item.id.clone()).collect())
        .ok_or_else(|| format!("unsupported DepMap concept category '{category}'"))
}

fn normalized_concept_token(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .chars()
        .filter(|character| !matches!(character, ' ' | '-' | '_'))
        .collect()
}

pub(crate) fn canonical_concept_id(category: &str, value: &str) -> Result<Option<String>, String> {
    let concepts = registry()?
        .concepts
        .get(category)
        .ok_or_else(|| format!("unsupported DepMap concept category '{category}'"))?;
    let normalized = normalized_concept_token(value);
    Ok(concepts
        .iter()
        .find(|concept| {
            normalized_concept_token(&concept.id) == normalized
                || concept
                    .aliases
                    .iter()
                    .any(|alias| normalized_concept_token(alias) == normalized)
        })
        .map(|concept| concept.id.clone()))
}

pub(crate) fn unique_concept_category(value: &str) -> Result<Option<(String, String)>, String> {
    let mut matches = Vec::new();
    for category in ["phenotypes", "molecular_focus", "mechanisms"] {
        if let Some(id) = canonical_concept_id(category, value)? {
            matches.push((category.to_string(), id));
        }
    }
    Ok((matches.len() == 1).then(|| matches.remove(0)))
}

pub(crate) fn concept_contract(category: &str, id: &str) -> Result<Value, String> {
    let concept = registry()?
        .concepts
        .get(category)
        .and_then(|items| items.iter().find(|item| item.id == id))
        .ok_or_else(|| format!("unsupported {category} concept '{id}'"))?;
    serde_json::to_value(concept)
        .map_err(|error| format!("cannot serialize {category} concept '{id}': {error}"))
}

pub(crate) fn concept_routing_hints(category: &str, ids: &[String]) -> Result<Value, String> {
    let concepts = registry()?
        .concepts
        .get(category)
        .ok_or_else(|| format!("unsupported DepMap concept category '{category}'"))?;
    let mut question_tags = Vec::new();
    let mut entity_sets = Vec::new();
    for id in ids {
        let concept = concepts
            .iter()
            .find(|concept| &concept.id == id)
            .ok_or_else(|| format!("unsupported {category} concept '{id}'"))?;
        for tag in &concept.question_tags {
            if !question_tags.contains(tag) {
                question_tags.push(tag.clone());
            }
        }
        for entity_set in &concept.entity_sets {
            if !entity_sets.contains(entity_set) {
                entity_sets.push(entity_set.clone());
            }
        }
    }
    Ok(serde_json::json!({
        "question_tags": question_tags,
        "entity_sets": entity_sets
    }))
}

fn render_template(value: &Value, bindings: &BTreeMap<String, Value>) -> Result<Value, String> {
    if let Value::String(text) = value {
        if text.starts_with("${?") && text.ends_with('}') {
            let key = &text[3..text.len() - 1];
            return Ok(bindings
                .get(key)
                .cloned()
                .filter(|value| has_bound_value(value))
                .unwrap_or(Value::Null));
        }
    }
    match value {
        Value::String(text) if text.starts_with("${") && text.ends_with('}') => {
            let key = &text[2..text.len() - 1];
            bindings
                .get(key)
                .cloned()
                .filter(|value| has_bound_value(value))
                .ok_or_else(|| format!("recommended query requires unresolved entity '{key}'"))
        }
        Value::Array(values) => values
            .iter()
            .map(|value| render_template(value, bindings))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Value::Object(values) => {
            let mut rendered = serde_json::Map::new();
            for (key, value) in values {
                let value = render_template(value, bindings)?;
                if !value.is_null() {
                    rendered.insert(key.clone(), value);
                }
            }
            Ok(Value::Object(rendered))
        }
        _ => Ok(value.clone()),
    }
}

pub(crate) fn recommended_query(
    route: &RouteCapability,
    bindings: &BTreeMap<String, Value>,
) -> Result<Value, String> {
    route
        .recommended_query
        .as_ref()
        .map(|query| render_template(query, bindings))
        .transpose()
        .map(|query| query.unwrap_or(Value::Null))
}

pub(crate) fn entity_resolution_query(
    missing_fields: &[String],
    bindings: &BTreeMap<String, Value>,
) -> Result<Option<Value>, String> {
    for field in missing_fields {
        if let Some(template) = registry()?.entity_resolvers.get(field) {
            return render_template(template, bindings).map(Some);
        }
    }
    Ok(None)
}

pub(crate) fn query_capability(mode: &str) -> Result<&'static QueryCapability, String> {
    registry()?
        .query_capabilities
        .iter()
        .find(|capability| capability.mode == mode)
        .ok_or_else(|| format!("unsupported query mode '{mode}'"))
}

pub(crate) fn query_modes() -> Result<Vec<String>, String> {
    Ok(registry()?
        .query_capabilities
        .iter()
        .map(|capability| capability.mode.clone())
        .collect())
}

pub(crate) fn query_contract(mode: &str) -> Result<Value, String> {
    let capability = query_capability(mode)?;
    serde_json::to_value(capability)
        .map_err(|error| format!("cannot serialize query capability '{mode}': {error}"))
}

pub(crate) fn tool_contract(tool: &str) -> Result<Value, String> {
    let capability = registry()?
        .tool_capabilities
        .iter()
        .find(|capability| capability.tool == tool)
        .ok_or_else(|| format!("unsupported bounded evidence tool '{tool}'"))?;
    serde_json::to_value(capability)
        .map_err(|error| format!("cannot serialize tool capability '{tool}': {error}"))
}

pub(crate) fn candidate_contracts(route: &RouteCapability) -> Result<Value, String> {
    let mut contracts = Vec::new();
    for reference in &route.candidate_capabilities {
        let contract = if let Some(mode) = reference.strip_prefix("query:") {
            query_contract(mode)?
        } else if let Some(tool) = reference.strip_prefix("tool:") {
            tool_contract(tool)?
        } else {
            return Err(format!(
                "invalid candidate capability reference '{reference}'"
            ));
        };
        contracts.push(serde_json::json!({"reference":reference,"contract":contract}));
    }
    Ok(Value::Array(contracts))
}

pub(crate) fn registry_summary(route_id: &str) -> Result<Value, String> {
    let registry = registry()?;
    Ok(serde_json::json!({
        "schema_version": registry.schema_version,
        "release": registry.release,
        "route_id": route_id,
        "source": "depmap-knowledge-query/references/agent-capability-registry.json"
    }))
}

pub(crate) fn result_states() -> Result<Value, String> {
    serde_json::to_value(&registry()?.result_states)
        .map_err(|error| format!("cannot serialize result-state contract: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_registry_is_valid_and_covers_every_query_mode() {
        let registry = registry().unwrap();
        assert_eq!(registry.schema_version, 2);
        assert_eq!(registry.release, "26Q1");
        assert_eq!(registry.query_capabilities.len(), 17);
        assert_eq!(registry.tool_capabilities.len(), 8);
        assert!(registry.result_states.contains_key("INELIGIBLE"));
        assert!(concept_ids("phenotypes")
            .unwrap()
            .contains(&"tumor_cell_stemness".to_string()));
    }

    #[test]
    fn route_variants_and_templates_are_data_driven() {
        let mut entities = BTreeMap::from([
            ("gene".into(), Value::String("KRAS".into())),
            ("cancer".into(), Value::String("肺癌".into())),
            ("canonical_lineage".into(), Value::String("Lung".into())),
        ]);
        let lineage = resolve_route("gene_evidence", &entities).unwrap();
        assert_eq!(lineage.id, "gene_evidence_in_lineage");
        assert_eq!(
            recommended_query(lineage, &entities).unwrap()["arguments"],
            serde_json::json!({"gene":"KRAS","lineage":"Lung","limit":3})
        );

        entities.remove("cancer");
        entities.remove("canonical_lineage");
        let pan_cancer = resolve_route("gene_evidence", &entities).unwrap();
        assert_eq!(pan_cancer.id, "gene_evidence_pan_cancer");
        assert_eq!(
            recommended_query(pan_cancer, &entities).unwrap()["arguments"],
            serde_json::json!({"mode":"core","gene":"KRAS"})
        );
    }

    #[test]
    fn query_contract_carries_metric_and_claim_boundaries() {
        let contract = query_contract("lineage_network").unwrap();
        assert_eq!(contract["metric"], "family_declared_correlation");
        assert!(contract["forbidden_claims"]
            .as_array()
            .unwrap()
            .contains(&Value::String("synthetic lethality".into())));

        let route = resolve_route(
            "gene_pair_evidence",
            &BTreeMap::from([
                ("source_gene".into(), Value::String("KRAS".into())),
                ("target_gene".into(), Value::String("RAF1".into())),
            ]),
        )
        .unwrap();
        let candidates = candidate_contracts(route).unwrap();
        assert_eq!(candidates.as_array().unwrap().len(), 2);
        assert_eq!(candidates[0]["reference"], "query:pair");
    }

    #[test]
    fn optional_template_arguments_are_omitted_and_tool_contract_is_declared() {
        let route = resolve_route("subtype_evidence", &BTreeMap::new()).unwrap();
        let query = recommended_query(route, &BTreeMap::new()).unwrap();
        assert_eq!(query["arguments"], serde_json::json!({"limit":20}));
        let contract = tool_contract("depmap_subtype_evidence").unwrap();
        assert_eq!(contract["metric"], "frozen_subtype_dependency_contrast");

        let resolution = entity_resolution_query(
            &["canonical_lineage".into()],
            &BTreeMap::from([("cancer".into(), Value::String("罕见名称".into()))]),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            resolution,
            serde_json::json!({
                "tool":"depmap_resolve_entity",
                "arguments":{"entity_type":"cancer","term":"罕见名称"},
                "single_call":true
            })
        );
    }
}

//! Versioned ScientificIntent and the deterministic BridgePlanner.
//!
//! The model may propose an Intent. Host code decides whether that meaning
//! maps to one capability, needs clarification, is unsupported, or hits a
//! coverage/provider/policy/bridge boundary. Desktop, CLI, eval, delegated,
//! and resumed paths share [`plan_scientific_intent`].

use crate::specialist_manifest::{
    assemble, load_depmap_manifest, AssemblyError, HostPolicy, ResolvedSpecialistSnapshot,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use wisp_llm::ToolSchema;
use wisp_tools::{Registry, Tool, ToolEnv, ToolResult};

pub const SCIENTIFIC_INTENT_PLAN_TOOL: &str = "plan_scientific_intent";

pub const INTENT_SCHEMA_VERSION: u32 = 1;
pub const PLANNER_CONTRACT_ID: &str = "scientific_intent.bridge_planner.v1";

fn skip_empty_string(value: &Option<String>) -> bool {
    value.as_ref().is_none_or(|text| text.is_empty())
}

fn skip_empty_map<K, V>(value: &BTreeMap<K, V>) -> bool {
    value.is_empty()
}

fn skip_empty_vec<T>(value: &Vec<T>) -> bool {
    value.is_empty()
}

/// Canonical scientific request extracted from user language.
///
/// Model-authored extras such as `proposed_capability` and `proposed_coverage`
/// are recorded and rejected. They never grant authority or coverage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ScientificIntent {
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "skip_empty_vec")]
    pub entities: Vec<IntentEntity>,
    #[serde(default)]
    pub relation: String,
    #[serde(default, skip_serializing_if = "skip_empty_string")]
    pub data_modality: Option<String>,
    #[serde(default, skip_serializing_if = "skip_empty_string")]
    pub metric: Option<String>,
    #[serde(default)]
    pub scope: IntentScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<IntentDirection>,
    #[serde(default)]
    pub action: RequestedAction,
    #[serde(default, skip_serializing_if = "skip_empty_string")]
    pub release: Option<String>,
    #[serde(default, skip_serializing_if = "skip_empty_map")]
    pub constraints: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "AmbiguityMetadata::is_none")]
    pub ambiguity: AmbiguityMetadata,
    #[serde(default, skip_serializing_if = "skip_empty_string")]
    pub proposed_capability: Option<String>,
    #[serde(default, skip_serializing_if = "skip_empty_string")]
    pub proposed_coverage: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntentEntity {
    pub role: String,
    pub kind: String,
    pub identifier: String,
    #[serde(default, skip_serializing_if = "skip_empty_vec")]
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum IntentScope {
    #[default]
    Global,
    Lineage,
}

impl IntentScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Lineage => "lineage",
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        match normalize_key(raw).as_str() {
            "global" | "pancancer" | "pan_cancer" | "all_models" => Some(Self::Global),
            "lineage" | "cancer" | "tissue" => Some(Self::Lineage),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentDirection {
    Positive,
    Negative,
}

impl IntentDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Positive => "positive",
            Self::Negative => "negative",
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        match normalize_key(raw).as_str() {
            "positive" | "similar" | "pos" => Some(Self::Positive),
            "negative" | "opposite" | "neg" => Some(Self::Negative),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RequestedAction {
    #[default]
    RetrieveEvidence,
    Interpret,
    Inventory,
    Compare,
    Compute,
}

impl RequestedAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RetrieveEvidence => "retrieve_evidence",
            Self::Interpret => "interpret",
            Self::Inventory => "inventory",
            Self::Compare => "compare",
            Self::Compute => "compute",
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        match normalize_key(raw).as_str() {
            "retrieve_evidence" | "retrieve" | "query" | "lookup" | "show" | "detail"
            | "details" => Some(Self::RetrieveEvidence),
            "interpret" | "explain" => Some(Self::Interpret),
            "inventory" => Some(Self::Inventory),
            "compare" => Some(Self::Compare),
            "compute" | "run" | "recompute" | "new_analysis" => Some(Self::Compute),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AmbiguityMetadata {
    #[serde(default)]
    pub status: AmbiguityStatus,
    #[serde(default, skip_serializing_if = "skip_empty_vec")]
    pub competing_capability_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "skip_empty_string")]
    pub reason: Option<String>,
}

impl AmbiguityMetadata {
    fn is_none(&self) -> bool {
        self.status == AmbiguityStatus::None
            && self.competing_capability_ids.is_empty()
            && self.reason.as_ref().is_none_or(|text| text.is_empty())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AmbiguityStatus {
    #[default]
    None,
    CompetingInterpretations,
    MissingEntity,
}

/// Versioned capability matching table. Domain catalogs extend this document;
/// the planner never keys off example gene or lineage identifiers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntentCatalog {
    pub schema_version: u32,
    pub id: String,
    pub manifest_version: String,
    #[serde(default)]
    pub relation_aliases: BTreeMap<String, RelationAlias>,
    #[serde(default)]
    pub modality_aliases: BTreeMap<String, String>,
    #[serde(default)]
    pub metric_aliases: BTreeMap<String, String>,
    #[serde(default)]
    pub action_aliases: BTreeMap<String, String>,
    #[serde(default)]
    pub scope_aliases: BTreeMap<String, String>,
    #[serde(default)]
    pub direction_aliases: BTreeMap<String, String>,
    #[serde(default)]
    pub ambiguous_relations: Vec<AmbiguousRelation>,
    #[serde(default)]
    pub capabilities: Vec<IntentSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationAlias {
    pub relation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_modality: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AmbiguousRelation {
    pub aliases: Vec<String>,
    pub competing_capability_ids: Vec<String>,
}

/// One capability the host may execute for a canonical Intent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntentSpec {
    pub id: String,
    pub tool: String,
    pub relation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_modality: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric: Option<String>,
    #[serde(default)]
    pub action: RequestedAction,
    #[serde(default)]
    pub scopes: Vec<IntentScope>,
    #[serde(default)]
    pub entity_roles: Vec<RequiredRole>,
    #[serde(default)]
    pub require_any_roles: Vec<String>,
    #[serde(default)]
    pub arguments: Vec<ArgumentBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequiredRole {
    pub role: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArgumentBinding {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub optional: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ToolCatalog {
    tools: BTreeMap<String, DiscoveredTool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiscoveredTool {
    pub name: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannerHostPolicy {
    pub provider_available: bool,
    pub compute_authorized: bool,
    pub coverage: CoverageSignal,
    pub allowed_capability_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum CoverageSignal {
    #[default]
    NotInspected,
    Available,
    Gap {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlannerOutcome {
    pub schema_version: u32,
    pub contract: String,
    pub decision: PlannerDecision,
    pub canonical_intent: ScientificIntent,
    #[serde(default, skip_serializing_if = "skip_empty_vec")]
    pub matched_capability_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "skip_empty_vec")]
    pub rejected_model_claims: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlannerDecision {
    Execute {
        capability_id: String,
        tool: String,
        arguments: Value,
    },
    ClarificationRequired {
        competing_capability_ids: Vec<String>,
        competing_interpretations: Vec<Value>,
        reason: String,
    },
    UnsupportedIntent {
        reason: String,
    },
    CoverageGap {
        capability_id: String,
        reason: String,
    },
    BridgeUnavailable {
        capability_id: String,
        tool: String,
        reason: String,
    },
    ProviderUnavailable {
        reason: String,
    },
    PolicyBlocked {
        reason: String,
    },
}

impl PlannerDecision {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Execute { .. } => "execute",
            Self::ClarificationRequired { .. } => "clarification_required",
            Self::UnsupportedIntent { .. } => "unsupported_intent",
            Self::CoverageGap { .. } => "coverage_gap",
            Self::BridgeUnavailable { .. } => "bridge_unavailable",
            Self::ProviderUnavailable { .. } => "provider_unavailable",
            Self::PolicyBlocked { .. } => "policy_blocked",
        }
    }
}

pub struct HostScientificBridge {
    pub specialist: ResolvedSpecialistSnapshot,
    pub catalog: IntentCatalog,
}

impl ScientificIntent {
    pub fn entity(&self, role: &str) -> Option<&IntentEntity> {
        self.entities.iter().find(|entity| entity.role == role)
    }
}

impl IntentCatalog {
    pub fn from_json(json: &str) -> Result<Self, String> {
        let mut catalog: Self =
            serde_json::from_str(json).map_err(|error| format!("intent catalog: {error}"))?;
        if catalog.schema_version != INTENT_SCHEMA_VERSION {
            return Err(format!(
                "intent catalog schema {} is incompatible with runtime {INTENT_SCHEMA_VERSION}",
                catalog.schema_version
            ));
        }
        catalog.normalize_alias_tables();
        Ok(catalog)
    }

    pub fn bundled_depmap() -> Self {
        Self::from_json(include_str!(
            "../../../specialists/depmap_r_agent.intent.v1.json"
        ))
        .expect("compiled DepMap intent catalog must be valid JSON")
    }

    fn normalize_alias_tables(&mut self) {
        self.relation_aliases = take_normalized_map(&mut self.relation_aliases);
        self.modality_aliases = take_normalized_map(&mut self.modality_aliases);
        self.metric_aliases = take_normalized_map(&mut self.metric_aliases);
        self.action_aliases = take_normalized_map(&mut self.action_aliases);
        self.scope_aliases = take_normalized_map(&mut self.scope_aliases);
        self.direction_aliases = take_normalized_map(&mut self.direction_aliases);
        for entry in &mut self.ambiguous_relations {
            entry.aliases = entry
                .aliases
                .iter()
                .map(|alias| normalize_key(alias))
                .collect();
        }
    }

    pub fn canonicalize(&self, mut intent: ScientificIntent) -> ScientificIntent {
        intent.schema_version = INTENT_SCHEMA_VERSION;
        let relation_key = normalize_key(&intent.relation);
        if let Some(alias) = self.lookup_relation(&relation_key) {
            intent.relation = alias.relation.clone();
            if intent.data_modality.is_none() {
                intent.data_modality = alias.data_modality.clone();
            }
            if intent.metric.is_none() {
                intent.metric = alias.metric.clone();
            }
        } else if !intent.relation.is_empty() {
            intent.relation = relation_key;
        }
        if let Some(modality) = intent.data_modality.take() {
            let key = normalize_key(&modality);
            intent.data_modality = Some(self.modality_aliases.get(&key).cloned().unwrap_or(key));
        }
        if let Some(metric) = intent.metric.take() {
            let key = normalize_key(&metric);
            intent.metric = Some(self.metric_aliases.get(&key).cloned().unwrap_or(key));
        }
        if let Some(mapped) = self
            .action_aliases
            .get(&normalize_key(intent.action.as_str()))
            .and_then(|value| RequestedAction::parse(value))
        {
            intent.action = mapped;
        }
        if let Some(mapped) = self
            .scope_aliases
            .get(&normalize_key(intent.scope.as_str()))
            .and_then(|value| IntentScope::parse(value))
        {
            intent.scope = mapped;
        }
        if let Some(direction) = intent.direction {
            if let Some(mapped) = self
                .direction_aliases
                .get(&normalize_key(direction.as_str()))
                .and_then(|value| IntentDirection::parse(value))
            {
                intent.direction = Some(mapped);
            }
        }
        for entity in &mut intent.entities {
            entity.role = normalize_key(&entity.role);
            entity.kind = normalize_key(&entity.kind);
            let trimmed = entity.identifier.trim();
            entity.identifier = if entity.kind == "gene" {
                trimmed.to_ascii_uppercase()
            } else {
                trimmed.to_string()
            };
        }
        intent
            .entities
            .retain(|entity| !entity.identifier.is_empty());
        intent
            .entities
            .sort_by(|left, right| left.role.cmp(&right.role));
        intent.constraints.retain(|_, value| !value.is_null());
        normalize_event_constraint(&mut intent);
        if let Some(capability) = intent.proposed_capability.as_mut() {
            let trimmed = capability.trim();
            if trimmed.is_empty() {
                intent.proposed_capability = None;
            } else {
                *capability = trimmed.to_string();
            }
        }
        if let Some(coverage) = intent.proposed_coverage.as_mut() {
            let trimmed = coverage.trim();
            if trimmed.is_empty() {
                intent.proposed_coverage = None;
            } else {
                *coverage = trimmed.to_string();
            }
        }
        intent
    }

    fn lookup_relation(&self, key: &str) -> Option<&RelationAlias> {
        self.relation_aliases.get(key).or_else(|| {
            self.relation_aliases
                .values()
                .find(|alias| normalize_key(&alias.relation) == key)
        })
    }

    fn ambiguous_for(&self, relation: &str) -> Option<&AmbiguousRelation> {
        let key = normalize_key(relation);
        self.ambiguous_relations
            .iter()
            .find(|entry| entry.aliases.iter().any(|alias| alias == &key))
    }
}

fn take_normalized_map<V>(source: &mut BTreeMap<String, V>) -> BTreeMap<String, V> {
    std::mem::take(source)
        .into_iter()
        .map(|(key, value)| (normalize_key(&key), value))
        .collect()
}

impl Default for PlannerHostPolicy {
    fn default() -> Self {
        Self {
            provider_available: true,
            compute_authorized: false,
            coverage: CoverageSignal::NotInspected,
            allowed_capability_ids: None,
        }
    }
}

impl ToolCatalog {
    pub fn insert(&mut self, name: impl Into<String>, input_schema: Value) {
        let name = name.into();
        self.tools
            .insert(name.clone(), DiscoveredTool { name, input_schema });
    }

    pub fn ensure_available(&mut self, name: &str) {
        self.tools
            .entry(name.to_string())
            .or_insert_with(|| DiscoveredTool {
                name: name.to_string(),
                input_schema: json!({
                    "type": "object",
                    "additionalProperties": true
                }),
            });
    }

    pub fn get(&self, name: &str) -> Option<&DiscoveredTool> {
        self.tools.get(name)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    pub fn schema_fingerprint(&self) -> String {
        let mut names: Vec<_> = self.tools.keys().cloned().collect();
        names.sort();
        names.join("\n")
    }

    pub fn from_registry(registry: &Registry) -> Self {
        let mut catalog = Self::default();
        for name in registry.names() {
            if let Some(tool) = registry.get(name) {
                catalog.insert(name, tool.schema().function.parameters);
            }
        }
        catalog
    }

    pub fn from_registry_and_names(registry: &Registry, extra_names: &[String]) -> Self {
        let mut catalog = Self::from_registry(registry);
        for name in extra_names {
            catalog.ensure_available(name);
        }
        catalog
    }
}

/// Shared host entry point. CLI, desktop, eval, delegated, and resumed agents
/// call this function rather than forking planner logic.
pub fn plan_scientific_intent(
    proposed: ScientificIntent,
    catalog: &IntentCatalog,
    tools: &ToolCatalog,
    policy: &PlannerHostPolicy,
) -> PlannerOutcome {
    BridgePlanner {
        catalog,
        tools,
        policy,
    }
    .plan(proposed)
}

pub fn host_scientific_bridge(host: &HostPolicy) -> Result<HostScientificBridge, AssemblyError> {
    Ok(HostScientificBridge {
        specialist: assemble(&load_depmap_manifest(), host)?,
        catalog: IntentCatalog::bundled_depmap(),
    })
}

pub struct ScientificIntentPlanTool {
    catalog: IntentCatalog,
    tools: ToolCatalog,
    project_id: String,
    session_id: String,
}

/// Install the shared planner tool after the host registry (including MCP) is complete.
pub fn install_scientific_intent_planner(registry: &mut Registry) {
    install_scientific_intent_planner_in(registry, "workspace", "default");
}

pub fn install_scientific_intent_planner_in(
    registry: &mut Registry,
    project_id: impl Into<String>,
    session_id: impl Into<String>,
) {
    let tools = ToolCatalog::from_registry(registry);
    registry.add(Box::new(ScientificIntentPlanTool {
        catalog: IntentCatalog::bundled_depmap(),
        tools,
        project_id: project_id.into(),
        session_id: session_id.into(),
    }));
}

#[async_trait]
impl Tool for ScientificIntentPlanTool {
    fn name(&self) -> &str {
        SCIENTIFIC_INTENT_PLAN_TOOL
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            SCIENTIFIC_INTENT_PLAN_TOOL,
            "Convert a typed ScientificIntent into one host-validated capability. The model may propose an Intent; this tool decides execute, clarification, unsupported, or a typed boundary. Do not treat this result as scientific evidence.",
            json!({
                "type": "object",
                "additionalProperties": true
            }),
        )
    }

    fn read_only(&self) -> bool {
        true
    }

    fn preview(&self, _args: &Value) -> String {
        "plan scientific intent".into()
    }

    async fn run(&self, args: &Value, env: &dyn ToolEnv) -> ToolResult {
        let policy = PlannerHostPolicy::default();
        let digests = crate::bridge_checkpoint::host_contract_digests(
            &self.catalog,
            &self.tools,
            &policy,
            &self.project_id,
        );
        let (outcome, checkpoint) = if let Some(checkpoint_id) =
            args.get("checkpoint_id").and_then(Value::as_str)
        {
            let checkpoint = match crate::bridge_checkpoint::load_checkpoint_file(
                env.project_root(),
                checkpoint_id,
            ) {
                Ok(checkpoint) => checkpoint,
                Err(reason) => {
                    return ToolResult::fail(reason)
                        .allow_next_tools(vec!["ask_user".into(), "attempt_completion".into()])
                        .stop_batch();
                }
            };
            let action = match crate::bridge_checkpoint::resume_action_from_args(args) {
                Ok(action) => action,
                Err(reason) => {
                    return ToolResult::fail(reason)
                        .allow_next_tools(vec!["ask_user".into(), "attempt_completion".into()])
                        .stop_batch();
                }
            };
            let resume_env = crate::bridge_checkpoint::ResumeEnvironment {
                project_id: &self.project_id,
                session_id: &self.session_id,
                catalog: &self.catalog,
                tools: &self.tools,
                policy: &policy,
                digests: &digests,
            };
            match crate::bridge_checkpoint::resume_bridge_checkpoint(checkpoint, action, resume_env)
            {
                crate::bridge_checkpoint::ResumeOutcome::Resumed(checkpoint)
                | crate::bridge_checkpoint::ResumeOutcome::Idempotent { checkpoint } => {
                    let plan = PlannerOutcome {
                        schema_version: INTENT_SCHEMA_VERSION,
                        contract: PLANNER_CONTRACT_ID.into(),
                        decision: checkpoint.decision.clone(),
                        canonical_intent: checkpoint.intent.clone(),
                        matched_capability_ids: checkpoint
                            .capability_id
                            .clone()
                            .into_iter()
                            .collect(),
                        rejected_model_claims: Vec::new(),
                    };
                    (plan, Some(checkpoint))
                }
                crate::bridge_checkpoint::ResumeOutcome::ResumeContractChanged { reason }
                | crate::bridge_checkpoint::ResumeOutcome::Rejected { reason } => {
                    return ToolResult::fail(reason)
                        .allow_next_tools(vec!["ask_user".into(), "attempt_completion".into()])
                        .stop_batch();
                }
            }
        } else {
            let proposed = match proposed_intent_from_tool_args(args) {
                Ok(intent) => intent,
                Err(reason) => {
                    return ToolResult::fail(reason)
                        .allow_next_tools(vec!["ask_user".into(), "attempt_completion".into()])
                        .stop_batch();
                }
            };
            let outcome = plan_scientific_intent(proposed, &self.catalog, &self.tools, &policy);
            let operation_id = args
                .get("operation_id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            let checkpoint = crate::bridge_checkpoint::checkpoint_for_outcome(
                &outcome,
                &self.catalog,
                &self.project_id,
                &self.session_id,
                &operation_id,
                &self.catalog.manifest_version,
                digests,
            );
            (outcome, checkpoint)
        };
        if let Some(checkpoint) = &checkpoint {
            if let Err(reason) =
                crate::bridge_checkpoint::persist_checkpoint_file(env.project_root(), checkpoint)
            {
                return ToolResult::fail(reason)
                    .allow_next_tools(vec!["ask_user".into(), "attempt_completion".into()])
                    .stop_batch();
            }
        }
        let mut body = serde_json::to_value(&outcome).unwrap_or(json!({}));
        if let Some(checkpoint) = &checkpoint {
            body["checkpoint"] = serde_json::to_value(checkpoint).unwrap_or(Value::Null);
        }
        let body = serde_json::to_string_pretty(&body).unwrap_or_else(|_| "{}".into());
        match &outcome.decision {
            PlannerDecision::Execute { tool, .. } => ToolResult::ok(body).allow_next_tools(vec![
                tool.clone(),
                "ask_user".into(),
                "attempt_completion".into(),
            ]),
            _ => ToolResult::fail(body)
                .allow_next_tools(vec!["ask_user".into(), "attempt_completion".into()])
                .stop_batch(),
        }
    }
}

fn proposed_intent_from_tool_args(args: &Value) -> Result<ScientificIntent, String> {
    if let Some(intent) = args.get("intent") {
        if intent.is_object() {
            return serde_json::from_value(intent.clone())
                .map_err(|error| format!("scientific intent: {error}"));
        }
        if let Some(relation) = intent.as_str() {
            let mut proposed = ScientificIntent {
                schema_version: INTENT_SCHEMA_VERSION,
                relation: relation.into(),
                ..ScientificIntent::default()
            };
            if let Some(entities) = args.get("entities") {
                proposed.entities = serde_json::from_value(entities.clone()).unwrap_or_default();
            }
            proposed.data_modality = args
                .get("data_modality")
                .and_then(Value::as_str)
                .map(str::to_string);
            proposed.metric = args
                .get("metric")
                .and_then(Value::as_str)
                .map(str::to_string);
            if let Some(scope) = args.get("scope").and_then(Value::as_str) {
                proposed.scope = IntentScope::parse(scope).unwrap_or_default();
            }
            proposed.action = args
                .get("action")
                .and_then(Value::as_str)
                .and_then(RequestedAction::parse)
                .unwrap_or_default();
            if let Some(constraints) = args.get("constraints").and_then(Value::as_object) {
                proposed.constraints = constraints
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect();
            }
            return Ok(proposed);
        }
    }
    serde_json::from_value(args.clone()).map_err(|error| format!("scientific intent: {error}"))
}

fn normalize_event_constraint(intent: &mut ScientificIntent) {
    let Some(raw) = intent
        .constraints
        .get("event")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return;
    };
    let mapped = match (intent.scope, raw.as_str()) {
        (IntentScope::Global, "damaging") => "damaging_mutation",
        (IntentScope::Global, "hotspot") => "hotspot_mutation",
        (IntentScope::Global, "custom_missense") => "custom_missense_mutation",
        _ => raw.as_str(),
    };
    intent.constraints.insert("event".into(), json!(mapped));
}

struct BridgePlanner<'a> {
    catalog: &'a IntentCatalog,
    tools: &'a ToolCatalog,
    policy: &'a PlannerHostPolicy,
}

impl BridgePlanner<'_> {
    fn plan(&self, proposed: ScientificIntent) -> PlannerOutcome {
        let mut rejected_model_claims = Vec::new();
        if proposed
            .proposed_capability
            .as_ref()
            .is_some_and(|value| !value.is_empty())
        {
            rejected_model_claims.push("proposed_capability".into());
        }
        if proposed
            .proposed_coverage
            .as_ref()
            .is_some_and(|value| !value.is_empty())
        {
            rejected_model_claims.push("proposed_coverage".into());
        }
        if proposed.schema_version != 0 && proposed.schema_version != INTENT_SCHEMA_VERSION {
            return outcome(
                PlannerDecision::UnsupportedIntent {
                    reason: format!(
                        "intent schema {} is incompatible with runtime {INTENT_SCHEMA_VERSION}",
                        proposed.schema_version
                    ),
                },
                proposed,
                Vec::new(),
                rejected_model_claims,
            );
        }

        let canonical = self.catalog.canonicalize(proposed);
        if let Some(ambiguous) = self.catalog.ambiguous_for(&canonical.relation) {
            return outcome(
                clarification(
                    ambiguous.competing_capability_ids.clone(),
                    self.interpretations(&ambiguous.competing_capability_ids),
                    "competing scientific interpretations require clarification",
                ),
                canonical,
                ambiguous.competing_capability_ids.clone(),
                rejected_model_claims,
            );
        }
        if canonical.ambiguity.status == AmbiguityStatus::CompetingInterpretations
            && canonical.ambiguity.competing_capability_ids.len() > 1
        {
            let competing = canonical.ambiguity.competing_capability_ids.clone();
            return outcome(
                clarification(
                    competing.clone(),
                    self.interpretations(&competing),
                    canonical.ambiguity.reason.clone().unwrap_or_else(|| {
                        "competing scientific interpretations require clarification".into()
                    }),
                ),
                canonical,
                competing,
                rejected_model_claims,
            );
        }

        if canonical.action == RequestedAction::Compute && !self.policy.compute_authorized {
            return outcome(
                PlannerDecision::PolicyBlocked {
                    reason: "compute is not authorized; a request for detail or evidence is not authorization to compute".into(),
                },
                canonical,
                Vec::new(),
                rejected_model_claims,
            );
        }

        let matches: Vec<&IntentSpec> = self
            .catalog
            .capabilities
            .iter()
            .filter(|spec| spec_matches(spec, &canonical))
            .collect();
        let matched_ids = matches
            .iter()
            .map(|spec| spec.id.clone())
            .collect::<Vec<_>>();
        if matches.len() > 1 {
            return outcome(
                clarification(
                    matched_ids.clone(),
                    self.interpretations(&matched_ids),
                    "multiple compatible capabilities; host will not guess",
                ),
                canonical,
                matched_ids,
                rejected_model_claims,
            );
        }
        if matches.is_empty() {
            let missing = self
                .catalog
                .capabilities
                .iter()
                .filter(|spec| spec_matches_ignoring_entities(spec, &canonical))
                .filter(|spec| missing_required_roles(spec, &canonical).is_some())
                .map(|spec| spec.id.clone())
                .collect::<Vec<_>>();
            if !missing.is_empty() {
                return outcome(
                    clarification(
                        missing.clone(),
                        self.interpretations(&missing),
                        "required entity roles are missing",
                    ),
                    canonical,
                    missing,
                    rejected_model_claims,
                );
            }
            return outcome(
                PlannerDecision::UnsupportedIntent {
                    reason: "no compatible capability for this canonical intent".into(),
                },
                canonical,
                Vec::new(),
                rejected_model_claims,
            );
        }

        let spec = matches[0];
        if let Some(allowed) = &self.policy.allowed_capability_ids {
            if !allowed.iter().any(|id| id == &spec.id) {
                return outcome(
                    PlannerDecision::PolicyBlocked {
                        reason: format!("capability `{}` is not enabled by host policy", spec.id),
                    },
                    canonical,
                    matched_ids,
                    rejected_model_claims,
                );
            }
        }
        if !self.policy.provider_available {
            return outcome(
                PlannerDecision::ProviderUnavailable {
                    reason: "the evidence provider is unavailable".into(),
                },
                canonical,
                matched_ids,
                rejected_model_claims,
            );
        }
        if let CoverageSignal::Gap { reason } = &self.policy.coverage {
            return outcome(
                PlannerDecision::CoverageGap {
                    capability_id: spec.id.clone(),
                    reason: reason.clone(),
                },
                canonical,
                matched_ids,
                rejected_model_claims,
            );
        }
        let Some(tool) = self.tools.get(&spec.tool) else {
            return outcome(
                PlannerDecision::BridgeUnavailable {
                    capability_id: spec.id.clone(),
                    tool: spec.tool.clone(),
                    reason: format!(
                        "declared capability `{}` is missing from the resolved tool registry",
                        spec.id
                    ),
                },
                canonical,
                matched_ids,
                rejected_model_claims,
            );
        };
        let arguments = match map_arguments(spec, &canonical) {
            Ok(arguments) => arguments,
            Err(reason) => {
                return outcome(
                    clarification(
                        vec![spec.id.clone()],
                        self.interpretations(&[spec.id.clone()]),
                        reason,
                    ),
                    canonical,
                    matched_ids,
                    rejected_model_claims,
                );
            }
        };
        if let Err(error) = validate_discovered_schema(&tool.input_schema, &arguments) {
            return outcome(
                PlannerDecision::BridgeUnavailable {
                    capability_id: spec.id.clone(),
                    tool: spec.tool.clone(),
                    reason: format!(
                        "planned call failed discovered MCP schema validation: {error}"
                    ),
                },
                canonical,
                matched_ids,
                rejected_model_claims,
            );
        }
        let mut canonical = canonical;
        canonical.proposed_capability = None;
        canonical.proposed_coverage = None;
        outcome(
            PlannerDecision::Execute {
                capability_id: spec.id.clone(),
                tool: spec.tool.clone(),
                arguments,
            },
            canonical,
            matched_ids,
            rejected_model_claims,
        )
    }

    fn interpretations(&self, ids: &[String]) -> Vec<Value> {
        ids.iter()
            .filter_map(|id| self.catalog.capabilities.iter().find(|spec| &spec.id == id))
            .map(|spec| {
                json!({
                    "capability_id": spec.id,
                    "relation": spec.relation,
                    "data_modality": spec.data_modality,
                    "metric": spec.metric,
                    "action": spec.action,
                    "scopes": spec.scopes,
                    "tool": spec.tool
                })
            })
            .collect()
    }
}

fn outcome(
    decision: PlannerDecision,
    canonical_intent: ScientificIntent,
    matched_capability_ids: Vec<String>,
    rejected_model_claims: Vec<String>,
) -> PlannerOutcome {
    PlannerOutcome {
        schema_version: INTENT_SCHEMA_VERSION,
        contract: PLANNER_CONTRACT_ID.to_string(),
        decision,
        canonical_intent,
        matched_capability_ids,
        rejected_model_claims,
    }
}

fn clarification(
    competing_capability_ids: Vec<String>,
    competing_interpretations: Vec<Value>,
    reason: impl Into<String>,
) -> PlannerDecision {
    PlannerDecision::ClarificationRequired {
        competing_capability_ids,
        competing_interpretations,
        reason: reason.into(),
    }
}

fn spec_matches(spec: &IntentSpec, intent: &ScientificIntent) -> bool {
    spec_matches_ignoring_entities(spec, intent) && missing_required_roles(spec, intent).is_none()
}

fn spec_matches_ignoring_entities(spec: &IntentSpec, intent: &ScientificIntent) -> bool {
    if spec.relation != "*" && spec.relation != intent.relation {
        return false;
    }
    if spec.action != intent.action {
        return false;
    }
    if !spec.scopes.is_empty() && !spec.scopes.contains(&intent.scope) {
        return false;
    }
    if let (Some(expected), Some(actual)) = (&spec.data_modality, &intent.data_modality) {
        if expected != actual {
            return false;
        }
    }
    if let (Some(expected), Some(actual)) = (&spec.metric, &intent.metric) {
        if expected != actual {
            return false;
        }
    }
    true
}

fn missing_required_roles(spec: &IntentSpec, intent: &ScientificIntent) -> Option<String> {
    for role in spec.entity_roles.iter().filter(|role| role.required) {
        match intent.entity(&role.role) {
            None => return Some(role.role.clone()),
            Some(entity) if !role.kind.is_empty() && entity.kind != role.kind => {
                return Some(role.role.clone())
            }
            Some(_) => {}
        }
    }
    if !spec.require_any_roles.is_empty()
        && spec
            .require_any_roles
            .iter()
            .all(|role| intent.entity(role).is_none())
    {
        return Some(spec.require_any_roles.join("|"));
    }
    None
}

fn map_arguments(spec: &IntentSpec, intent: &ScientificIntent) -> Result<Value, String> {
    let mut object = Map::new();
    for binding in &spec.arguments {
        match lookup_binding(intent, &binding.from) {
            Some(value) if !value.is_null() => {
                object.insert(binding.to.clone(), value);
            }
            Some(_) | None if binding.optional => {
                if let Some(default) = &binding.default {
                    if !default.is_null() {
                        object.insert(binding.to.clone(), default.clone());
                    }
                }
            }
            _ => {
                return Err(format!(
                    "required argument `{}` is missing from the canonical intent",
                    binding.to
                ));
            }
        }
    }
    Ok(Value::Object(object))
}

pub fn arguments_for_capability(
    catalog: &IntentCatalog,
    intent: &ScientificIntent,
    capability_id: &str,
) -> Value {
    catalog
        .capabilities
        .iter()
        .find(|spec| spec.id == capability_id)
        .and_then(|spec| map_arguments(spec, intent).ok())
        .unwrap_or(Value::Object(Map::new()))
}

fn lookup_binding(intent: &ScientificIntent, from: &str) -> Option<Value> {
    if let Some(role) = from.strip_prefix("entity.") {
        return intent
            .entity(role)
            .map(|entity| Value::String(entity.identifier.clone()));
    }
    if let Some(key) = from.strip_prefix("constraint.") {
        return intent.constraints.get(key).cloned();
    }
    match from {
        "relation" => Some(Value::String(intent.relation.clone())),
        "data_modality" => intent.data_modality.clone().map(Value::String),
        "metric" => intent.metric.clone().map(Value::String),
        "scope" => Some(Value::String(intent.scope.as_str().to_string())),
        "direction" => intent
            .direction
            .map(|direction| Value::String(direction.as_str().to_string())),
        "action" => Some(Value::String(intent.action.as_str().to_string())),
        "release" => intent.release.clone().map(Value::String),
        _ => None,
    }
}

/// Normalize aliases and drop null object fields before schema checks.
/// Compatible with the discovered MCP `inputSchema` subset from #93.
pub fn omit_null_object_fields(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.retain(|_, child| !child.is_null());
            for child in object.values_mut() {
                omit_null_object_fields(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                omit_null_object_fields(item);
            }
        }
        _ => {}
    }
}

pub fn validate_discovered_schema(schema: &Value, arguments: &Value) -> Result<(), String> {
    let mut arguments = arguments.clone();
    omit_null_object_fields(&mut arguments);
    validate_against_schema(schema, &arguments, "arguments")
}

fn validate_against_schema(schema: &Value, value: &Value, path: &str) -> Result<(), String> {
    if !schema.is_object() {
        return Ok(());
    }
    if let Some(all_of) = schema.get("allOf").and_then(Value::as_array) {
        for branch in all_of {
            validate_against_schema(branch, value, path)?;
        }
    }
    if let Some(any_of) = schema.get("anyOf").and_then(Value::as_array) {
        if !any_of
            .iter()
            .any(|branch| validate_against_schema(branch, value, path).is_ok())
        {
            return Err(format!("{path} does not match any anyOf branch"));
        }
    }
    if let Some(one_of) = schema.get("oneOf").and_then(Value::as_array) {
        let matches = one_of
            .iter()
            .filter(|branch| validate_against_schema(branch, value, path).is_ok())
            .count();
        if matches != 1 {
            return Err(format!(
                "{path} must match exactly one oneOf branch; matched {matches}"
            ));
        }
    }
    if let Some(type_constraint) = schema.get("type") {
        if !value_matches_type(value, type_constraint) {
            return Err(format!("{path} does not match the tool input schema"));
        }
    }
    if let Some(enum_values) = schema.get("enum").and_then(Value::as_array) {
        if !enum_values.iter().any(|allowed| allowed == value) {
            return Err(format!("{path} is not one of the allowed values"));
        }
    }
    if let Some(const_value) = schema.get("const") {
        if value != const_value {
            return Err(format!("{path} does not match the required const value"));
        }
    }
    validate_numeric_bounds(schema, value, path)?;
    validate_string_constraints(schema, value, path)?;
    if let Some(object) = value.as_object() {
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            for name in required.iter().filter_map(Value::as_str) {
                if !object.contains_key(name) {
                    return Err(format!("{path} is missing required property '{name}'"));
                }
            }
        }
        let properties = schema.get("properties").and_then(Value::as_object);
        if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
            if let Some(properties) = properties {
                if let Some(unknown) = object.keys().find(|key| !properties.contains_key(*key)) {
                    return Err(format!("{path} has unexpected property '{unknown}'"));
                }
            } else if !object.is_empty() {
                return Err(format!("{path} does not allow additional properties"));
            }
        }
        if let Some(min_properties) = schema.get("minProperties").and_then(Value::as_u64) {
            if (object.len() as u64) < min_properties {
                return Err(format!("{path} has fewer properties than minProperties"));
            }
        }
        if let Some(max_properties) = schema.get("maxProperties").and_then(Value::as_u64) {
            if (object.len() as u64) > max_properties {
                return Err(format!("{path} has more properties than maxProperties"));
            }
        }
        if let Some(properties) = properties {
            for (name, child) in object {
                if let Some(child_schema) = properties.get(name) {
                    validate_against_schema(child_schema, child, &format!("{path}.{name}"))?;
                }
            }
        }
    }
    if let Some(array) = value.as_array() {
        if let Some(min_items) = schema.get("minItems").and_then(Value::as_u64) {
            if (array.len() as u64) < min_items {
                return Err(format!("{path} has fewer items than minItems"));
            }
        }
        if let Some(max_items) = schema.get("maxItems").and_then(Value::as_u64) {
            if (array.len() as u64) > max_items {
                return Err(format!("{path} has more items than maxItems"));
            }
        }
        if let Some(items) = schema.get("items") {
            for (index, child) in array.iter().enumerate() {
                validate_against_schema(items, child, &format!("{path}[{index}]"))?;
            }
        }
    }
    Ok(())
}

fn validate_numeric_bounds(schema: &Value, value: &Value, path: &str) -> Result<(), String> {
    let Some(number) = value.as_f64() else {
        return Ok(());
    };
    if let Some(minimum) = schema.get("minimum").and_then(Value::as_f64) {
        let exclusive = schema
            .get("exclusiveMinimum")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if exclusive && number <= minimum {
            return Err(format!("{path} is not greater than exclusiveMinimum"));
        }
        if !exclusive && number < minimum {
            return Err(format!("{path} is below minimum"));
        }
    }
    if let Some(exclusive_minimum) = schema.get("exclusiveMinimum").and_then(Value::as_f64) {
        if number <= exclusive_minimum {
            return Err(format!("{path} is not greater than exclusiveMinimum"));
        }
    }
    if let Some(maximum) = schema.get("maximum").and_then(Value::as_f64) {
        let exclusive = schema
            .get("exclusiveMaximum")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if exclusive && number >= maximum {
            return Err(format!("{path} is not less than exclusiveMaximum"));
        }
        if !exclusive && number > maximum {
            return Err(format!("{path} is above maximum"));
        }
    }
    if let Some(exclusive_maximum) = schema.get("exclusiveMaximum").and_then(Value::as_f64) {
        if number >= exclusive_maximum {
            return Err(format!("{path} is not less than exclusiveMaximum"));
        }
    }
    Ok(())
}

fn validate_string_constraints(schema: &Value, value: &Value, path: &str) -> Result<(), String> {
    let Some(text) = value.as_str() else {
        return Ok(());
    };
    let len = text.chars().count() as u64;
    if let Some(min_length) = schema.get("minLength").and_then(Value::as_u64) {
        if len < min_length {
            return Err(format!("{path} is shorter than minLength"));
        }
    }
    if let Some(max_length) = schema.get("maxLength").and_then(Value::as_u64) {
        if len > max_length {
            return Err(format!("{path} is longer than maxLength"));
        }
    }
    if let Some(pattern) = schema.get("pattern").and_then(Value::as_str) {
        let regex = regex::Regex::new(pattern)
            .map_err(|error| format!("{path} has an invalid schema pattern: {error}"))?;
        if !regex.is_match(text) {
            return Err(format!("{path} does not match the required pattern"));
        }
    }
    Ok(())
}

fn value_matches_type(value: &Value, type_constraint: &Value) -> bool {
    match type_constraint {
        Value::String(type_name) => value_is_json_type(value, type_name),
        Value::Array(types) => types.iter().any(|item| {
            item.as_str()
                .is_some_and(|type_name| value_is_json_type(value, type_name))
        }),
        _ => true,
    }
}

fn value_is_json_type(value: &Value, type_name: &str) -> bool {
    match type_name {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => true,
    }
}

fn normalize_key(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut last_underscore = false;
    for character in value.trim().chars() {
        if character.is_whitespace() || matches!(character, '-' | '/' | '\\') {
            if !last_underscore && !out.is_empty() {
                out.push('_');
                last_underscore = true;
            }
        } else {
            for lower in character.to_lowercase() {
                out.push(lower);
            }
            last_underscore = false;
        }
    }
    out.trim_matches('_').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::specialist_manifest::HostPolicy;

    fn entity(role: &str, kind: &str, identifier: &str) -> IntentEntity {
        IntentEntity {
            role: role.into(),
            kind: kind.into(),
            identifier: identifier.into(),
            aliases: Vec::new(),
        }
    }

    fn retrieve_intent(relation: &str, modality: Option<&str>) -> ScientificIntent {
        ScientificIntent {
            schema_version: INTENT_SCHEMA_VERSION,
            entities: vec![entity("gene", "gene", "GENEA")],
            relation: relation.into(),
            data_modality: modality.map(str::to_string),
            metric: None,
            scope: IntentScope::Global,
            direction: Some(IntentDirection::Positive),
            action: RequestedAction::RetrieveEvidence,
            release: None,
            constraints: BTreeMap::new(),
            ambiguity: AmbiguityMetadata::default(),
            proposed_capability: None,
            proposed_coverage: None,
        }
    }

    fn fake_catalog() -> IntentCatalog {
        IntentCatalog::from_json(
            r#"{
              "schema_version": 1,
              "id": "fake.intent",
              "manifest_version": "1.0.0",
              "relation_aliases": {
                "co-dependency": {
                  "relation": "codependency",
                  "data_modality": "crispr_gene_effect",
                  "metric": "gene_effect_correlation"
                },
                "codependency": {
                  "relation": "codependency",
                  "data_modality": "crispr_gene_effect",
                  "metric": "gene_effect_correlation"
                },
                "共依赖": {
                  "relation": "codependency",
                  "data_modality": "crispr_gene_effect",
                  "metric": "gene_effect_correlation"
                },
                "crispr gene effect correlation": {
                  "relation": "codependency",
                  "data_modality": "crispr_gene_effect",
                  "metric": "gene_effect_correlation"
                },
                "coexpression": {
                  "relation": "coexpression",
                  "data_modality": "transcript_expression_log2_tpm_plus_1",
                  "metric": "expression_correlation"
                },
                "共表达": {
                  "relation": "coexpression",
                  "data_modality": "transcript_expression_log2_tpm_plus_1",
                  "metric": "expression_correlation"
                },
                "expression correlation": {
                  "relation": "coexpression",
                  "data_modality": "transcript_expression_log2_tpm_plus_1",
                  "metric": "expression_correlation"
                }
              },
              "modality_aliases": {
                "crispr_gene_effect": "crispr_gene_effect",
                "ceres": "crispr_gene_effect",
                "chronos": "crispr_gene_effect",
                "transcript_expression_log2_tpm_plus_1": "transcript_expression_log2_tpm_plus_1"
              },
              "ambiguous_relations": [
                {
                  "aliases": ["related", "有什么关系", "associated"],
                  "competing_capability_ids": ["codependency_query", "coexpression_query"]
                }
              ],
              "capabilities": [
                {
                  "id": "codependency_query",
                  "tool": "fake_codependency_tool",
                  "relation": "codependency",
                  "data_modality": "crispr_gene_effect",
                  "metric": "gene_effect_correlation",
                  "action": "retrieve_evidence",
                  "scopes": ["global", "lineage"],
                  "entity_roles": [{"role": "gene", "kind": "gene", "required": true}],
                  "arguments": [
                    {"from": "entity.gene", "to": "gene", "optional": false},
                    {"from": "entity.lineage", "to": "lineage", "optional": true},
                    {"from": "direction", "to": "direction", "optional": true},
                    {"from": "constraint.limit", "to": "limit", "optional": true, "default": 20}
                  ]
                },
                {
                  "id": "coexpression_query",
                  "tool": "fake_coexpression_tool",
                  "relation": "coexpression",
                  "data_modality": "transcript_expression_log2_tpm_plus_1",
                  "metric": "expression_correlation",
                  "action": "retrieve_evidence",
                  "scopes": ["global", "lineage"],
                  "entity_roles": [{"role": "gene", "kind": "gene", "required": true}],
                  "arguments": [
                    {"from": "entity.gene", "to": "gene", "optional": false},
                    {"from": "entity.lineage", "to": "lineage", "optional": true}
                  ]
                },
                {
                  "id": "mutation_global",
                  "tool": "fake_mutation_tool",
                  "relation": "event_stratified_dependency",
                  "data_modality": "somatic_mutation_event_vs_crispr_dependency",
                  "action": "retrieve_evidence",
                  "scopes": ["global"],
                  "entity_roles": [{"role": "source", "kind": "mutation_event", "required": true}],
                  "arguments": [
                    {"from": "entity.source", "to": "source", "optional": false},
                    {"from": "constraint.event", "to": "event", "optional": true}
                  ]
                },
                {
                  "id": "recompute_codependency",
                  "tool": "fake_compute_tool",
                  "relation": "codependency",
                  "data_modality": "crispr_gene_effect",
                  "action": "compute",
                  "scopes": ["global", "lineage"],
                  "entity_roles": [{"role": "gene", "kind": "gene", "required": true}],
                  "arguments": [{"from": "entity.gene", "to": "gene", "optional": false}]
                }
              ]
            }"#,
        )
        .unwrap()
    }

    fn codependency_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "gene": {"type": "string"},
                "lineage": {"type": "string"},
                "direction": {"type": "string"},
                "limit": {"type": "integer"}
            },
            "required": ["gene"],
            "additionalProperties": false
        })
    }

    fn tools_with_codependency() -> ToolCatalog {
        let mut tools = ToolCatalog::default();
        tools.insert("fake_codependency_tool", codependency_schema());
        tools.insert(
            "fake_coexpression_tool",
            json!({
                "type": "object",
                "properties": {
                    "gene": {"type": "string"},
                    "lineage": {"type": "string"}
                },
                "required": ["gene"],
                "additionalProperties": false
            }),
        );
        tools.insert(
            "fake_mutation_tool",
            json!({
                "type": "object",
                "properties": {
                    "source": {"type": "string"},
                    "event": {"type": "string"}
                },
                "required": ["source"],
                "additionalProperties": false
            }),
        );
        tools.insert(
            "fake_compute_tool",
            json!({
                "type": "object",
                "properties": {"gene": {"type": "string"}},
                "required": ["gene"],
                "additionalProperties": false
            }),
        );
        tools
    }

    fn plan(intent: ScientificIntent) -> PlannerOutcome {
        plan_scientific_intent(
            intent,
            &fake_catalog(),
            &tools_with_codependency(),
            &PlannerHostPolicy::default(),
        )
    }

    #[test]
    fn paraphrases_canonicalize_to_the_same_intent_and_capability() {
        let catalog = fake_catalog();
        let paraphrases = [
            retrieve_intent("co-dependency", Some("CRISPR Gene Effect")),
            retrieve_intent("共依赖", None),
            retrieve_intent("CRISPR Gene Effect correlation", None),
            retrieve_intent("codependency", Some("ceres")),
        ];
        let canonical: Vec<_> = paraphrases
            .into_iter()
            .map(|intent| catalog.canonicalize(intent))
            .collect();
        for item in &canonical {
            assert_eq!(item.relation, "codependency");
            assert_eq!(item.data_modality.as_deref(), Some("crispr_gene_effect"));
            assert_eq!(item.metric.as_deref(), Some("gene_effect_correlation"));
            assert_eq!(item.action, RequestedAction::RetrieveEvidence);
            assert_eq!(item.entities[0].identifier, "GENEA");
        }
        assert_eq!(canonical[0], canonical[1]);
        assert_eq!(canonical[1], canonical[2]);

        let decisions: Vec<_> = canonical
            .into_iter()
            .map(|intent| plan(intent).decision.kind().to_string())
            .collect();
        assert!(decisions.iter().all(|kind| kind == "execute"));
        let tools: Vec<_> = [
            retrieve_intent("co-dependency", None),
            retrieve_intent("共依赖", None),
        ]
        .into_iter()
        .map(|intent| match plan(intent).decision {
            PlannerDecision::Execute {
                tool,
                capability_id,
                ..
            } => (capability_id, tool),
            other => panic!("{other:?}"),
        })
        .collect();
        assert_eq!(tools[0], tools[1]);
        assert_eq!(tools[0].0, "codependency_query");
    }

    #[test]
    fn ambiguous_phrases_require_clarification_with_competing_interpretations() {
        let outcome = plan(retrieve_intent("有什么关系", None));
        match outcome.decision {
            PlannerDecision::ClarificationRequired {
                competing_capability_ids,
                competing_interpretations,
                ..
            } => {
                assert_eq!(
                    competing_capability_ids,
                    vec!["codependency_query", "coexpression_query"]
                );
                assert_eq!(competing_interpretations.len(), 2);
                assert_eq!(competing_interpretations[0]["relation"], "codependency");
                assert_eq!(competing_interpretations[1]["relation"], "coexpression");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn unsupported_scope_is_unsupported_intent_not_a_guessed_tool() {
        let mut intent = ScientificIntent {
            schema_version: INTENT_SCHEMA_VERSION,
            entities: vec![entity("source", "mutation_event", "GENEA")],
            relation: "event_stratified_dependency".into(),
            data_modality: Some("somatic_mutation_event_vs_crispr_dependency".into()),
            metric: None,
            scope: IntentScope::Lineage,
            direction: None,
            action: RequestedAction::RetrieveEvidence,
            release: None,
            constraints: BTreeMap::new(),
            ambiguity: AmbiguityMetadata::default(),
            proposed_capability: Some("mutation_global".into()),
            proposed_coverage: Some("FOUND".into()),
        };
        intent
            .entities
            .push(entity("lineage", "lineage", "ExampleLineage"));
        let outcome = plan(intent);
        assert_eq!(outcome.decision.kind(), "unsupported_intent");
        assert_eq!(
            outcome.rejected_model_claims,
            vec!["proposed_capability", "proposed_coverage"]
        );
        match outcome.decision {
            PlannerDecision::UnsupportedIntent { reason } => {
                assert!(reason.contains("no compatible capability"), "{reason}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn missing_declared_tool_is_bridge_unavailable_not_not_computed() {
        let outcome = plan_scientific_intent(
            retrieve_intent("codependency", None),
            &fake_catalog(),
            &ToolCatalog::default(),
            &PlannerHostPolicy::default(),
        );
        match outcome.decision {
            PlannerDecision::BridgeUnavailable {
                capability_id,
                tool,
                reason,
            } => {
                assert_eq!(capability_id, "codependency_query");
                assert_eq!(tool, "fake_codependency_tool");
                assert!(reason.contains("missing from the resolved tool registry"));
                assert!(!reason.contains("NOT_COMPUTED"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn planned_calls_omit_absent_optional_fields_instead_of_null() {
        let outcome = plan(retrieve_intent("共依赖", None));
        match outcome.decision {
            PlannerDecision::Execute { arguments, .. } => {
                assert_eq!(arguments["gene"], "GENEA");
                assert_eq!(arguments["limit"], 20);
                assert!(arguments.get("lineage").is_none());
                assert!(!arguments.as_object().unwrap().values().any(Value::is_null));
                let encoded = serde_json::to_string(&arguments).unwrap();
                assert!(!encoded.contains("null"), "{encoded}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn codependency_does_not_become_expression_or_synthetic_lethality() {
        let outcome = plan(retrieve_intent("codependency", None));
        match outcome.decision {
            PlannerDecision::Execute {
                capability_id,
                tool,
                ..
            } => {
                assert_eq!(capability_id, "codependency_query");
                assert_eq!(tool, "fake_codependency_tool");
            }
            other => panic!("{other:?}"),
        }
        let expression = plan(retrieve_intent("expression correlation", None));
        match expression.decision {
            PlannerDecision::Execute {
                capability_id,
                tool,
                ..
            } => {
                assert_eq!(capability_id, "coexpression_query");
                assert_eq!(tool, "fake_coexpression_tool");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn detail_request_does_not_authorize_compute() {
        let mut intent = retrieve_intent("codependency", None);
        intent.action = RequestedAction::parse("details").unwrap();
        let outcome = plan(intent);
        assert_eq!(outcome.decision.kind(), "execute");
        match outcome.decision {
            PlannerDecision::Execute { capability_id, .. } => {
                assert_eq!(capability_id, "codependency_query");
            }
            other => panic!("{other:?}"),
        }

        let mut compute = retrieve_intent("codependency", None);
        compute.action = RequestedAction::Compute;
        let blocked = plan(compute);
        assert_eq!(blocked.decision.kind(), "policy_blocked");
    }

    #[test]
    fn host_coverage_and_provider_signals_are_not_model_claims() {
        let mut policy = PlannerHostPolicy::default();
        policy.coverage = CoverageSignal::Gap {
            reason: "host inspected coverage and found a gap".into(),
        };
        let gap = plan_scientific_intent(
            retrieve_intent("codependency", None),
            &fake_catalog(),
            &tools_with_codependency(),
            &policy,
        );
        assert_eq!(gap.decision.kind(), "coverage_gap");

        policy.coverage = CoverageSignal::NotInspected;
        policy.provider_available = false;
        let unavailable = plan_scientific_intent(
            retrieve_intent("codependency", None),
            &fake_catalog(),
            &tools_with_codependency(),
            &policy,
        );
        assert_eq!(unavailable.decision.kind(), "provider_unavailable");
    }

    #[test]
    fn null_optionals_are_stripped_before_schema_validation() {
        let schema = codependency_schema();
        let mut arguments = json!({
            "gene": "GENEA",
            "lineage": null,
            "direction": "positive"
        });
        omit_null_object_fields(&mut arguments);
        validate_discovered_schema(&schema, &arguments).unwrap();
        assert!(arguments.get("lineage").is_none());
        let invalid =
            validate_discovered_schema(&schema, &json!({"gene": "GENEA", "invented": true}))
                .unwrap_err();
        assert!(invalid.contains("unexpected property"), "{invalid}");
    }

    #[test]
    fn discovered_schema_enforces_bounds_pattern_and_const() {
        let schema = json!({
            "type": "object",
            "properties": {
                "gene": {"type": "string", "minLength": 3, "pattern": "^[A-Z0-9]+$"},
                "count": {"type": "integer", "minimum": 1, "maximum": 3},
                "kind": {"const": "query"},
                "ids": {"type": "array", "minItems": 1, "maxItems": 2, "items": {"type": "string"}}
            },
            "required": ["gene", "count", "kind", "ids"],
            "additionalProperties": false
        });
        validate_discovered_schema(
            &schema,
            &json!({"gene": "PTK7", "count": 2, "kind": "query", "ids": ["a"]}),
        )
        .unwrap();
        let short = validate_discovered_schema(
            &schema,
            &json!({"gene": "AB", "count": 2, "kind": "query", "ids": ["a"]}),
        )
        .unwrap_err();
        assert!(short.contains("minLength"), "{short}");
        let pattern = validate_discovered_schema(
            &schema,
            &json!({"gene": "ptk7", "count": 2, "kind": "query", "ids": ["a"]}),
        )
        .unwrap_err();
        assert!(pattern.contains("pattern"), "{pattern}");
        let high = validate_discovered_schema(
            &schema,
            &json!({"gene": "PTK7", "count": 9, "kind": "query", "ids": ["a"]}),
        )
        .unwrap_err();
        assert!(high.contains("maximum"), "{high}");
        let empty = validate_discovered_schema(
            &schema,
            &json!({"gene": "PTK7", "count": 1, "kind": "query", "ids": []}),
        )
        .unwrap_err();
        assert!(empty.contains("minItems"), "{empty}");
        let constant = validate_discovered_schema(
            &schema,
            &json!({"gene": "PTK7", "count": 1, "kind": "other", "ids": ["a"]}),
        )
        .unwrap_err();
        assert!(constant.contains("const"), "{constant}");
    }

    #[test]
    fn discovered_schema_enforces_nested_combinator_constraints() {
        let schema = json!({
            "type": "object",
            "properties": {
                "event": {
                    "anyOf": [
                        {"type": "string", "enum": ["damaging_mutation", "hotspot_mutation"]},
                        {"type": "null"}
                    ]
                },
                "mode": {
                    "oneOf": [
                        {"const": "global"},
                        {"const": "lineage"}
                    ]
                }
            },
            "required": ["event", "mode"]
        });
        validate_discovered_schema(
            &schema,
            &json!({"event": "damaging_mutation", "mode": "global"}),
        )
        .unwrap();
        let event = validate_discovered_schema(
            &schema,
            &json!({"event": "removed_event", "mode": "global"}),
        )
        .unwrap_err();
        assert!(event.contains("anyOf"), "{event}");
    }

    #[test]
    fn desktop_cli_eval_delegation_and_resume_share_the_planner_contract() {
        let bridge = host_scientific_bridge(&HostPolicy::bundled_depmap()).unwrap();
        assert_eq!(bridge.specialist.id, "depmap_r_agent");
        assert_eq!(bridge.catalog.schema_version, INTENT_SCHEMA_VERSION);
        assert_eq!(PLANNER_CONTRACT_ID, "scientific_intent.bridge_planner.v1");
        let outcome = plan_scientific_intent(
            retrieve_intent("codependency", None),
            &bridge.catalog,
            &{
                let mut tools = ToolCatalog::default();
                tools.insert(
                    "depmap_codependency_evidence",
                    json!({
                        "type": "object",
                        "properties": {
                            "gene": {"type": "string"},
                            "lineage": {"type": "string"},
                            "direction": {"type": "string"},
                            "limit": {"type": "integer"}
                        },
                        "required": ["gene"],
                        "additionalProperties": false
                    }),
                );
                tools
            },
            &PlannerHostPolicy::default(),
        );
        assert_eq!(outcome.contract, PLANNER_CONTRACT_ID);
        assert_eq!(outcome.decision.kind(), "execute");
    }

    #[test]
    fn bundled_intent_catalog_is_domain_predicates_not_gene_branches() {
        let catalog = IntentCatalog::bundled_depmap();
        let encoded = serde_json::to_string(&catalog).unwrap();
        for forbidden in ["PTK7", "GPX4", "ESR1", "KRAS", "Liver", "Breast"] {
            assert!(
                !encoded.contains(forbidden),
                "catalog must not encode ticket examples: {forbidden}"
            );
        }
        assert!(catalog
            .capabilities
            .iter()
            .any(|spec| spec.relation == "codependency"));
        assert!(catalog
            .capabilities
            .iter()
            .any(|spec| spec.relation == "coexpression"));
        assert!(catalog
            .capabilities
            .iter()
            .any(|spec| spec.relation == "event_stratified_dependency"));
    }

    #[test]
    fn global_mutation_events_use_the_pan_cancer_vocabulary() {
        let mut intent = ScientificIntent {
            schema_version: INTENT_SCHEMA_VERSION,
            entities: vec![entity("source", "mutation_event", "GENEA")],
            relation: "event_stratified_dependency".into(),
            data_modality: Some("somatic_mutation_event_vs_crispr_dependency".into()),
            metric: Some("delta_gene_effect".into()),
            scope: IntentScope::Global,
            direction: None,
            action: RequestedAction::RetrieveEvidence,
            release: None,
            constraints: BTreeMap::from([("event".into(), json!("damaging"))]),
            ambiguity: AmbiguityMetadata::default(),
            proposed_capability: None,
            proposed_coverage: None,
        };
        let outcome = plan(intent.clone());
        match outcome.decision {
            PlannerDecision::Execute { arguments, .. } => {
                assert_eq!(arguments["event"], "damaging_mutation");
            }
            other => panic!("{other:?}"),
        }
        intent.scope = IntentScope::Lineage;
        intent
            .entities
            .push(entity("lineage", "lineage", "ExampleLineage"));
        let lineage = IntentCatalog::bundled_depmap().canonicalize(intent);
        assert_eq!(lineage.constraints["event"], json!("damaging"));
    }

    #[test]
    fn pair_capabilities_bind_discovered_mcp_parameters() {
        let mut tools = ToolCatalog::default();
        tools.insert(
            "depmap_pair_evidence",
            json!({
                "type": "object",
                "properties": {
                    "source": {"type": "string"},
                    "target": {"type": "string"},
                    "lineage": {"type": "string"}
                },
                "required": ["source", "target"],
                "additionalProperties": false
            }),
        );
        let outcome = plan_scientific_intent(
            ScientificIntent {
                schema_version: INTENT_SCHEMA_VERSION,
                entities: vec![
                    entity("source", "gene", "GENEA"),
                    entity("target", "gene", "GENEB"),
                ],
                relation: "coexpression".into(),
                data_modality: Some("transcript_expression_log2_tpm_plus_1".into()),
                metric: Some("expression_correlation".into()),
                scope: IntentScope::Global,
                direction: None,
                action: RequestedAction::RetrieveEvidence,
                release: None,
                constraints: BTreeMap::from([("limit".into(), json!(20))]),
                ambiguity: AmbiguityMetadata::default(),
                proposed_capability: None,
                proposed_coverage: None,
            },
            &IntentCatalog::bundled_depmap(),
            &tools,
            &PlannerHostPolicy::default(),
        );
        match outcome.decision {
            PlannerDecision::Execute {
                tool, arguments, ..
            } => {
                assert_eq!(tool, "depmap_pair_evidence");
                assert_eq!(arguments["source"], "GENEA");
                assert_eq!(arguments["target"], "GENEB");
                assert!(arguments.get("source_gene").is_none());
                assert!(arguments.get("limit").is_none());
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn planner_installs_into_a_host_registry_after_tools_exist() {
        let mut registry = wisp_tools::Registry::builtins();
        install_scientific_intent_planner(&mut registry);
        assert!(registry
            .names()
            .iter()
            .any(|name| *name == SCIENTIFIC_INTENT_PLAN_TOOL));
    }
}

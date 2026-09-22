//! Deterministic release gate for scientific Agent capability/use-case units.
//!
//! ACUs name a question family and its typed contract. Named genes and
//! lineages are fixtures only; the gate closes capabilities and terminal
//! bridge decisions, never example strings.

use crate::scientific_intent::PlannerHostPolicy;
use crate::specialist_manifest::ResolvedSpecialistSnapshot;
use crate::{
    plan_scientific_intent, IntentCatalog, PlannerDecision, ScientificIntent, ToolCatalog,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const ACU_CORPUS_SCHEMA: &str = "wisp.agent-capability-use-cases.v1";
pub const RELEASE_GATE_SCHEMA: &str = "wisp.release-capability-gate.v1";
pub const DEPMAP_SERVER_CONTRACT_SCHEMA: &str = "wisp.depmap-server-contract-snapshot.v1";
pub const RELEASE_CONTRACT_LOCK_SCHEMA: &str = "wisp.release-contract-lock.v1";

const TERMINAL_DECISIONS: &[&str] = &[
    "execute",
    "clarification_required",
    "unsupported_intent",
    "coverage_gap",
    "bridge_unavailable",
    "provider_unavailable",
    "policy_blocked",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcuCoverageState {
    Computed,
    NotRetained,
    NotTested,
    NotComputed,
    AnnotationUnavailable,
    BridgeUnavailable,
    ProviderUnavailable,
    PolicyBlocked,
}

impl AcuCoverageState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Computed => "computed",
            Self::NotRetained => "not_retained",
            Self::NotTested => "not_tested",
            Self::NotComputed => "not_computed",
            Self::AnnotationUnavailable => "annotation_unavailable",
            Self::BridgeUnavailable => "bridge_unavailable",
            Self::ProviderUnavailable => "provider_unavailable",
            Self::PolicyBlocked => "policy_blocked",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcuEvidenceInvariant {
    StructuredEvidenceRequired,
    ReleaseRequired,
    ScopeRequired,
    CoverageStatePreserved,
    ClaimsMustBeGrounded,
    NoRawMatrixIo,
    NoUnsupportedClaim,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcuBudget {
    pub max_tool_calls: u32,
    pub max_context_tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AcuFixture {
    pub coverage: AcuCoverageState,
    #[serde(default = "default_true")]
    pub provider_available: bool,
    #[serde(default)]
    pub compute_authorized: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_capability_ids: Option<Vec<String>>,
    #[serde(default)]
    pub tool_schemas: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AcuCase {
    pub id: String,
    pub question_family: Vec<String>,
    pub canonical_intent: ScientificIntent,
    #[serde(default)]
    pub allowed_ambiguity: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_capability: Option<String>,
    #[serde(default)]
    pub expected_query_shape: Value,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub forbidden_tools: Vec<String>,
    pub allowed_terminal_decisions: Vec<String>,
    pub fixture: AcuFixture,
    pub budget: AcuBudget,
    pub evidence_invariants: Vec<AcuEvidenceInvariant>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AcuCorpus {
    pub schema: String,
    pub id: String,
    pub version: String,
    pub release: String,
    pub cases: Vec<AcuCase>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AcuReplay {
    pub id: String,
    pub passed: bool,
    pub decision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
    #[serde(default)]
    pub failures: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseGateInputs {
    pub release: String,
    pub reader_modes: BTreeMap<String, String>,
    pub coverage: BTreeMap<String, AcuCoverageState>,
    pub provider_contract: String,
    pub evidence_envelope_contract: String,
    pub claim_validator_contract: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_digests: Option<ReleaseContractDigests>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseContractDigests {
    pub capability: String,
    pub coverage: String,
    pub server_contract: String,
    pub acu_suite: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseContractLock {
    pub schema: String,
    pub release: String,
    pub digests: ReleaseContractDigests,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DepMapServerContractSnapshot {
    pub schema: String,
    pub release: String,
    pub provider_contract: String,
    pub evidence_envelope_contract: String,
    pub reader_modes: BTreeMap<String, String>,
    pub coverage: BTreeMap<String, AcuCoverageState>,
    pub tool_schemas: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseCanaryPolicy {
    pub allowed_tools: BTreeSet<String>,
    pub max_tool_calls: u32,
    pub max_result_bytes: usize,
    pub mutation_allowed: bool,
    pub ssh_allowed: bool,
    pub raw_matrix_io_allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseCanaryObservation {
    pub tool: String,
    pub call_number: u32,
    pub result_bytes: usize,
    pub read_only: bool,
    pub uses_ssh: bool,
    pub uses_raw_matrix_io: bool,
}

impl Default for ReleaseCanaryPolicy {
    fn default() -> Self {
        Self {
            allowed_tools: ["depmap_status", "depmap_analysis_catalog"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            max_tool_calls: 2,
            max_result_bytes: 256 * 1024,
            mutation_allowed: false,
            ssh_allowed: false,
            raw_matrix_io_allowed: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityClosureRow {
    pub capability_id: String,
    pub intent_mapped: bool,
    pub specialist_manifest_version: String,
    pub tool: String,
    pub tool_registered: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discovered_schema_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reader_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage: Option<AcuCoverageState>,
    pub provider_contract: String,
    pub evidence_envelope_contract: String,
    pub claim_validator_contract: String,
    #[serde(default)]
    pub acu_ids: Vec<String>,
    pub closed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseGateBlockerKind {
    DeterministicContract,
    ModelQuality,
    ProviderCanary,
    ReleaseAssembly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseGateBlocker {
    pub kind: ReleaseGateBlockerKind,
    pub code: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReleaseGateArtifact {
    pub schema: String,
    pub release: String,
    pub capability_digest: String,
    pub coverage_digest: String,
    pub server_contract_digest: String,
    pub acu_suite_digest: String,
    pub rows: Vec<CapabilityClosureRow>,
    pub replays: Vec<AcuReplay>,
    pub canary_policy: ReleaseCanaryPolicy,
    pub blockers: Vec<ReleaseGateBlocker>,
    pub passed: bool,
}

/// Validate one optional provider-canary observation without granting the
/// canary any capability beyond the release-gate policy.
pub fn validate_release_canary(
    policy: &ReleaseCanaryPolicy,
    observation: &ReleaseCanaryObservation,
) -> Result<(), ReleaseGateBlocker> {
    let failure = if !policy.allowed_tools.contains(&observation.tool) {
        Some((
            "forbidden_canary_tool",
            format!(
                "tool '{}' is outside the canary allowlist",
                observation.tool
            ),
        ))
    } else if observation.call_number == 0 || observation.call_number > policy.max_tool_calls {
        Some((
            "canary_call_budget_exceeded",
            format!(
                "call {} exceeds the {}-call canary budget",
                observation.call_number, policy.max_tool_calls
            ),
        ))
    } else if observation.result_bytes > policy.max_result_bytes {
        Some((
            "canary_result_too_large",
            format!(
                "{} bytes exceeds the {}-byte canary result budget",
                observation.result_bytes, policy.max_result_bytes
            ),
        ))
    } else if !observation.read_only && !policy.mutation_allowed {
        Some((
            "canary_mutation_forbidden",
            "the release canary must be read-only".into(),
        ))
    } else if observation.uses_ssh && !policy.ssh_allowed {
        Some((
            "canary_ssh_forbidden",
            "the release canary must not open an SSH execution context".into(),
        ))
    } else if observation.uses_raw_matrix_io && !policy.raw_matrix_io_allowed {
        Some((
            "canary_raw_matrix_forbidden",
            "the release canary must not read or download raw matrices".into(),
        ))
    } else {
        None
    };

    match failure {
        Some((code, detail)) => Err(ReleaseGateBlocker {
            kind: ReleaseGateBlockerKind::ProviderCanary,
            code: code.into(),
            detail,
        }),
        None => Ok(()),
    }
}

pub fn load_bundled_depmap_acu_corpus() -> AcuCorpus {
    serde_json::from_str(include_str!("../fixtures/depmap-acu-v1.json"))
        .expect("bundled DepMap ACU corpus must be valid JSON")
}

pub fn load_bundled_depmap_server_contract() -> DepMapServerContractSnapshot {
    let snapshot: DepMapServerContractSnapshot =
        serde_json::from_str(include_str!("../fixtures/depmap-mcp-contract-v1.json"))
            .expect("bundled DepMap MCP contract snapshot must be valid JSON");
    assert_eq!(
        snapshot.schema, DEPMAP_SERVER_CONTRACT_SCHEMA,
        "bundled DepMap MCP contract snapshot schema drifted"
    );
    snapshot
}

pub fn load_bundled_depmap_release_lock() -> ReleaseContractLock {
    let contract: ReleaseContractLock =
        serde_json::from_str(include_str!("../fixtures/depmap-release-lock-v1.json"))
            .expect("bundled DepMap release lock must be valid JSON");
    assert_eq!(
        contract.schema, RELEASE_CONTRACT_LOCK_SCHEMA,
        "bundled DepMap release lock schema drifted"
    );
    contract
}

pub fn validate_acu_corpus(corpus: &AcuCorpus, catalog: &IntentCatalog) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    if corpus.schema != ACU_CORPUS_SCHEMA {
        errors.push(format!(
            "unsupported ACU corpus schema '{}'; expected {ACU_CORPUS_SCHEMA}",
            corpus.schema
        ));
    }
    if corpus.id.trim().is_empty()
        || corpus.version.trim().is_empty()
        || corpus.release.trim().is_empty()
    {
        errors.push("ACU corpus id, version, and release must be non-empty".into());
    }
    let capability_ids: BTreeSet<_> = catalog
        .capabilities
        .iter()
        .map(|capability| capability.id.as_str())
        .collect();
    let mut ids = BTreeSet::new();
    let mut covered_capabilities = BTreeSet::new();
    let mut covered_decisions = BTreeSet::new();
    for case in &corpus.cases {
        if case.id.trim().is_empty() || !ids.insert(case.id.as_str()) {
            errors.push(format!(
                "ACU ids must be non-empty and unique: '{}'",
                case.id
            ));
        }
        if case.question_family.is_empty()
            || case
                .question_family
                .iter()
                .any(|question| question.trim().is_empty())
        {
            errors.push(format!(
                "ACU '{}' needs non-empty question variants",
                case.id
            ));
        }
        if case.budget.max_tool_calls == 0 || case.budget.max_context_tokens == 0 {
            errors.push(format!("ACU '{}' needs positive budgets", case.id));
        }
        if case.evidence_invariants.is_empty() {
            errors.push(format!("ACU '{}' needs typed evidence invariants", case.id));
        }
        if case.allowed_terminal_decisions.is_empty() {
            errors.push(format!(
                "ACU '{}' needs a terminal bridge decision",
                case.id
            ));
        }
        for decision in &case.allowed_terminal_decisions {
            if !TERMINAL_DECISIONS.contains(&decision.as_str()) {
                errors.push(format!(
                    "ACU '{}' names unknown terminal decision '{decision}'",
                    case.id
                ));
            }
            covered_decisions.insert(decision.as_str());
        }
        if let Some(capability) = &case.expected_capability {
            if !capability_ids.contains(capability.as_str()) {
                errors.push(format!(
                    "ACU '{}' names unknown capability '{capability}'",
                    case.id
                ));
            }
            covered_capabilities.insert(capability.as_str());
        }
        for tool in &case.allowed_tools {
            if case.forbidden_tools.contains(tool) {
                errors.push(format!(
                    "ACU '{}' allows and forbids tool '{tool}'",
                    case.id
                ));
            }
        }
    }
    for capability in capability_ids {
        if !covered_capabilities.contains(capability) {
            errors.push(format!("capability '{capability}' has no ACU"));
        }
    }
    for decision in TERMINAL_DECISIONS {
        if !covered_decisions.contains(decision) {
            errors.push(format!("terminal decision '{decision}' has no ACU"));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

pub fn replay_acu(case: &AcuCase, catalog: &IntentCatalog) -> AcuReplay {
    let mut tools = ToolCatalog::default();
    for (name, schema) in &case.fixture.tool_schemas {
        tools.insert(name.clone(), schema.clone());
    }
    replay_acu_with_tools(case, catalog, &tools)
}

fn replay_acu_with_tools(
    case: &AcuCase,
    catalog: &IntentCatalog,
    tools: &ToolCatalog,
) -> AcuReplay {
    let coverage = match case.fixture.coverage {
        AcuCoverageState::Computed
        | AcuCoverageState::BridgeUnavailable
        | AcuCoverageState::ProviderUnavailable
        | AcuCoverageState::PolicyBlocked => crate::scientific_intent::CoverageSignal::Available,
        state => crate::scientific_intent::CoverageSignal::Gap {
            reason: state.as_str().into(),
        },
    };
    let policy = PlannerHostPolicy {
        provider_available: case.fixture.provider_available,
        compute_authorized: case.fixture.compute_authorized,
        coverage,
        allowed_capability_ids: case.fixture.allowed_capability_ids.clone(),
    };
    let outcome = plan_scientific_intent(case.canonical_intent.clone(), catalog, tools, &policy);
    let decision = outcome.decision.kind().to_string();
    let mut replay = AcuReplay {
        id: case.id.clone(),
        passed: true,
        decision: decision.clone(),
        capability_id: None,
        tool: None,
        arguments: None,
        failures: Vec::new(),
    };
    if !case.allowed_terminal_decisions.contains(&decision) {
        replay.failures.push(format!(
            "terminal decision '{decision}' is outside {:?}",
            case.allowed_terminal_decisions
        ));
    }
    match outcome.decision {
        PlannerDecision::Execute {
            capability_id,
            tool,
            arguments,
        } => {
            if case.expected_capability.as_deref() != Some(capability_id.as_str()) {
                replay.failures.push(format!(
                    "expected capability {:?}, got '{capability_id}'",
                    case.expected_capability
                ));
            }
            if !case.allowed_tools.contains(&tool) {
                replay
                    .failures
                    .push(format!("tool '{tool}' is not allowed by the ACU"));
            }
            if case.forbidden_tools.contains(&tool) {
                replay
                    .failures
                    .push(format!("tool '{tool}' is explicitly forbidden"));
            }
            if !json_subset(&case.expected_query_shape, &arguments) {
                replay.failures.push(format!(
                    "query shape {} does not match arguments {}",
                    case.expected_query_shape, arguments
                ));
            }
            replay.capability_id = Some(capability_id);
            replay.tool = Some(tool);
            replay.arguments = Some(arguments);
        }
        decision => {
            if let Some(expected) = &case.expected_capability {
                let actual = decision_capability(&decision);
                if actual != Some(expected.as_str()) {
                    replay.failures.push(format!(
                        "expected terminal capability '{expected}', got {actual:?}"
                    ));
                }
            }
            if matches!(
                case.fixture.coverage,
                AcuCoverageState::NotRetained
                    | AcuCoverageState::NotTested
                    | AcuCoverageState::NotComputed
                    | AcuCoverageState::AnnotationUnavailable
            ) && !decision_reason(&decision)
                .is_some_and(|reason| reason.contains(case.fixture.coverage.as_str()))
            {
                replay.failures.push(format!(
                    "coverage state '{}' was collapsed",
                    case.fixture.coverage.as_str()
                ));
            }
        }
    }
    replay.passed = replay.failures.is_empty();
    replay
}

pub fn build_release_gate(
    corpus: &AcuCorpus,
    catalog: &IntentCatalog,
    specialist: &ResolvedSpecialistSnapshot,
    tools: &ToolCatalog,
    inputs: &ReleaseGateInputs,
) -> ReleaseGateArtifact {
    let mut blockers = validate_acu_corpus(corpus, catalog)
        .err()
        .unwrap_or_default()
        .into_iter()
        .map(|detail| ReleaseGateBlocker {
            kind: ReleaseGateBlockerKind::DeterministicContract,
            code: "invalid_acu_corpus".into(),
            detail,
        })
        .collect::<Vec<_>>();
    if corpus.release != inputs.release {
        blockers.push(ReleaseGateBlocker {
            kind: ReleaseGateBlockerKind::ReleaseAssembly,
            code: "release_mismatch".into(),
            detail: format!(
                "ACU corpus release '{}' does not match '{}'",
                corpus.release, inputs.release
            ),
        });
    }
    let replays: Vec<_> = corpus
        .cases
        .iter()
        .map(|case| {
            if case
                .allowed_terminal_decisions
                .iter()
                .any(|decision| decision == "execute")
            {
                replay_acu_with_tools(case, catalog, tools)
            } else {
                replay_acu(case, catalog)
            }
        })
        .collect();
    for replay in &replays {
        for failure in &replay.failures {
            blockers.push(ReleaseGateBlocker {
                kind: ReleaseGateBlockerKind::DeterministicContract,
                code: "acu_replay_failed".into(),
                detail: format!("{}: {failure}", replay.id),
            });
        }
    }
    let mut rows = Vec::new();
    for capability in &catalog.capabilities {
        let discovered = tools.get(&capability.tool);
        let acu_ids: Vec<_> = corpus
            .cases
            .iter()
            .filter(|case| case.expected_capability.as_deref() == Some(&capability.id))
            .map(|case| case.id.clone())
            .collect();
        let reader_mode = inputs.reader_modes.get(&capability.id).cloned();
        let coverage = inputs.coverage.get(&capability.id).copied();
        let schema_digest = discovered.map(|tool| digest_json(&tool.input_schema));
        let closed = discovered.is_some()
            && reader_mode.as_ref().is_some_and(|mode| !mode.is_empty())
            && coverage.is_some()
            && !inputs.provider_contract.is_empty()
            && !inputs.evidence_envelope_contract.is_empty()
            && !inputs.claim_validator_contract.is_empty()
            && !acu_ids.is_empty();
        if !closed {
            blockers.push(ReleaseGateBlocker {
                kind: ReleaseGateBlockerKind::ReleaseAssembly,
                code: "capability_not_closed".into(),
                detail: capability.id.clone(),
            });
        }
        rows.push(CapabilityClosureRow {
            capability_id: capability.id.clone(),
            intent_mapped: true,
            specialist_manifest_version: specialist.manifest_version.clone(),
            tool: capability.tool.clone(),
            tool_registered: discovered.is_some(),
            discovered_schema_digest: schema_digest,
            reader_mode,
            coverage,
            provider_contract: inputs.provider_contract.clone(),
            evidence_envelope_contract: inputs.evidence_envelope_contract.clone(),
            claim_validator_contract: inputs.claim_validator_contract.clone(),
            acu_ids,
            closed,
        });
    }
    let capability_digest = digest_json(&catalog.capabilities);
    let coverage_digest = digest_json(&inputs.coverage);
    let server_contract_digest = digest_json(&Value::Array(
        rows.iter()
            .map(|row| {
                serde_json::json!({
                    "tool": row.tool,
                    "schema": row.discovered_schema_digest,
                    "reader_mode": row.reader_mode,
                    "provider": row.provider_contract,
                    "evidence_envelope": row.evidence_envelope_contract,
                    "claim_validator": row.claim_validator_contract,
                    "specialist_manifest_version": row.specialist_manifest_version,
                })
            })
            .collect(),
    ));
    let acu_suite_digest = digest_json(corpus);
    if let Some(expected) = &inputs.expected_digests {
        for (code, label, actual, expected) in [
            (
                "stale_capability_digest",
                "capability",
                capability_digest.as_str(),
                expected.capability.as_str(),
            ),
            (
                "stale_coverage_digest",
                "coverage",
                coverage_digest.as_str(),
                expected.coverage.as_str(),
            ),
            (
                "stale_server_contract_digest",
                "server contract",
                server_contract_digest.as_str(),
                expected.server_contract.as_str(),
            ),
            (
                "stale_acu_suite_digest",
                "ACU suite",
                acu_suite_digest.as_str(),
                expected.acu_suite.as_str(),
            ),
        ] {
            if actual != expected {
                blockers.push(ReleaseGateBlocker {
                    kind: ReleaseGateBlockerKind::ReleaseAssembly,
                    code: code.into(),
                    detail: format!("{label} digest changed: expected {expected}, got {actual}"),
                });
            }
        }
    }
    let passed = blockers.is_empty();
    ReleaseGateArtifact {
        schema: RELEASE_GATE_SCHEMA.into(),
        release: inputs.release.clone(),
        capability_digest,
        coverage_digest,
        server_contract_digest,
        acu_suite_digest,
        rows,
        replays,
        canary_policy: ReleaseCanaryPolicy::default(),
        blockers,
        passed,
    }
}

fn decision_capability(decision: &PlannerDecision) -> Option<&str> {
    match decision {
        PlannerDecision::Execute { capability_id, .. }
        | PlannerDecision::CoverageGap { capability_id, .. }
        | PlannerDecision::BridgeUnavailable { capability_id, .. } => Some(capability_id),
        _ => None,
    }
}

fn decision_reason(decision: &PlannerDecision) -> Option<&str> {
    match decision {
        PlannerDecision::ClarificationRequired { reason, .. }
        | PlannerDecision::UnsupportedIntent { reason }
        | PlannerDecision::CoverageGap { reason, .. }
        | PlannerDecision::BridgeUnavailable { reason, .. }
        | PlannerDecision::ProviderUnavailable { reason }
        | PlannerDecision::PolicyBlocked { reason } => Some(reason),
        PlannerDecision::Execute { .. } => None,
    }
}

fn json_subset(expected: &Value, actual: &Value) -> bool {
    match (expected, actual) {
        (Value::Object(expected), Value::Object(actual)) => expected.iter().all(|(key, value)| {
            actual
                .get(key)
                .is_some_and(|candidate| json_subset(value, candidate))
        }),
        (Value::Array(expected), Value::Array(actual)) => {
            expected.len() == actual.len()
                && expected
                    .iter()
                    .zip(actual)
                    .all(|(left, right)| json_subset(left, right))
        }
        _ => expected == actual,
    }
}

fn digest_json(value: &impl Serialize) -> String {
    let value = serde_json::to_value(value).unwrap_or(Value::Null);
    format!("{:x}", Sha256::digest(canonical_json(&value).as_bytes()))
}

fn canonical_json(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => serde_json::to_string(value).unwrap_or_default(),
        Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(canonical_json)
                .collect::<Vec<_>>()
                .join(",")
        ),
        Value::Object(values) => {
            let mut keys: Vec<_> = values.keys().collect();
            keys.sort_unstable();
            format!(
                "{{{}}}",
                keys.into_iter()
                    .map(|key| format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap_or_default(),
                        canonical_json(&values[key])
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
    }
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scientific_intent::host_scientific_bridge;
    use crate::specialist_manifest::HostPolicy;

    fn assembled_tools(corpus: &AcuCorpus) -> ToolCatalog {
        let mut tools = ToolCatalog::default();
        for case in &corpus.cases {
            for (name, schema) in &case.fixture.tool_schemas {
                tools.insert(name.clone(), schema.clone());
            }
        }
        tools
    }

    fn production_contract_tools() -> ToolCatalog {
        let mut tools = ToolCatalog::default();
        for (name, schema) in load_bundled_depmap_server_contract().tool_schemas {
            tools.insert(name, schema);
        }
        tools
    }

    fn gate_inputs(catalog: &IntentCatalog) -> ReleaseGateInputs {
        ReleaseGateInputs {
            release: "26Q1".into(),
            reader_modes: catalog
                .capabilities
                .iter()
                .map(|capability| (capability.id.clone(), "bounded_precomputed".into()))
                .collect(),
            coverage: catalog
                .capabilities
                .iter()
                .map(|capability| (capability.id.clone(), AcuCoverageState::Computed))
                .collect(),
            provider_contract: "depmap-mcp.v1".into(),
            evidence_envelope_contract: "wisp.mcp-tool-result.v1".into(),
            claim_validator_contract: crate::CLAIM_RECORD_CONTRACT.into(),
            expected_digests: None,
        }
    }

    #[test]
    fn bundled_corpus_covers_every_capability_and_terminal_decision() {
        let corpus = load_bundled_depmap_acu_corpus();
        let catalog = IntentCatalog::bundled_depmap();
        validate_acu_corpus(&corpus, &catalog).unwrap();
        let replays: Vec<_> = corpus
            .cases
            .iter()
            .map(|case| replay_acu(case, &catalog))
            .collect();
        assert!(replays.iter().all(|replay| replay.passed), "{replays:#?}");
        assert!(replays.iter().any(|replay| {
            replay.id == "bridge-unavailable-is-not-not-computed"
                && replay.decision == "bridge_unavailable"
        }));
    }

    #[test]
    fn closure_matrix_uses_production_manifest_catalog_and_contract_digests() {
        let corpus = load_bundled_depmap_acu_corpus();
        let bridge = host_scientific_bridge(&HostPolicy::bundled_depmap()).unwrap();
        let tools = production_contract_tools();
        let artifact = build_release_gate(
            &corpus,
            &bridge.catalog,
            &bridge.specialist,
            &tools,
            &gate_inputs(&bridge.catalog),
        );
        assert!(artifact.passed, "{:#?}", artifact.blockers);
        assert_eq!(artifact.rows.len(), bridge.catalog.capabilities.len());
        assert!(artifact.rows.iter().all(|row| row.closed));
        assert!(artifact.rows.iter().all(|row| {
            row.discovered_schema_digest
                .as_ref()
                .is_some_and(|digest| digest.len() == 64)
        }));
        assert_eq!(artifact.capability_digest.len(), 64);
        assert_eq!(artifact.coverage_digest.len(), 64);
        assert_eq!(artifact.server_contract_digest.len(), 64);
        assert_eq!(artifact.acu_suite_digest.len(), 64);
        assert!(artifact.replays.iter().any(|replay| {
            replay.id == "stale-schema-blocks-dispatch" && replay.decision == "bridge_unavailable"
        }));
    }

    #[test]
    fn stale_release_and_missing_schema_fail_before_scientific_dispatch() {
        let corpus = load_bundled_depmap_acu_corpus();
        let bridge = host_scientific_bridge(&HostPolicy::bundled_depmap()).unwrap();
        let mut inputs = gate_inputs(&bridge.catalog);
        inputs.release = "stale-release".into();
        let artifact = build_release_gate(
            &corpus,
            &bridge.catalog,
            &bridge.specialist,
            &ToolCatalog::default(),
            &inputs,
        );
        assert!(!artifact.passed);
        assert!(artifact
            .blockers
            .iter()
            .any(|blocker| blocker.code == "release_mismatch"));
        assert!(artifact
            .blockers
            .iter()
            .any(|blocker| blocker.code == "capability_not_closed"));
    }

    #[test]
    fn production_schema_must_accept_every_executable_acu_query() {
        let corpus = load_bundled_depmap_acu_corpus();
        let bridge = host_scientific_bridge(&HostPolicy::bundled_depmap()).unwrap();
        let mut tools = production_contract_tools();
        tools.insert(
            "depmap_codependency_evidence",
            serde_json::json!({
                "type": "object",
                "required": ["renamed_gene"],
                "properties": {"renamed_gene": {"type": "string"}},
                "additionalProperties": false
            }),
        );

        let artifact = build_release_gate(
            &corpus,
            &bridge.catalog,
            &bridge.specialist,
            &tools,
            &gate_inputs(&bridge.catalog),
        );

        assert!(!artifact.passed);
        assert!(artifact.blockers.iter().any(|blocker| {
            blocker.code == "acu_replay_failed" && blocker.detail.contains("codependency-execute")
        }));
    }

    #[test]
    fn server_digest_covers_reader_envelope_validator_and_manifest_contracts() {
        let corpus = load_bundled_depmap_acu_corpus();
        let bridge = host_scientific_bridge(&HostPolicy::bundled_depmap()).unwrap();
        let tools = production_contract_tools();
        let inputs = gate_inputs(&bridge.catalog);
        let baseline = build_release_gate(
            &corpus,
            &bridge.catalog,
            &bridge.specialist,
            &tools,
            &inputs,
        );

        let mut reader = inputs.clone();
        reader
            .reader_modes
            .insert("codependency_evidence".into(), "changed-reader".into());
        let mut envelope = inputs.clone();
        envelope.evidence_envelope_contract = "changed-envelope".into();
        let mut validator = inputs.clone();
        validator.claim_validator_contract = "changed-validator".into();
        let mut specialist = bridge.specialist.clone();
        specialist.manifest_version = "changed-manifest".into();

        for changed in [
            build_release_gate(
                &corpus,
                &bridge.catalog,
                &bridge.specialist,
                &tools,
                &reader,
            ),
            build_release_gate(
                &corpus,
                &bridge.catalog,
                &bridge.specialist,
                &tools,
                &envelope,
            ),
            build_release_gate(
                &corpus,
                &bridge.catalog,
                &bridge.specialist,
                &tools,
                &validator,
            ),
            build_release_gate(&corpus, &bridge.catalog, &specialist, &tools, &inputs),
        ] {
            assert_ne!(
                changed.server_contract_digest,
                baseline.server_contract_digest
            );
        }
    }

    #[test]
    fn stale_contract_digests_are_separate_release_blockers() {
        let corpus = load_bundled_depmap_acu_corpus();
        let bridge = host_scientific_bridge(&HostPolicy::bundled_depmap()).unwrap();
        let tools = assembled_tools(&corpus);
        let mut inputs = gate_inputs(&bridge.catalog);
        inputs.expected_digests = Some(ReleaseContractDigests {
            capability: "stale-capability".into(),
            coverage: "stale-coverage".into(),
            server_contract: "stale-server".into(),
            acu_suite: "stale-acu".into(),
        });
        let artifact = build_release_gate(
            &corpus,
            &bridge.catalog,
            &bridge.specialist,
            &tools,
            &inputs,
        );
        let codes: BTreeSet<_> = artifact
            .blockers
            .iter()
            .map(|blocker| blocker.code.as_str())
            .collect();
        for expected in [
            "stale_capability_digest",
            "stale_coverage_digest",
            "stale_server_contract_digest",
            "stale_acu_suite_digest",
        ] {
            assert!(codes.contains(expected), "{codes:?}");
        }
    }

    #[test]
    fn optional_canary_is_read_only_bounded_and_provider_scoped() {
        let canary = ReleaseCanaryPolicy::default();
        assert_eq!(
            canary.allowed_tools,
            ["depmap_analysis_catalog", "depmap_status"]
                .into_iter()
                .map(str::to_string)
                .collect()
        );
        assert_eq!(canary.max_tool_calls, 2);
        assert!(canary.max_result_bytes <= 256 * 1024);
        assert!(!canary.mutation_allowed);
        assert!(!canary.ssh_allowed);
        assert!(!canary.raw_matrix_io_allowed);

        validate_release_canary(
            &canary,
            &ReleaseCanaryObservation {
                tool: "depmap_status".into(),
                call_number: 1,
                result_bytes: 1024,
                read_only: true,
                uses_ssh: false,
                uses_raw_matrix_io: false,
            },
        )
        .unwrap();

        for observation in [
            ReleaseCanaryObservation {
                tool: "shell".into(),
                call_number: 1,
                result_bytes: 0,
                read_only: true,
                uses_ssh: false,
                uses_raw_matrix_io: false,
            },
            ReleaseCanaryObservation {
                tool: "depmap_status".into(),
                call_number: 3,
                result_bytes: 0,
                read_only: true,
                uses_ssh: false,
                uses_raw_matrix_io: false,
            },
            ReleaseCanaryObservation {
                tool: "depmap_analysis_catalog".into(),
                call_number: 1,
                result_bytes: canary.max_result_bytes + 1,
                read_only: true,
                uses_ssh: false,
                uses_raw_matrix_io: false,
            },
            ReleaseCanaryObservation {
                tool: "depmap_status".into(),
                call_number: 1,
                result_bytes: 0,
                read_only: false,
                uses_ssh: false,
                uses_raw_matrix_io: false,
            },
            ReleaseCanaryObservation {
                tool: "depmap_status".into(),
                call_number: 1,
                result_bytes: 0,
                read_only: true,
                uses_ssh: true,
                uses_raw_matrix_io: false,
            },
            ReleaseCanaryObservation {
                tool: "depmap_status".into(),
                call_number: 1,
                result_bytes: 0,
                read_only: true,
                uses_ssh: false,
                uses_raw_matrix_io: true,
            },
        ] {
            let blocker = validate_release_canary(&canary, &observation).unwrap_err();
            assert_eq!(blocker.kind, ReleaseGateBlockerKind::ProviderCanary);
        }
    }

    #[test]
    fn negative_wording_is_not_graded_by_substring() {
        let catalog = IntentCatalog::bundled_depmap();
        let mut case = load_bundled_depmap_acu_corpus()
            .cases
            .into_iter()
            .find(|case| case.id == "mutation-global-execute")
            .unwrap();
        case.question_family
            .push("The returned association does not establish synthetic lethality.".into());
        let replay = replay_acu(&case, &catalog);
        assert!(replay.passed, "{:#?}", replay.failures);
    }
}

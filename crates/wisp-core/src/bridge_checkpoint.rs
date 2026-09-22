//! Durable continuation of a nonterminal BridgePlanner decision.
//!
//! Resume uses the persisted intent, capability, and arguments. It never asks
//! the model to recreate the original scientific request from chat text.

use crate::scientific_intent::{
    plan_scientific_intent, IntentCatalog, PlannerDecision, PlannerHostPolicy, PlannerOutcome,
    ScientificIntent, ToolCatalog, INTENT_SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const BRIDGE_CHECKPOINT_CONTRACT: &str = "wisp.bridge-checkpoint.v1";
pub const BRIDGE_CHECKPOINT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractDigests {
    pub manifest: String,
    pub intent_catalog: String,
    pub mcp_schema: String,
    pub provider: String,
    pub coverage: String,
    pub authorization_scope: String,
}

impl ContractDigests {
    pub fn fingerprint(&self) -> String {
        digest_parts(&[
            &self.manifest,
            &self.intent_catalog,
            &self.mcp_schema,
            &self.provider,
            &self.coverage,
            &self.authorization_scope,
        ])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointState {
    ClarificationRequired,
    CoverageGap,
    NewAnalysisProposed,
    AwaitingApproval,
    AwaitingRun,
    ProviderReconnect,
    UncertainExternal,
    Cancelled,
    Completed,
}

impl CheckpointState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClarificationRequired => "clarification_required",
            Self::CoverageGap => "coverage_gap",
            Self::NewAnalysisProposed => "new_analysis_proposed",
            Self::AwaitingApproval => "awaiting_approval",
            Self::AwaitingRun => "awaiting_run",
            Self::ProviderReconnect => "provider_reconnect",
            Self::UncertainExternal => "uncertain_external",
            Self::Cancelled => "cancelled",
            Self::Completed => "completed",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Cancelled | Self::Completed)
    }

    pub fn may_resume(self) -> bool {
        !self.is_terminal()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectReceipt {
    pub operation_id: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BridgeCheckpoint {
    pub contract: String,
    pub schema_version: u32,
    pub checkpoint_id: String,
    pub operation_id: String,
    pub project_id: String,
    pub session_id: String,
    pub intent: ScientificIntent,
    pub capability_id: Option<String>,
    pub capability_version: String,
    pub arguments: Value,
    pub release: Option<String>,
    pub digests: ContractDigests,
    pub decision: PlannerDecision,
    pub state: CheckpointState,
    pub next_transition: String,
    pub proposal_id: Option<String>,
    pub run_id: Option<String>,
    pub artifact_ids: Vec<String>,
    pub unresolved_fields: Vec<String>,
    pub effect_receipts: Vec<EffectReceipt>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResumeEnvironment<'a> {
    pub project_id: &'a str,
    pub session_id: &'a str,
    pub catalog: &'a IntentCatalog,
    pub tools: &'a ToolCatalog,
    pub policy: &'a PlannerHostPolicy,
    pub digests: &'a ContractDigests,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResumeAction {
    ContinueBare,
    Clarify {
        field: String,
        value: Value,
    },
    ProposeAnalysis {
        operation_id: String,
    },
    Approve {
        proposal_id: String,
        project_id: String,
        session_id: String,
    },
    SubmitRun {
        operation_id: String,
        run_id: String,
    },
    ReconcileExternal {
        succeeded: bool,
        operation_id: String,
    },
    RetryUncertain,
    Cancel,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResumeOutcome {
    Resumed(BridgeCheckpoint),
    ResumeContractChanged { reason: String },
    Rejected { reason: String },
    Idempotent { checkpoint: BridgeCheckpoint },
}

pub fn checkpoint_for_outcome(
    outcome: &PlannerOutcome,
    project_id: &str,
    session_id: &str,
    operation_id: &str,
    capability_version: &str,
    digests: ContractDigests,
) -> Option<BridgeCheckpoint> {
    let (state, next_transition, capability_id, arguments, unresolved) = match &outcome.decision {
        PlannerDecision::ClarificationRequired { .. } => (
            CheckpointState::ClarificationRequired,
            "clarify_unresolved_fields",
            None,
            Value::Null,
            unresolved_fields(&outcome.canonical_intent),
        ),
        PlannerDecision::CoverageGap { capability_id, .. } => (
            CheckpointState::CoverageGap,
            "propose_analysis",
            Some(capability_id.clone()),
            arguments_of(&outcome.decision),
            Vec::new(),
        ),
        PlannerDecision::ProviderUnavailable { .. } | PlannerDecision::BridgeUnavailable { .. } => {
            (
                CheckpointState::ProviderReconnect,
                "reconnect_provider",
                capability_of(&outcome.decision),
                arguments_of(&outcome.decision),
                Vec::new(),
            )
        }
        PlannerDecision::Execute { .. }
        | PlannerDecision::UnsupportedIntent { .. }
        | PlannerDecision::PolicyBlocked { .. } => return None,
    };
    Some(BridgeCheckpoint {
        contract: BRIDGE_CHECKPOINT_CONTRACT.into(),
        schema_version: BRIDGE_CHECKPOINT_SCHEMA_VERSION,
        checkpoint_id: digest_parts(&[operation_id, project_id, session_id]),
        operation_id: operation_id.into(),
        project_id: project_id.into(),
        session_id: session_id.into(),
        intent: outcome.canonical_intent.clone(),
        capability_id,
        capability_version: capability_version.into(),
        arguments,
        release: outcome.canonical_intent.release.clone(),
        digests,
        decision: outcome.decision.clone(),
        state,
        next_transition: next_transition.into(),
        proposal_id: None,
        run_id: None,
        artifact_ids: Vec::new(),
        unresolved_fields: unresolved,
        effect_receipts: Vec::new(),
    })
}

pub fn resume_bridge_checkpoint(
    checkpoint: BridgeCheckpoint,
    action: ResumeAction,
    env: ResumeEnvironment<'_>,
) -> ResumeOutcome {
    if checkpoint.schema_version != BRIDGE_CHECKPOINT_SCHEMA_VERSION
        || checkpoint.contract != BRIDGE_CHECKPOINT_CONTRACT
    {
        return ResumeOutcome::ResumeContractChanged {
            reason: "incompatible checkpoint schema".into(),
        };
    }
    if checkpoint.intent.schema_version != INTENT_SCHEMA_VERSION {
        return ResumeOutcome::ResumeContractChanged {
            reason: "incompatible intent schema".into(),
        };
    }
    if checkpoint.digests.fingerprint() != env.digests.fingerprint() {
        return ResumeOutcome::ResumeContractChanged {
            reason: "resume_contract_changed".into(),
        };
    }
    if checkpoint.project_id != env.project_id {
        return ResumeOutcome::Rejected {
            reason: "checkpoint is project-scoped".into(),
        };
    }
    if checkpoint.state.is_terminal() {
        return ResumeOutcome::Rejected {
            reason: format!("checkpoint is already {}", checkpoint.state.as_str()),
        };
    }
    match action {
        ResumeAction::ContinueBare => resume_bare(checkpoint, env),
        ResumeAction::Clarify { field, value } => resume_clarify(checkpoint, field, value, env),
        ResumeAction::ProposeAnalysis { operation_id } => resume_propose(checkpoint, operation_id),
        ResumeAction::Approve {
            proposal_id,
            project_id,
            session_id,
        } => resume_approve(checkpoint, &proposal_id, &project_id, &session_id),
        ResumeAction::SubmitRun {
            operation_id,
            run_id,
        } => resume_submit_run(checkpoint, operation_id, run_id),
        ResumeAction::ReconcileExternal {
            succeeded,
            operation_id,
        } => resume_reconcile(checkpoint, succeeded, operation_id),
        ResumeAction::RetryUncertain => ResumeOutcome::Rejected {
            reason: "uncertain external outcome requires reconciliation, not retry".into(),
        },
        ResumeAction::Cancel => {
            let mut checkpoint = checkpoint;
            checkpoint.state = CheckpointState::Cancelled;
            checkpoint.next_transition = "cancelled".into();
            ResumeOutcome::Resumed(checkpoint)
        }
    }
}

fn resume_bare(checkpoint: BridgeCheckpoint, env: ResumeEnvironment<'_>) -> ResumeOutcome {
    match checkpoint.state {
        CheckpointState::ClarificationRequired => ResumeOutcome::Resumed(checkpoint),
        CheckpointState::CoverageGap
        | CheckpointState::NewAnalysisProposed
        | CheckpointState::AwaitingApproval
        | CheckpointState::AwaitingRun
        | CheckpointState::ProviderReconnect => {
            let planned = plan_scientific_intent(
                checkpoint.intent.clone(),
                env.catalog,
                env.tools,
                env.policy,
            );
            if !same_frozen_plan(&checkpoint, &planned) {
                return ResumeOutcome::Rejected {
                    reason: "bare continue cannot switch capability, release, or arguments".into(),
                };
            }
            ResumeOutcome::Resumed(checkpoint)
        }
        CheckpointState::UncertainExternal => ResumeOutcome::Rejected {
            reason: "uncertain external outcome requires reconciliation, not retry".into(),
        },
        CheckpointState::Cancelled | CheckpointState::Completed => ResumeOutcome::Rejected {
            reason: "checkpoint is terminal".into(),
        },
    }
}

fn resume_clarify(
    mut checkpoint: BridgeCheckpoint,
    field: String,
    value: Value,
    env: ResumeEnvironment<'_>,
) -> ResumeOutcome {
    if checkpoint.state != CheckpointState::ClarificationRequired {
        return ResumeOutcome::Rejected {
            reason: format!(
                "clarification is not the next transition ({})",
                checkpoint.next_transition
            ),
        };
    }
    if !checkpoint
        .unresolved_fields
        .iter()
        .any(|item| item == &field)
    {
        return ResumeOutcome::Rejected {
            reason: format!("field `{field}` is not an unresolved clarification slot"),
        };
    }
    let original_relation = checkpoint.intent.relation.clone();
    let original_roles: Vec<_> = checkpoint
        .intent
        .entities
        .iter()
        .map(|entity| (entity.role.clone(), entity.kind.clone()))
        .collect();
    apply_field(&mut checkpoint.intent, &field, value);
    checkpoint.unresolved_fields.retain(|item| item != &field);
    let planned = plan_scientific_intent(
        checkpoint.intent.clone(),
        env.catalog,
        env.tools,
        env.policy,
    );
    if planned.canonical_intent.relation != original_relation {
        return ResumeOutcome::Rejected {
            reason: "clarification cannot overwrite the original relation".into(),
        };
    }
    for (role, kind) in original_roles {
        match planned.canonical_intent.entity(&role) {
            Some(entity) if entity.kind == kind => {}
            _ => {
                return ResumeOutcome::Rejected {
                    reason: "clarification cannot overwrite original entity roles".into(),
                };
            }
        }
    }
    checkpoint.intent = planned.canonical_intent.clone();
    checkpoint.decision = planned.decision.clone();
    checkpoint.release = planned.canonical_intent.release.clone();
    match &planned.decision {
        PlannerDecision::Execute {
            capability_id,
            arguments,
            ..
        } => {
            checkpoint.capability_id = Some(capability_id.clone());
            checkpoint.arguments = arguments.clone();
            checkpoint.state = CheckpointState::Completed;
            checkpoint.next_transition = "execute".into();
            checkpoint.unresolved_fields.clear();
        }
        PlannerDecision::ClarificationRequired { .. } => {
            checkpoint.unresolved_fields = unresolved_fields(&planned.canonical_intent);
            checkpoint.next_transition = "clarify_unresolved_fields".into();
        }
        other => {
            checkpoint.capability_id = capability_of(other);
            checkpoint.arguments = arguments_of(other);
        }
    }
    ResumeOutcome::Resumed(checkpoint)
}

fn resume_propose(mut checkpoint: BridgeCheckpoint, operation_id: String) -> ResumeOutcome {
    if let Some(existing) = checkpoint
        .effect_receipts
        .iter()
        .find(|receipt| receipt.operation_id == operation_id)
    {
        if existing.kind == "proposal" {
            return ResumeOutcome::Idempotent {
                checkpoint: checkpoint.clone(),
            };
        }
    }
    if checkpoint.state != CheckpointState::CoverageGap {
        return ResumeOutcome::Rejected {
            reason: format!(
                "analysis proposal is not the next transition ({})",
                checkpoint.next_transition
            ),
        };
    }
    let proposal_id = digest_parts(&[
        &operation_id,
        checkpoint.capability_id.as_deref().unwrap_or(""),
        &checkpoint.arguments.to_string(),
        checkpoint.release.as_deref().unwrap_or(""),
    ]);
    checkpoint.proposal_id = Some(proposal_id);
    checkpoint.state = CheckpointState::NewAnalysisProposed;
    checkpoint.next_transition = "await_approval".into();
    checkpoint.effect_receipts.push(EffectReceipt {
        operation_id,
        kind: "proposal".into(),
    });
    ResumeOutcome::Resumed(checkpoint)
}

fn resume_approve(
    mut checkpoint: BridgeCheckpoint,
    proposal_id: &str,
    project_id: &str,
    session_id: &str,
) -> ResumeOutcome {
    if !matches!(
        checkpoint.state,
        CheckpointState::NewAnalysisProposed | CheckpointState::AwaitingApproval
    ) {
        return ResumeOutcome::Rejected {
            reason: format!(
                "approval is not the next transition ({})",
                checkpoint.next_transition
            ),
        };
    }
    if project_id != checkpoint.project_id || session_id != checkpoint.session_id {
        return ResumeOutcome::Rejected {
            reason: "approval cannot be replayed in another project or session".into(),
        };
    }
    if checkpoint.proposal_id.as_deref() != Some(proposal_id) {
        return ResumeOutcome::Rejected {
            reason: "approval is bound to the exact proposal".into(),
        };
    }
    checkpoint.state = CheckpointState::AwaitingApproval;
    checkpoint.next_transition = "submit_run".into();
    ResumeOutcome::Resumed(checkpoint)
}

fn resume_submit_run(
    mut checkpoint: BridgeCheckpoint,
    operation_id: String,
    run_id: String,
) -> ResumeOutcome {
    if checkpoint.state != CheckpointState::AwaitingApproval {
        return ResumeOutcome::Rejected {
            reason: format!(
                "run submission is not the next transition ({})",
                checkpoint.next_transition
            ),
        };
    }
    if let Some(existing) = checkpoint
        .effect_receipts
        .iter()
        .find(|receipt| receipt.operation_id == operation_id && receipt.kind == "run")
    {
        let _ = existing;
        return ResumeOutcome::Idempotent {
            checkpoint: checkpoint.clone(),
        };
    }
    checkpoint.run_id = Some(run_id);
    checkpoint.state = CheckpointState::AwaitingRun;
    checkpoint.next_transition = "reconcile_run".into();
    checkpoint.effect_receipts.push(EffectReceipt {
        operation_id,
        kind: "run".into(),
    });
    ResumeOutcome::Resumed(checkpoint)
}

fn resume_reconcile(
    mut checkpoint: BridgeCheckpoint,
    succeeded: bool,
    operation_id: String,
) -> ResumeOutcome {
    if checkpoint.state == CheckpointState::UncertainExternal
        && checkpoint
            .effect_receipts
            .iter()
            .any(|receipt| receipt.operation_id == operation_id && receipt.kind == "reconcile")
    {
        return ResumeOutcome::Idempotent {
            checkpoint: checkpoint.clone(),
        };
    }
    if !matches!(
        checkpoint.state,
        CheckpointState::AwaitingRun
            | CheckpointState::UncertainExternal
            | CheckpointState::ProviderReconnect
    ) {
        return ResumeOutcome::Rejected {
            reason: format!(
                "reconciliation is not the next transition ({})",
                checkpoint.next_transition
            ),
        };
    }
    checkpoint.effect_receipts.push(EffectReceipt {
        operation_id,
        kind: "reconcile".into(),
    });
    if succeeded {
        checkpoint.state = CheckpointState::Completed;
        checkpoint.next_transition = "completed".into();
    } else {
        checkpoint.state = CheckpointState::UncertainExternal;
        checkpoint.next_transition = "reconcile_external".into();
    }
    ResumeOutcome::Resumed(checkpoint)
}

fn same_frozen_plan(checkpoint: &BridgeCheckpoint, planned: &PlannerOutcome) -> bool {
    if planned.canonical_intent.relation != checkpoint.intent.relation {
        return false;
    }
    if planned.canonical_intent.release != checkpoint.release {
        return false;
    }
    match (&checkpoint.decision, &planned.decision) {
        (
            PlannerDecision::CoverageGap { capability_id, .. },
            PlannerDecision::CoverageGap {
                capability_id: next,
                ..
            },
        ) => capability_id == next,
        (
            PlannerDecision::Execute {
                capability_id,
                arguments,
                ..
            },
            PlannerDecision::Execute {
                capability_id: next_id,
                arguments: next_args,
                ..
            },
        ) => capability_id == next_id && arguments == next_args,
        (left, right) => {
            left.kind() == right.kind() && checkpoint.capability_id == capability_of(right)
        }
    }
}

fn apply_field(intent: &mut ScientificIntent, field: &str, value: Value) {
    if let Some(role) = field.strip_prefix("entity.") {
        let identifier = value.as_str().unwrap_or_default().to_string();
        if let Some(entity) = intent
            .entities
            .iter_mut()
            .find(|entity| entity.role == role)
        {
            entity.identifier = identifier;
        } else {
            intent
                .entities
                .push(crate::scientific_intent::IntentEntity {
                    role: role.into(),
                    kind: role.into(),
                    identifier,
                    aliases: Vec::new(),
                });
        }
        return;
    }
    match field {
        "relation" => {
            if let Some(relation) = value.as_str() {
                intent.relation = relation.into();
            }
        }
        "release" => intent.release = value.as_str().map(str::to_string),
        other => {
            intent.constraints.insert(other.into(), value);
        }
    }
}

fn unresolved_fields(intent: &ScientificIntent) -> Vec<String> {
    let mut fields = Vec::new();
    if intent.entity("gene").is_none() && intent.entity("source").is_none() {
        fields.push("entity.gene".into());
    }
    if intent.scope == crate::scientific_intent::IntentScope::Lineage
        && intent.entity("lineage").is_none()
    {
        fields.push("entity.lineage".into());
    }
    fields
}

fn capability_of(decision: &PlannerDecision) -> Option<String> {
    match decision {
        PlannerDecision::Execute { capability_id, .. }
        | PlannerDecision::CoverageGap { capability_id, .. }
        | PlannerDecision::BridgeUnavailable { capability_id, .. } => Some(capability_id.clone()),
        _ => None,
    }
}

fn arguments_of(decision: &PlannerDecision) -> Value {
    match decision {
        PlannerDecision::Execute { arguments, .. } => arguments.clone(),
        _ => Value::Null,
    }
}

pub fn digest_parts(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    format!("{:x}", hasher.finalize())
}

pub fn persist_checkpoint(
    by_operation: &mut BTreeMap<String, BridgeCheckpoint>,
    checkpoint: BridgeCheckpoint,
) -> ResumeOutcome {
    if let Some(existing) = by_operation.get(&checkpoint.operation_id) {
        return ResumeOutcome::Idempotent {
            checkpoint: existing.clone(),
        };
    }
    by_operation.insert(checkpoint.operation_id.clone(), checkpoint.clone());
    ResumeOutcome::Resumed(checkpoint)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scientific_intent::{CoverageSignal, IntentEntity, IntentScope, RequestedAction};

    fn entity(role: &str, kind: &str, identifier: &str) -> IntentEntity {
        IntentEntity {
            role: role.into(),
            kind: kind.into(),
            identifier: identifier.into(),
            aliases: Vec::new(),
        }
    }

    fn retrieve_intent(relation: &str) -> ScientificIntent {
        ScientificIntent {
            schema_version: INTENT_SCHEMA_VERSION,
            entities: vec![entity("gene", "gene", "GENEA")],
            relation: relation.into(),
            data_modality: None,
            metric: None,
            scope: IntentScope::Global,
            direction: None,
            action: RequestedAction::RetrieveEvidence,
            release: Some("25Q2".into()),
            constraints: BTreeMap::new(),
            ambiguity: Default::default(),
            proposed_capability: None,
            proposed_coverage: None,
        }
    }

    fn catalog() -> IntentCatalog {
        IntentCatalog::from_json(
            r#"{
              "schema_version": 1,
              "id": "fake.intent",
              "manifest_version": "1.0.0",
              "relation_aliases": {
                "codependency": {
                  "relation": "codependency",
                  "data_modality": "crispr_gene_effect",
                  "metric": "gene_effect_correlation"
                }
              },
              "modality_aliases": {},
              "ambiguous_relations": [
                {
                  "aliases": ["related"],
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
                    {"from": "entity.lineage", "to": "lineage", "optional": true}
                  ]
                },
                {
                  "id": "coexpression_query",
                  "tool": "fake_coexpression_tool",
                  "relation": "coexpression",
                  "data_modality": "transcript_expression_log2_tpm_plus_1",
                  "action": "retrieve_evidence",
                  "scopes": ["global"],
                  "entity_roles": [{"role": "gene", "kind": "gene", "required": true}],
                  "arguments": [{"from": "entity.gene", "to": "gene", "optional": false}]
                }
              ]
            }"#,
        )
        .unwrap()
    }

    fn tools() -> ToolCatalog {
        let mut tools = ToolCatalog::default();
        tools.insert(
            "fake_codependency_tool",
            json!({
                "type": "object",
                "properties": {"gene": {"type": "string"}, "lineage": {"type": "string"}},
                "required": ["gene"],
                "additionalProperties": false
            }),
        );
        tools.insert(
            "fake_coexpression_tool",
            json!({
                "type": "object",
                "properties": {"gene": {"type": "string"}},
                "required": ["gene"],
                "additionalProperties": false
            }),
        );
        tools
    }

    fn digests() -> ContractDigests {
        ContractDigests {
            manifest: "manifest-1".into(),
            intent_catalog: "catalog-1".into(),
            mcp_schema: "schema-1".into(),
            provider: "provider-1".into(),
            coverage: "coverage-gap".into(),
            authorization_scope: "project-a".into(),
        }
    }

    fn env<'a>(
        catalog: &'a IntentCatalog,
        tools: &'a ToolCatalog,
        policy: &'a PlannerHostPolicy,
        digests: &'a ContractDigests,
    ) -> ResumeEnvironment<'a> {
        ResumeEnvironment {
            project_id: "project-a",
            session_id: "session-a",
            catalog,
            tools,
            policy,
            digests,
        }
    }

    fn gap_checkpoint() -> (
        BridgeCheckpoint,
        IntentCatalog,
        ToolCatalog,
        PlannerHostPolicy,
        ContractDigests,
    ) {
        let catalog = catalog();
        let tools = tools();
        let policy = PlannerHostPolicy {
            provider_available: true,
            compute_authorized: true,
            coverage: CoverageSignal::Gap {
                reason: "index inspected a coverage gap".into(),
            },
            allowed_capability_ids: None,
        };
        let outcome =
            plan_scientific_intent(retrieve_intent("codependency"), &catalog, &tools, &policy);
        let checkpoint = checkpoint_for_outcome(
            &outcome,
            "project-a",
            "session-a",
            "op-gap",
            "1.0.0",
            digests(),
        )
        .unwrap();
        (checkpoint, catalog, tools, policy, digests())
    }

    #[test]
    fn coverage_gap_proposal_approval_and_run_keep_frozen_plan() {
        let (checkpoint, catalog, tools, policy, digests) = gap_checkpoint();
        assert_eq!(checkpoint.state, CheckpointState::CoverageGap);
        assert_eq!(
            checkpoint.capability_id.as_deref(),
            Some("codependency_query")
        );
        let env = env(&catalog, &tools, &policy, &digests);
        let proposed = match resume_bridge_checkpoint(
            checkpoint,
            ResumeAction::ProposeAnalysis {
                operation_id: "propose-1".into(),
            },
            env.clone(),
        ) {
            ResumeOutcome::Resumed(checkpoint) => checkpoint,
            other => panic!("{other:?}"),
        };
        assert_eq!(proposed.state, CheckpointState::NewAnalysisProposed);
        let replay = resume_bridge_checkpoint(
            proposed.clone(),
            ResumeAction::ProposeAnalysis {
                operation_id: "propose-1".into(),
            },
            env.clone(),
        );
        assert!(matches!(replay, ResumeOutcome::Idempotent { .. }));
        let approved = match resume_bridge_checkpoint(
            proposed.clone(),
            ResumeAction::Approve {
                proposal_id: proposed.proposal_id.clone().unwrap(),
                project_id: "project-a".into(),
                session_id: "session-a".into(),
            },
            env.clone(),
        ) {
            ResumeOutcome::Resumed(checkpoint) => checkpoint,
            other => panic!("{other:?}"),
        };
        let running = match resume_bridge_checkpoint(
            approved,
            ResumeAction::SubmitRun {
                operation_id: "run-1".into(),
                run_id: "run-row".into(),
            },
            env.clone(),
        ) {
            ResumeOutcome::Resumed(checkpoint) => checkpoint,
            other => panic!("{other:?}"),
        };
        assert_eq!(running.state, CheckpointState::AwaitingRun);
        assert_eq!(running.capability_id.as_deref(), Some("codependency_query"));
        assert_eq!(running.release.as_deref(), Some("25Q2"));
        let bare = resume_bridge_checkpoint(running.clone(), ResumeAction::ContinueBare, env);
        match bare {
            ResumeOutcome::Resumed(next) => {
                assert_eq!(next.capability_id, running.capability_id);
                assert_eq!(next.release, running.release);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn bare_continue_cannot_switch_tools_when_catalog_would_prefer_another() {
        let (mut checkpoint, catalog, tools, policy, digests) = gap_checkpoint();
        checkpoint.intent.relation = "related".into();
        let env = env(&catalog, &tools, &policy, &digests);
        let outcome = resume_bridge_checkpoint(checkpoint, ResumeAction::ContinueBare, env);
        assert!(matches!(outcome, ResumeOutcome::Rejected { .. }));
    }

    #[test]
    fn clarifying_lineage_does_not_overwrite_relation_or_gene_role() {
        let catalog = catalog();
        let tools = tools();
        let policy = PlannerHostPolicy {
            provider_available: true,
            compute_authorized: true,
            coverage: CoverageSignal::NotInspected,
            allowed_capability_ids: None,
        };
        let mut intent = retrieve_intent("related");
        intent.entities.clear();
        let outcome = plan_scientific_intent(intent, &catalog, &tools, &policy);
        let mut checkpoint = checkpoint_for_outcome(
            &outcome,
            "project-a",
            "session-a",
            "op-clarify",
            "1.0.0",
            digests(),
        )
        .unwrap();
        checkpoint.intent.relation = "codependency".into();
        checkpoint.intent.entities = vec![entity("gene", "gene", "GENEA")];
        checkpoint.unresolved_fields = vec!["entity.lineage".into()];
        checkpoint.intent.scope = IntentScope::Lineage;
        let digests = digests();
        let env = env(&catalog, &tools, &policy, &digests);
        let resumed = match resume_bridge_checkpoint(
            checkpoint,
            ResumeAction::Clarify {
                field: "entity.lineage".into(),
                value: json!("liver"),
            },
            env,
        ) {
            ResumeOutcome::Resumed(checkpoint) => checkpoint,
            other => panic!("{other:?}"),
        };
        assert_eq!(resumed.intent.relation, "codependency");
        assert_eq!(resumed.intent.entity("gene").unwrap().identifier, "GENEA");
        assert_eq!(
            resumed.intent.entity("lineage").unwrap().identifier,
            "liver"
        );
    }

    #[test]
    fn digest_change_blocks_resume_before_dispatch() {
        let (checkpoint, catalog, tools, policy, mut digests) = gap_checkpoint();
        digests.mcp_schema = "schema-2".into();
        let env = env(&catalog, &tools, &policy, &digests);
        let outcome = resume_bridge_checkpoint(checkpoint, ResumeAction::ContinueBare, env);
        match outcome {
            ResumeOutcome::ResumeContractChanged { reason } => {
                assert!(reason.contains("resume_contract_changed"), "{reason}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn approval_replay_in_another_project_is_rejected() {
        let (checkpoint, catalog, tools, policy, digests) = gap_checkpoint();
        let env = env(&catalog, &tools, &policy, &digests);
        let proposed = match resume_bridge_checkpoint(
            checkpoint,
            ResumeAction::ProposeAnalysis {
                operation_id: "propose-1".into(),
            },
            env.clone(),
        ) {
            ResumeOutcome::Resumed(checkpoint) => checkpoint,
            other => panic!("{other:?}"),
        };
        let outcome = resume_bridge_checkpoint(
            proposed.clone(),
            ResumeAction::Approve {
                proposal_id: proposed.proposal_id.clone().unwrap(),
                project_id: "other-project".into(),
                session_id: "session-a".into(),
            },
            env,
        );
        assert!(matches!(outcome, ResumeOutcome::Rejected { .. }));
    }

    #[test]
    fn process_restart_restores_pending_decision_from_memory_map() {
        let (checkpoint, _, _, _, _) = gap_checkpoint();
        let mut store = BTreeMap::new();
        persist_checkpoint(&mut store, checkpoint.clone());
        persist_checkpoint(&mut store, checkpoint.clone());
        let restored = store.get("op-gap").cloned().unwrap();
        assert_eq!(restored.decision.kind(), "coverage_gap");
        assert_eq!(restored.checkpoint_id, checkpoint.checkpoint_id);
    }

    #[test]
    fn cancellation_and_uncertain_run_are_distinct_from_success() {
        let (checkpoint, catalog, tools, policy, digests) = gap_checkpoint();
        let env = env(&catalog, &tools, &policy, &digests);
        let cancelled =
            match resume_bridge_checkpoint(checkpoint.clone(), ResumeAction::Cancel, env.clone()) {
                ResumeOutcome::Resumed(checkpoint) => checkpoint,
                other => panic!("{other:?}"),
            };
        assert_eq!(cancelled.state, CheckpointState::Cancelled);
        let proposed = match resume_bridge_checkpoint(
            checkpoint,
            ResumeAction::ProposeAnalysis {
                operation_id: "propose-1".into(),
            },
            env.clone(),
        ) {
            ResumeOutcome::Resumed(checkpoint) => checkpoint,
            other => panic!("{other:?}"),
        };
        let approved = match resume_bridge_checkpoint(
            proposed.clone(),
            ResumeAction::Approve {
                proposal_id: proposed.proposal_id.clone().unwrap(),
                project_id: "project-a".into(),
                session_id: "session-a".into(),
            },
            env.clone(),
        ) {
            ResumeOutcome::Resumed(checkpoint) => checkpoint,
            other => panic!("{other:?}"),
        };
        let running = match resume_bridge_checkpoint(
            approved,
            ResumeAction::SubmitRun {
                operation_id: "run-1".into(),
                run_id: "run-row".into(),
            },
            env.clone(),
        ) {
            ResumeOutcome::Resumed(checkpoint) => checkpoint,
            other => panic!("{other:?}"),
        };
        let uncertain = match resume_bridge_checkpoint(
            running,
            ResumeAction::ReconcileExternal {
                succeeded: false,
                operation_id: "reconcile-1".into(),
            },
            env.clone(),
        ) {
            ResumeOutcome::Resumed(checkpoint) => checkpoint,
            other => panic!("{other:?}"),
        };
        assert_eq!(uncertain.state, CheckpointState::UncertainExternal);
        let retry = resume_bridge_checkpoint(uncertain, ResumeAction::RetryUncertain, env);
        assert!(matches!(retry, ResumeOutcome::Rejected { .. }));
    }

    #[test]
    fn incompatible_schema_is_a_typed_contract_change() {
        let (mut checkpoint, catalog, tools, policy, digests) = gap_checkpoint();
        checkpoint.schema_version = 0;
        let env = env(&catalog, &tools, &policy, &digests);
        assert!(matches!(
            resume_bridge_checkpoint(checkpoint, ResumeAction::ContinueBare, env),
            ResumeOutcome::ResumeContractChanged { .. }
        ));
    }
}

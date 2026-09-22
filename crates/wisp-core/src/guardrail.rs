//! Composable Agent guardrail lifecycle.
//!
//! Host stages run in a fixed order. A later stage cannot turn a denial into
//! an allow, reuse a stale approval, or execute before required approval.

use crate::observability::SpanGuard;
use crate::scientific_intent::validate_discovered_schema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const GUARDRAIL_CONTRACT_ID: &str = "wisp.agent-guardrail.v1";
pub const GUARDRAIL_CONTRACT_VERSION: &str = "1.0.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardrailStage {
    Input,
    ModelAction,
    ToolInput,
    ToolOutput,
    Handoff,
    FinalOutput,
}

impl GuardrailStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::ModelAction => "model_action",
            Self::ToolInput => "tool_input",
            Self::ToolOutput => "tool_output",
            Self::Handoff => "handoff",
            Self::FinalOutput => "final_output",
        }
    }

    fn order(self) -> u8 {
        match self {
            Self::Input => 0,
            Self::ModelAction => 1,
            Self::ToolInput => 2,
            Self::ToolOutput => 3,
            Self::Handoff => 4,
            Self::FinalOutput => 5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardrailSeverity {
    Recoverable,
    Terminal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GuardrailDecision {
    Allow,
    Transform {
        reason: String,
        value: Value,
    },
    RequestApproval {
        reason: String,
    },
    Reject {
        severity: GuardrailSeverity,
        reason: String,
    },
}

impl GuardrailDecision {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Transform { .. } => "transform",
            Self::RequestApproval { .. } => "request_approval",
            Self::Reject {
                severity: GuardrailSeverity::Recoverable,
                ..
            } => "recoverable_reject",
            Self::Reject {
                severity: GuardrailSeverity::Terminal,
                ..
            } => "terminal_reject",
        }
    }

    pub fn is_denial(&self) -> bool {
        matches!(self, Self::Reject { .. } | Self::RequestApproval { .. })
    }

    pub fn rank_for_merge(&self) -> u8 {
        self.rank()
    }

    fn rank(&self) -> u8 {
        match self {
            Self::Allow => 0,
            Self::Transform { .. } => 1,
            Self::RequestApproval { .. } => 2,
            Self::Reject {
                severity: GuardrailSeverity::Recoverable,
                ..
            } => 3,
            Self::Reject {
                severity: GuardrailSeverity::Terminal,
                ..
            } => 4,
        }
    }

    fn merge(self, next: Self) -> Self {
        if next.rank() > self.rank() {
            next
        } else {
            self
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GuardrailRecord {
    pub id: String,
    pub version: String,
    pub stage: GuardrailStage,
    pub outcome: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GuardrailOutcome {
    pub contract: String,
    pub contract_version: String,
    pub stage: GuardrailStage,
    pub decision: GuardrailDecision,
    pub records: Vec<GuardrailRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchPath {
    Direct,
    DeferredMcp,
    Delegated,
    Resumed,
}

#[derive(Debug, Clone)]
pub struct GuardrailContext {
    pub path: DispatchPath,
    pub tool: String,
    pub arguments: Value,
    pub schema: Option<Value>,
    pub allowed_tools: Option<Vec<String>>,
    pub approval_required: bool,
    pub approval_granted: bool,
    pub stale_approval: bool,
    pub output: Option<Value>,
    pub output_contract: Option<Value>,
}

pub trait Guardrail: Send + Sync {
    fn id(&self) -> &'static str;
    fn version(&self) -> &'static str {
        GUARDRAIL_CONTRACT_VERSION
    }
    fn stage(&self) -> GuardrailStage;
    fn evaluate(&self, ctx: &GuardrailContext) -> GuardrailDecision;
}

pub struct GuardrailChain {
    rails: Vec<Box<dyn Guardrail>>,
}

impl GuardrailChain {
    pub fn production() -> Self {
        Self {
            rails: vec![
                Box::new(RouteAllowlistRail),
                Box::new(ApprovalPolicyRail),
                Box::new(ToolInputSchemaRail),
                Box::new(FinalOutputContractRail),
            ],
        }
    }

    pub fn with_extra(mut self, rail: Box<dyn Guardrail>) -> Self {
        self.rails.push(rail);
        self
    }

    pub fn evaluate(
        &self,
        stage: GuardrailStage,
        ctx: &GuardrailContext,
        span: Option<&SpanGuard>,
    ) -> GuardrailOutcome {
        let mut rails: Vec<_> = self
            .rails
            .iter()
            .filter(|rail| rail.stage() == stage)
            .collect();
        rails.sort_by_key(|rail| rail.stage().order());
        let mut decision = GuardrailDecision::Allow;
        let mut records = Vec::new();
        for rail in rails {
            let next = rail.evaluate(ctx);
            records.push(GuardrailRecord {
                id: rail.id().into(),
                version: rail.version().into(),
                stage,
                outcome: next.as_str().into(),
            });
            if decision.is_denial() && matches!(next, GuardrailDecision::Allow) {
                continue;
            }
            decision = decision.merge(next);
            if matches!(
                decision,
                GuardrailDecision::Reject {
                    severity: GuardrailSeverity::Terminal,
                    ..
                }
            ) {
                break;
            }
        }
        if let Some((id, version)) = merged_span_identity(&records, &decision) {
            record_span(span, id, version, stage, &decision);
        }
        GuardrailOutcome {
            contract: GUARDRAIL_CONTRACT_ID.into(),
            contract_version: GUARDRAIL_CONTRACT_VERSION.into(),
            stage,
            decision,
            records,
        }
    }
}

fn merged_span_identity<'a>(
    records: &'a [GuardrailRecord],
    decision: &GuardrailDecision,
) -> Option<(&'a str, &'a str)> {
    let outcome = decision.as_str();
    records
        .iter()
        .rev()
        .find(|record| record.outcome == outcome)
        .or(records.last())
        .map(|record| (record.id.as_str(), record.version.as_str()))
}

fn record_span(
    span: Option<&SpanGuard>,
    id: &str,
    version: &str,
    stage: GuardrailStage,
    decision: &GuardrailDecision,
) {
    let Some(span) = span else {
        return;
    };
    span.set_str("guardrail_id", id);
    span.set_str("guardrail_version", version);
    span.set_str("guardrail_stage", stage.as_str());
    span.set_str("guardrail_outcome", decision.as_str());
}

struct RouteAllowlistRail;
impl Guardrail for RouteAllowlistRail {
    fn id(&self) -> &'static str {
        "route_allowlist"
    }
    fn stage(&self) -> GuardrailStage {
        GuardrailStage::ToolInput
    }
    fn evaluate(&self, ctx: &GuardrailContext) -> GuardrailDecision {
        let Some(allowed) = &ctx.allowed_tools else {
            return GuardrailDecision::Allow;
        };
        if allowed.iter().any(|pattern| {
            pattern == &ctx.tool
                || pattern
                    .strip_suffix('*')
                    .is_some_and(|prefix| ctx.tool.starts_with(prefix))
        }) {
            GuardrailDecision::Allow
        } else {
            GuardrailDecision::Reject {
                severity: GuardrailSeverity::Terminal,
                reason: format!("tool '{}' is blocked by the active turn route", ctx.tool),
            }
        }
    }
}

struct ApprovalPolicyRail;
impl Guardrail for ApprovalPolicyRail {
    fn id(&self) -> &'static str {
        "approval_policy"
    }
    fn stage(&self) -> GuardrailStage {
        GuardrailStage::ToolInput
    }
    fn evaluate(&self, ctx: &GuardrailContext) -> GuardrailDecision {
        if ctx.stale_approval {
            return GuardrailDecision::Reject {
                severity: GuardrailSeverity::Terminal,
                reason: "stale approval cannot be reused".into(),
            };
        }
        if ctx.approval_required && !ctx.approval_granted {
            return GuardrailDecision::RequestApproval {
                reason: format!("tool '{}' requires host approval before dispatch", ctx.tool),
            };
        }
        GuardrailDecision::Allow
    }
}

struct ToolInputSchemaRail;
impl Guardrail for ToolInputSchemaRail {
    fn id(&self) -> &'static str {
        "tool_input_schema"
    }
    fn stage(&self) -> GuardrailStage {
        GuardrailStage::ToolInput
    }
    fn evaluate(&self, ctx: &GuardrailContext) -> GuardrailDecision {
        if matches!(ctx.path, DispatchPath::DeferredMcp) && ctx.schema.is_none() {
            return GuardrailDecision::Reject {
                severity: GuardrailSeverity::Recoverable,
                reason: format!(
                    "deferred MCP tool '{}' has no discovered input schema",
                    ctx.tool
                ),
            };
        }
        let Some(schema) = &ctx.schema else {
            return GuardrailDecision::Allow;
        };
        match validate_discovered_schema(schema, &ctx.arguments) {
            Ok(()) => GuardrailDecision::Allow,
            Err(reason) => GuardrailDecision::Reject {
                severity: GuardrailSeverity::Recoverable,
                reason,
            },
        }
    }
}

struct FinalOutputContractRail;
impl Guardrail for FinalOutputContractRail {
    fn id(&self) -> &'static str {
        "final_output_contract"
    }
    fn stage(&self) -> GuardrailStage {
        GuardrailStage::FinalOutput
    }
    fn evaluate(&self, ctx: &GuardrailContext) -> GuardrailDecision {
        let (Some(contract), Some(output)) = (&ctx.output_contract, &ctx.output) else {
            return GuardrailDecision::Allow;
        };
        match validate_discovered_schema(contract, output) {
            Ok(()) => GuardrailDecision::Allow,
            Err(reason) => GuardrailDecision::Reject {
                severity: GuardrailSeverity::Terminal,
                reason: format!("final output failed the typed contract: {reason}"),
            },
        }
    }
}

/// Shared host entry used by direct, deferred MCP, delegated, and resumed paths.
pub fn evaluate_tool_input(
    chain: &GuardrailChain,
    ctx: GuardrailContext,
    span: Option<&SpanGuard>,
) -> GuardrailOutcome {
    chain.evaluate(GuardrailStage::ToolInput, &ctx, span)
}

pub fn evaluate_final_output(
    chain: &GuardrailChain,
    ctx: GuardrailContext,
    span: Option<&SpanGuard>,
) -> GuardrailOutcome {
    chain.evaluate(GuardrailStage::FinalOutput, &ctx, span)
}

pub fn evaluate_handoff(
    chain: &GuardrailChain,
    ctx: GuardrailContext,
    span: Option<&SpanGuard>,
) -> GuardrailOutcome {
    chain.evaluate(GuardrailStage::Handoff, &ctx, span)
}

pub fn default_tool_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "query": {"type": "string"}
        },
        "required": ["query"],
        "additionalProperties": false
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observability::{AgentTrace, SpanKind, SpanStatus, TurnIdentity};

    fn invalid_call(path: DispatchPath) -> GuardrailContext {
        GuardrailContext {
            path,
            tool: "fixture_mcp_query".into(),
            arguments: serde_json::json!({"invented": true}),
            schema: Some(default_tool_schema()),
            allowed_tools: None,
            approval_required: false,
            approval_granted: false,
            stale_approval: false,
            output: None,
            output_contract: None,
        }
    }

    #[test]
    fn invalid_tool_call_is_rejected_on_every_dispatch_path() {
        let chain = GuardrailChain::production();
        for path in [
            DispatchPath::Direct,
            DispatchPath::DeferredMcp,
            DispatchPath::Delegated,
            DispatchPath::Resumed,
        ] {
            let outcome = evaluate_tool_input(&chain, invalid_call(path), None);
            assert_eq!(outcome.contract, GUARDRAIL_CONTRACT_ID);
            match outcome.decision {
                GuardrailDecision::Reject {
                    severity: GuardrailSeverity::Recoverable,
                    reason,
                } => assert!(
                    reason.contains("unexpected") || reason.contains("missing"),
                    "{reason}"
                ),
                other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn later_middleware_cannot_downgrade_a_denial() {
        struct AllowAll;
        impl Guardrail for AllowAll {
            fn id(&self) -> &'static str {
                "allow_all"
            }
            fn stage(&self) -> GuardrailStage {
                GuardrailStage::ToolInput
            }
            fn evaluate(&self, _ctx: &GuardrailContext) -> GuardrailDecision {
                GuardrailDecision::Allow
            }
        }
        let chain = GuardrailChain::production().with_extra(Box::new(AllowAll));
        let outcome = evaluate_tool_input(&chain, invalid_call(DispatchPath::Direct), None);
        assert!(matches!(
            outcome.decision,
            GuardrailDecision::Reject {
                severity: GuardrailSeverity::Recoverable,
                ..
            }
        ));
        assert!(outcome
            .records
            .iter()
            .any(|record| record.id == "allow_all" && record.outcome == "allow"));
    }

    #[test]
    fn recoverable_schema_failure_does_not_become_terminal() {
        let outcome = evaluate_tool_input(
            &GuardrailChain::production(),
            invalid_call(DispatchPath::Direct),
            None,
        );
        assert_eq!(outcome.decision.as_str(), "recoverable_reject");
    }

    #[test]
    fn output_guardrail_rejects_unsupported_structured_claims() {
        let outcome = evaluate_final_output(
            &GuardrailChain::production(),
            GuardrailContext {
                path: DispatchPath::Direct,
                tool: String::new(),
                arguments: Value::Null,
                schema: None,
                allowed_tools: None,
                approval_required: false,
                approval_granted: false,
                stale_approval: false,
                output: Some(serde_json::json!({"status":"FOUND","secret":"row"})),
                output_contract: Some(serde_json::json!({
                    "type": "object",
                    "properties": {"status": {"type": "string"}},
                    "required": ["status"],
                    "additionalProperties": false
                })),
            },
            None,
        );
        match outcome.decision {
            GuardrailDecision::Reject {
                severity: GuardrailSeverity::Terminal,
                reason,
            } => assert!(reason.contains("final output"), "{reason}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn stale_approval_cannot_authorize_dispatch() {
        let mut ctx = invalid_call(DispatchPath::Direct);
        ctx.arguments = serde_json::json!({"query": "ok"});
        ctx.approval_required = true;
        ctx.approval_granted = true;
        ctx.stale_approval = true;
        let outcome = evaluate_tool_input(&GuardrailChain::production(), ctx, None);
        assert!(matches!(
            outcome.decision,
            GuardrailDecision::Reject {
                severity: GuardrailSeverity::Terminal,
                ..
            }
        ));
    }

    #[test]
    fn guardrail_outcome_is_traced_without_payloads() {
        let trace = AgentTrace::in_memory();
        let turn = trace.start_turn(TurnIdentity::default());
        let span = turn.child(SpanKind::Tool, "agent.tool");
        let _ = evaluate_tool_input(
            &GuardrailChain::production(),
            invalid_call(DispatchPath::Direct),
            Some(&span),
        );
        span.end(SpanStatus::Error);
        turn.end(SpanStatus::Error);
        let encoded = trace.memory().unwrap().document().encoded();
        assert!(encoded.contains("tool_input_schema"));
        assert!(encoded.contains("recoverable_reject"));
        assert!(!encoded.contains("invented"));
    }

    #[test]
    fn deferred_mcp_validates_nested_tool_input_not_the_gateway() {
        let schema = default_tool_schema();
        let nested = GuardrailContext {
            path: DispatchPath::DeferredMcp,
            tool: "fixture_mcp_query".into(),
            arguments: serde_json::json!({"invented": true}),
            schema: Some(schema.clone()),
            allowed_tools: None,
            approval_required: false,
            approval_granted: false,
            stale_approval: false,
            output: None,
            output_contract: None,
        };
        let outcome = evaluate_tool_input(&GuardrailChain::production(), nested, None);
        assert_eq!(outcome.decision.as_str(), "recoverable_reject");

        let missing = GuardrailContext {
            path: DispatchPath::DeferredMcp,
            tool: "missing_connector".into(),
            arguments: serde_json::json!({"query": "ok"}),
            schema: None,
            allowed_tools: None,
            approval_required: false,
            approval_granted: false,
            stale_approval: false,
            output: None,
            output_contract: None,
        };
        let missing_outcome = evaluate_tool_input(&GuardrailChain::production(), missing, None);
        match missing_outcome.decision {
            GuardrailDecision::Reject {
                severity: GuardrailSeverity::Recoverable,
                reason,
            } => assert!(reason.contains("discovered input schema"), "{reason}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn merged_span_keeps_the_denial_after_a_later_allow() {
        struct AllowAll;
        impl Guardrail for AllowAll {
            fn id(&self) -> &'static str {
                "allow_all"
            }
            fn stage(&self) -> GuardrailStage {
                GuardrailStage::ToolInput
            }
            fn evaluate(&self, _ctx: &GuardrailContext) -> GuardrailDecision {
                GuardrailDecision::Allow
            }
        }
        let trace = AgentTrace::in_memory();
        let turn = trace.start_turn(TurnIdentity::default());
        let span = turn.child(SpanKind::Tool, "agent.tool");
        let chain = GuardrailChain::production().with_extra(Box::new(AllowAll));
        let _ = evaluate_tool_input(&chain, invalid_call(DispatchPath::Direct), Some(&span));
        span.end(SpanStatus::Error);
        turn.end(SpanStatus::Error);
        let encoded = trace.memory().unwrap().document().encoded();
        assert!(encoded.contains("recoverable_reject"));
        assert!(!encoded.contains("\"guardrail_outcome\":\"allow\""));
    }
}

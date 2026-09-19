//! Non-exfiltrating remote-compute gateway for DepMap new analysis.
//!
//! Tests inject fakes. Production never opens a live SSH session from this
//! module; missing knowledge context is a typed blocked status.

use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComputeKind {
    RebuildRankings,
    FillMissingColumn,
    RecomputeTfActivity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayDecision {
    pub state: &'static str,
    pub code: &'static str,
    pub evidence_status: Option<&'static str>,
    pub gated_run: bool,
    pub ssh_used: bool,
    pub matrix_export: bool,
}

impl GatewayDecision {
    pub fn to_json(&self) -> Value {
        json!({
            "state": self.state,
            "code": self.code,
            "status": self.evidence_status,
            "gated_run": self.gated_run,
            "ssh_used": self.ssh_used,
            "matrix_export": self.matrix_export,
            "new_analysis_started": false,
            "exfiltrates": false
        })
    }
}

pub trait RemoteComputeGateway {
    fn admit(
        &self,
        kind: ComputeKind,
        knowledge_context_ready: bool,
        mcp_connected: bool,
    ) -> GatewayDecision;
}

/// Default gateway: never SSH, never export matrices, never guess folders.
#[derive(Debug, Default)]
pub struct NonExfiltratingGateway;

impl RemoteComputeGateway for NonExfiltratingGateway {
    fn admit(
        &self,
        _kind: ComputeKind,
        knowledge_context_ready: bool,
        mcp_connected: bool,
    ) -> GatewayDecision {
        if !knowledge_context_ready {
            return GatewayDecision {
                state: "blocked",
                code: "configuration_blocked",
                evidence_status: Some("MODULE_UNAVAILABLE"),
                gated_run: false,
                ssh_used: false,
                matrix_export: false,
            };
        }
        if !mcp_connected {
            return GatewayDecision {
                state: "blocked",
                code: "mcp_unavailable",
                evidence_status: Some("MODULE_UNAVAILABLE"),
                gated_run: false,
                ssh_used: false,
                matrix_export: false,
            };
        }
        GatewayDecision {
            state: "gated",
            code: "approval_required_run",
            evidence_status: Some("NOT_COMPUTED"),
            gated_run: true,
            ssh_used: false,
            matrix_export: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeGateway {
        ready: bool,
        mcp: bool,
    }

    impl RemoteComputeGateway for FakeGateway {
        fn admit(
            &self,
            kind: ComputeKind,
            knowledge_context_ready: bool,
            mcp_connected: bool,
        ) -> GatewayDecision {
            NonExfiltratingGateway.admit(
                kind,
                knowledge_context_ready && self.ready,
                mcp_connected && self.mcp,
            )
        }
    }

    #[test]
    fn missing_knowledge_context_is_typed_blocked_not_folder_guessing() {
        let gateway = FakeGateway {
            ready: false,
            mcp: true,
        };
        let decision = gateway.admit(ComputeKind::RebuildRankings, true, true);
        assert_eq!(decision.code, "configuration_blocked");
        assert_eq!(decision.evidence_status, Some("MODULE_UNAVAILABLE"));
        assert!(!decision.ssh_used);
        assert!(!decision.gated_run);
        let payload = decision.to_json();
        assert_eq!(payload["exfiltrates"], false);
        assert_eq!(payload["new_analysis_started"], false);
    }

    #[test]
    fn mcp_dropout_is_module_unavailable() {
        let gateway = FakeGateway {
            ready: true,
            mcp: false,
        };
        let decision = gateway.admit(ComputeKind::RecomputeTfActivity, true, true);
        assert_eq!(decision.code, "mcp_unavailable");
        assert_eq!(decision.evidence_status, Some("MODULE_UNAVAILABLE"));
        assert!(!decision.ssh_used);
    }

    #[test]
    fn ready_context_admits_a_gated_run_without_ssh_or_export() {
        let gateway = FakeGateway {
            ready: true,
            mcp: true,
        };
        let decision = gateway.admit(ComputeKind::FillMissingColumn, true, true);
        assert!(decision.gated_run);
        assert!(!decision.ssh_used);
        assert!(!decision.matrix_export);
        assert_eq!(decision.evidence_status, Some("NOT_COMPUTED"));
    }
}

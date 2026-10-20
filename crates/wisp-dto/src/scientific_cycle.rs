//! Scientific-cycle v1 records. Data only: kinds, statuses, typed references,
//! and graph checks. Persistence lives in `wisp-store`.

use serde::{Deserialize, Serialize};

pub const SCIENTIFIC_CYCLE_SCHEMA: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScientificNodeKind {
    ResearchQuestion,
    HypothesisCandidate,
    AnalysisPlan,
    Observation,
    ConsensusFinding,
    Decision,
    FollowUpHypothesis,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScientificNodeStatus {
    Proposed,
    Active,
    Measured,
    Rejected,
    Failed,
    Inconclusive,
    #[serde(rename = "null")]
    Null,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScientificRefKind {
    Evidence,
    Run,
    Artifact,
    Paper,
    Decision,
    Node,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScientificEdgeKind {
    Follows,
    Supports,
    Rejects,
    Derives,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScientificRef {
    pub kind: ScientificRefKind,
    pub id: String,
    pub version: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScientificNode {
    pub id: String,
    pub cycle_id: String,
    pub project_id: String,
    pub frame_id: String,
    pub kind: ScientificNodeKind,
    pub status: ScientificNodeStatus,
    pub payload_json: String,
    pub refs: Vec<ScientificRef>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScientificEdge {
    pub id: String,
    pub cycle_id: String,
    pub from_id: String,
    pub to_id: String,
    pub kind: ScientificEdgeKind,
    pub created_at: i64,
}

impl ScientificNodeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ResearchQuestion => "research_question",
            Self::HypothesisCandidate => "hypothesis_candidate",
            Self::AnalysisPlan => "analysis_plan",
            Self::Observation => "observation",
            Self::ConsensusFinding => "consensus_finding",
            Self::Decision => "decision",
            Self::FollowUpHypothesis => "follow_up_hypothesis",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "research_question" => Self::ResearchQuestion,
            "hypothesis_candidate" => Self::HypothesisCandidate,
            "analysis_plan" => Self::AnalysisPlan,
            "observation" => Self::Observation,
            "consensus_finding" => Self::ConsensusFinding,
            "decision" => Self::Decision,
            "follow_up_hypothesis" => Self::FollowUpHypothesis,
            _ => return None,
        })
    }

    pub fn is_hypothesis(self) -> bool {
        matches!(self, Self::HypothesisCandidate | Self::FollowUpHypothesis)
    }
}

impl ScientificNodeStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Active => "active",
            Self::Measured => "measured",
            Self::Rejected => "rejected",
            Self::Failed => "failed",
            Self::Inconclusive => "inconclusive",
            Self::Null => "null",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "proposed" => Self::Proposed,
            "active" => Self::Active,
            "measured" => Self::Measured,
            "rejected" => Self::Rejected,
            "failed" => Self::Failed,
            "inconclusive" => Self::Inconclusive,
            "null" => Self::Null,
            _ => return None,
        })
    }
}

impl ScientificRefKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Evidence => "evidence",
            Self::Run => "run",
            Self::Artifact => "artifact",
            Self::Paper => "paper",
            Self::Decision => "decision",
            Self::Node => "node",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "evidence" => Self::Evidence,
            "run" => Self::Run,
            "artifact" => Self::Artifact,
            "paper" => Self::Paper,
            "decision" => Self::Decision,
            "node" => Self::Node,
            _ => return None,
        })
    }
}

impl ScientificEdgeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Follows => "follows",
            Self::Supports => "supports",
            Self::Rejects => "rejects",
            Self::Derives => "derives",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "follows" => Self::Follows,
            "supports" => Self::Supports,
            "rejects" => Self::Rejects,
            "derives" => Self::Derives,
            _ => return None,
        })
    }
}

pub fn status_transition_allowed(from: ScientificNodeStatus, to: ScientificNodeStatus) -> bool {
    if from == to {
        return true;
    }
    matches!(
        (from, to),
        (
            ScientificNodeStatus::Proposed,
            ScientificNodeStatus::Active
                | ScientificNodeStatus::Rejected
                | ScientificNodeStatus::Failed
                | ScientificNodeStatus::Null
        ) | (
            ScientificNodeStatus::Active,
            ScientificNodeStatus::Measured
                | ScientificNodeStatus::Rejected
                | ScientificNodeStatus::Failed
                | ScientificNodeStatus::Inconclusive
                | ScientificNodeStatus::Null
        ) | (
            ScientificNodeStatus::Measured,
            ScientificNodeStatus::Inconclusive | ScientificNodeStatus::Rejected
        )
    )
}

/// Reject a node that would store a measured finding as an unverified hypothesis,
/// or an observation that does not name an immutable evidence, run, or artifact version.
pub fn validate_scientific_node(node: &ScientificNode) -> Result<(), String> {
    if node.id.trim().is_empty() || node.cycle_id.trim().is_empty() {
        return Err("scientific node id and cycle id are required".into());
    }
    if node.kind.is_hypothesis() && node.status == ScientificNodeStatus::Measured {
        return Err("a hypothesis cannot be marked measured".into());
    }
    if node.kind.is_hypothesis() {
        let value = serde_json::from_str::<serde_json::Value>(&node.payload_json)
            .map_err(|_| "scientific payload must be JSON".to_string())?;
        if value.get("measured_value").is_some() {
            return Err("a hypothesis payload cannot carry a measured value".into());
        }
    } else if serde_json::from_str::<serde_json::Value>(&node.payload_json).is_err() {
        return Err("scientific payload must be JSON".into());
    }
    if node.kind == ScientificNodeKind::Observation {
        let grounded = node.refs.iter().any(|reference| {
            !reference.version.trim().is_empty()
                && matches!(
                    reference.kind,
                    ScientificRefKind::Evidence
                        | ScientificRefKind::Run
                        | ScientificRefKind::Artifact
                )
        });
        if !grounded {
            return Err(
                "an observation requires an evidence, run, or artifact reference with a version"
                    .into(),
            );
        }
    }
    for reference in &node.refs {
        if reference.id.trim().is_empty() || reference.version.trim().is_empty() {
            return Err("a scientific reference needs an id and an immutable version".into());
        }
    }
    Ok(())
}

/// Record one already-returned MCP evidence identity. The observation stays
/// proposed: this does not approve analysis or start another query.
pub fn record_mcp_observation(
    cycle_id: &str,
    project_id: &str,
    frame_id: &str,
    node_id: &str,
    tool: &str,
    evidence_id: &str,
    contract_version: &str,
    recorded_at: i64,
) -> Result<ScientificNode, String> {
    if tool.trim().is_empty() || evidence_id.trim().is_empty() || contract_version.trim().is_empty()
    {
        return Err("an MCP observation needs a tool, evidence id, and contract version".into());
    }
    let payload = serde_json::json!({
        "tool": tool.trim(),
        "awaiting_approval": true,
    });
    let node = ScientificNode {
        id: node_id.trim().to_string(),
        cycle_id: cycle_id.trim().to_string(),
        project_id: project_id.trim().to_string(),
        frame_id: frame_id.trim().to_string(),
        kind: ScientificNodeKind::Observation,
        status: ScientificNodeStatus::Proposed,
        payload_json: payload.to_string(),
        refs: vec![ScientificRef {
            kind: ScientificRefKind::Evidence,
            id: evidence_id.trim().to_string(),
            version: contract_version.trim().to_string(),
        }],
        created_at: recorded_at,
        updated_at: recorded_at,
    };
    validate_scientific_node(&node)?;
    Ok(node)
}

pub fn validate_scientific_edges(edges: &[ScientificEdge]) -> Result<(), String> {
    let mut adjacency: Vec<(&str, &str)> = Vec::new();
    for edge in edges {
        if edge.from_id == edge.to_id {
            return Err("a scientific edge cannot link a node to itself".into());
        }
        if edge.from_id.trim().is_empty() || edge.to_id.trim().is_empty() {
            return Err("a scientific edge needs both endpoints".into());
        }
        adjacency.push((edge.from_id.as_str(), edge.to_id.as_str()));
    }
    if has_directed_cycle(&adjacency) {
        return Err("scientific cycle edges must stay acyclic".into());
    }
    Ok(())
}

fn has_directed_cycle(edges: &[(&str, &str)]) -> bool {
    let mut nodes = Vec::new();
    for (from, to) in edges {
        if !nodes.contains(from) {
            nodes.push(*from);
        }
        if !nodes.contains(to) {
            nodes.push(*to);
        }
    }
    let mut color = vec![0u8; nodes.len()];
    fn visit(node: &str, nodes: &[&str], edges: &[(&str, &str)], color: &mut [u8]) -> bool {
        let Some(index) = nodes.iter().position(|item| *item == node) else {
            return false;
        };
        if color[index] == 1 {
            return true;
        }
        if color[index] == 2 {
            return false;
        }
        color[index] = 1;
        for (from, to) in edges {
            if *from == node && visit(to, nodes, edges, color) {
                return true;
            }
        }
        color[index] = 2;
        false
    }
    nodes
        .iter()
        .any(|node| visit(node, &nodes, edges, &mut color))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hypothesis(payload: &str) -> ScientificNode {
        ScientificNode {
            id: "h".into(),
            cycle_id: "c".into(),
            project_id: "p".into(),
            frame_id: "f".into(),
            kind: ScientificNodeKind::HypothesisCandidate,
            status: ScientificNodeStatus::Proposed,
            payload_json: payload.into(),
            refs: vec![],
            created_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn an_mcp_observation_stays_proposed_and_cites_evidence() {
        let node = record_mcp_observation(
            "c",
            "p",
            "f",
            "obs",
            "depmap_gene_evidence",
            "ev-9",
            "13",
            4,
        )
        .unwrap();
        assert_eq!(node.status, ScientificNodeStatus::Proposed);
        assert_eq!(node.refs[0].id, "ev-9");
        assert!(
            record_mcp_observation("c", "p", "f", "obs", "depmap_gene_evidence", " ", "13", 4)
                .is_err()
        );
    }

    #[test]
    fn a_malformed_payload_is_rejected() {
        assert!(validate_scientific_node(&hypothesis("{")).is_err());
        assert!(validate_scientific_node(&hypothesis(r#"{"measured_value":1}"#)).is_err());
        assert!(validate_scientific_node(&hypothesis("{}")).is_ok());
    }
}

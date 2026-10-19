//! Local-first DepMap discovery slice. It records a reviewable workflow, not a claim of validity.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryTask {
    CollectEvidence,
    ProposeHypotheses,
    Review,
    Rank,
    HumanApproval,
    Analyze,
    Replicate,
    Consensus,
    ExportBundle,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryDag {
    pub version: String,
    pub tasks: Vec<DiscoveryTask>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimKind {
    Numeric,
    Literature,
    Hypothesis,
    Finding,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimRef {
    pub kind: ClaimKind,
    pub id: String,
    pub version: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryCase {
    pub case_id: String,
    pub held_out: bool,
    pub approved: bool,
    pub claims: Vec<ClaimRef>,
    pub includes_methods: bool,
    pub includes_nulls: bool,
    pub wet_lab: bool,
}

pub fn discovery_dag() -> DiscoveryDag {
    DiscoveryDag {
        version: "robin-lite-v1".into(),
        tasks: vec![
            DiscoveryTask::CollectEvidence,
            DiscoveryTask::ProposeHypotheses,
            DiscoveryTask::Review,
            DiscoveryTask::Rank,
            DiscoveryTask::HumanApproval,
            DiscoveryTask::Analyze,
            DiscoveryTask::Replicate,
            DiscoveryTask::Consensus,
            DiscoveryTask::ExportBundle,
        ],
    }
}

pub fn review_case(case: &DiscoveryCase) -> Result<(), String> {
    if case.wet_lab {
        return Err("wet-lab execution is outside this workflow".into());
    }
    if !case.approved {
        return Err("analysis requires a recorded human approval".into());
    }
    if !case.includes_methods || !case.includes_nulls {
        return Err("the bundle must include methods and nulls".into());
    }
    let dag = discovery_dag();
    let analyze_at = dag
        .tasks
        .iter()
        .position(|task| *task == DiscoveryTask::Analyze)
        .ok_or_else(|| "analyze task missing".to_string())?;
    let approval_at = dag
        .tasks
        .iter()
        .position(|task| *task == DiscoveryTask::HumanApproval)
        .ok_or_else(|| "approval task missing".to_string())?;
    if approval_at > analyze_at {
        return Err("approval must precede analysis".into());
    }
    for claim in &case.claims {
        if claim.id.is_empty() || claim.version.is_empty() {
            return Err(format!("{} is missing an id or version", claim.id));
        }
        if claim.kind == ClaimKind::Hypothesis
            && case
                .claims
                .iter()
                .any(|other| other.id == claim.id && other.kind == ClaimKind::Finding)
        {
            return Err(format!(
                "{} cannot be both a hypothesis and a finding",
                claim.id
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claim(kind: ClaimKind, id: &str) -> ClaimRef {
        ClaimRef {
            kind,
            id: id.into(),
            version: "v1".into(),
        }
    }

    fn ready(case_id: &str, held_out: bool) -> DiscoveryCase {
        DiscoveryCase {
            case_id: case_id.into(),
            held_out,
            approved: true,
            claims: vec![
                claim(ClaimKind::Numeric, "evidence-1"),
                claim(ClaimKind::Literature, "paper-1"),
                claim(ClaimKind::Hypothesis, "h-1"),
                claim(ClaimKind::Finding, "finding-1"),
            ],
            includes_methods: true,
            includes_nulls: true,
            wet_lab: false,
        }
    }

    #[test]
    fn approval_precedes_analysis_and_both_cases_pass() {
        let dag = discovery_dag();
        let approval = dag
            .tasks
            .iter()
            .position(|task| *task == DiscoveryTask::HumanApproval);
        let analyze = dag
            .tasks
            .iter()
            .position(|task| *task == DiscoveryTask::Analyze);
        assert!(approval < analyze);
        assert!(review_case(&ready("fixed", false)).is_ok());
        assert!(review_case(&ready("held-out", true)).is_ok());
    }

    #[test]
    fn unapproved_wet_lab_and_collapsed_hypothesis_fail() {
        let mut unapproved = ready("fixed", false);
        unapproved.approved = false;
        assert!(review_case(&unapproved).is_err());

        let mut wet = ready("fixed", false);
        wet.wet_lab = true;
        assert!(review_case(&wet).is_err());

        let mut collapsed = ready("fixed", false);
        collapsed.claims.push(claim(ClaimKind::Finding, "h-1"));
        assert!(review_case(&collapsed).is_err());
    }
}

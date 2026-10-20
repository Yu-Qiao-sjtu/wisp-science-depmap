//! Executable candidate tree for empirical software search. Held-out scores stay sealed until the final gate.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchSplit {
    Train,
    Validation,
    HeldOut,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateOutcome {
    Scored,
    Timeout,
    OutOfMemory,
    Crash,
    Partial,
    Nondeterministic,
    Invalid,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateNode {
    pub id: String,
    pub parents: Vec<String>,
    pub diff: String,
    pub split: SearchSplit,
    pub outcome: CandidateOutcome,
    pub score: Option<i64>,
    pub runtime_ms: u64,
    pub seed: u64,
    /// Local run that produced a scored outcome. Empty for records that have not been admitted.
    #[serde(default)]
    pub run_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchReplicate {
    pub id: String,
    pub seed: u64,
    pub budget: u32,
    pub nodes: Vec<CandidateNode>,
    pub frozen: bool,
    pub approved: bool,
}

pub fn admit_score(node: &CandidateNode, frozen: bool, approved: bool) -> Result<(), String> {
    if node.split == SearchSplit::HeldOut && !(frozen && approved) {
        return Err("held-out evaluation waits until search is frozen and approved".into());
    }
    if node.parents.is_empty() && node.id != "root" {
        return Err("a recombined candidate needs every parent".into());
    }
    if node.diff.is_empty() {
        return Err("a candidate needs a reviewable diff".into());
    }
    if node.outcome == CandidateOutcome::Scored && node.run_id.trim().is_empty() {
        return Err("a scored candidate needs a local run id".into());
    }
    Ok(())
}

pub fn prefer<'a>(left: &'a CandidateNode, right: &'a CandidateNode) -> &'a CandidateNode {
    let left_failed = left.outcome != CandidateOutcome::Scored;
    let right_failed = right.outcome != CandidateOutcome::Scored;
    if left_failed != right_failed {
        return if left_failed { right } else { left };
    }
    let left_score = left.score.unwrap_or(i64::MIN);
    let right_score = right.score.unwrap_or(i64::MIN);
    if left_score.abs_diff(right_score) <= 1 && left.runtime_ms != right.runtime_ms {
        return if left.runtime_ms < right.runtime_ms {
            left
        } else {
            right
        };
    }
    if left_score == right_score {
        return if left.id <= right.id { left } else { right };
    }
    if left_score > right_score {
        left
    } else {
        right
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(
        id: &str,
        split: SearchSplit,
        outcome: CandidateOutcome,
        score: i64,
        runtime: u64,
    ) -> CandidateNode {
        CandidateNode {
            id: id.into(),
            parents: vec!["root".into()],
            diff: format!("change {id}"),
            split,
            outcome,
            score: Some(score),
            runtime_ms: runtime,
            seed: 3,
            run_id: "run-1".into(),
        }
    }

    #[test]
    fn held_out_stays_sealed_and_failures_are_kept() {
        let held = node(
            "held",
            SearchSplit::HeldOut,
            CandidateOutcome::Scored,
            10,
            20,
        );
        assert!(admit_score(&held, false, true).is_err());
        assert!(admit_score(&held, true, true).is_ok());
        let timeout = node(
            "slow",
            SearchSplit::Train,
            CandidateOutcome::Timeout,
            0,
            9_000,
        );
        assert_eq!(timeout.outcome, CandidateOutcome::Timeout);
        let replicate = SearchReplicate {
            id: "r1".into(),
            seed: 3,
            budget: 2,
            nodes: vec![timeout],
            frozen: false,
            approved: false,
        };
        assert_eq!(replicate.nodes.len(), 1);
    }

    #[test]
    fn a_faster_near_tie_outranks_a_tiny_score_gain() {
        let fast = node(
            "fast",
            SearchSplit::Validation,
            CandidateOutcome::Scored,
            10,
            40,
        );
        let slow = node(
            "slow",
            SearchSplit::Validation,
            CandidateOutcome::Scored,
            11,
            400,
        );
        assert_eq!(prefer(&fast, &slow).id, "fast");
        let child = CandidateNode {
            id: "mix".into(),
            parents: vec!["fast".into(), "slow".into()],
            diff: "combine".into(),
            split: SearchSplit::Train,
            outcome: CandidateOutcome::Scored,
            score: Some(12),
            runtime_ms: 50,
            seed: 3,
            run_id: "run-mix".into(),
        };
        assert!(admit_score(&child, false, false).is_ok());
        let missing = CandidateNode {
            id: "missing".into(),
            parents: vec!["root".into()],
            diff: "none".into(),
            split: SearchSplit::Train,
            outcome: CandidateOutcome::Scored,
            score: None,
            runtime_ms: 1,
            seed: 3,
            run_id: "run-missing".into(),
        };
        let zero = node("zero", SearchSplit::Train, CandidateOutcome::Scored, 0, 5);
        assert_eq!(prefer(&missing, &zero).id, "zero");
        let mut unscored_run = zero.clone();
        unscored_run.run_id.clear();
        assert!(admit_score(&unscored_run, true, true).is_err());
    }
}

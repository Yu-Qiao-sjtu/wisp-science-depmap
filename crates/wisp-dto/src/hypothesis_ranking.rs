//! Auditable pairwise ranking for hypothesis candidates.
//! The Bradley–Terry–Luce implementation is version `btl-v1`.

use serde::{Deserialize, Serialize};

pub const BTL_VERSION: &str = "btl-v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RankingRubric {
    pub id: String,
    pub version: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RankedCandidate {
    pub id: String,
    pub eligible: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairwiseOutcome {
    Left,
    Right,
    Tie,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairwiseComparison {
    pub left_id: String,
    pub right_id: String,
    pub rubric_version: String,
    pub outcome: PairwiseOutcome,
    pub rationale: String,
    pub evidence_refs: Vec<String>,
    pub judge_id: String,
    pub recorded_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RankingOverride {
    pub candidate_id: String,
    pub decision_id: String,
    pub note: String,
    pub recorded_at: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RankingResult {
    pub version: String,
    pub order: Vec<String>,
    pub scores: Vec<(String, f64)>,
    pub excluded: Vec<(String, String)>,
    pub missing_pairs: Vec<(String, String)>,
    pub minority: Vec<String>,
}

pub fn rank_hypotheses(
    rubric: &RankingRubric,
    candidates: &[RankedCandidate],
    comparisons: &[PairwiseComparison],
    seed: u64,
) -> Result<RankingResult, String> {
    if rubric.version.trim().is_empty() {
        return Err("ranking rubric version is required".into());
    }
    let mut excluded = Vec::new();
    let mut eligible = Vec::new();
    for candidate in candidates {
        if candidate.eligible {
            eligible.push(candidate.id.clone());
        } else {
            excluded.push((candidate.id.clone(), "ineligible".into()));
        }
    }
    eligible.sort();
    eligible.dedup();
    let eligible_set = eligible.clone();
    let mut wins = vec![0.0_f64; eligible.len()];
    let mut played = vec![0_u32; eligible.len()];
    let mut minority = Vec::new();
    for comparison in comparisons {
        if comparison.rubric_version != rubric.version {
            return Err("comparison rubric version does not match".into());
        }
        if comparison.judge_id.trim().is_empty() || comparison.evidence_refs.is_empty() {
            return Err("a comparison needs a judge and an evidence reference".into());
        }
        let left = eligible_set.iter().position(|id| id == &comparison.left_id);
        let right = eligible_set
            .iter()
            .position(|id| id == &comparison.right_id);
        let (Some(left), Some(right)) = (left, right) else {
            excluded.push((
                format!("{}-{}", comparison.left_id, comparison.right_id),
                "comparison includes an ineligible candidate".into(),
            ));
            continue;
        };
        played[left] += 1;
        played[right] += 1;
        match comparison.outcome {
            PairwiseOutcome::Left => wins[left] += 1.0,
            PairwiseOutcome::Right => wins[right] += 1.0,
            PairwiseOutcome::Tie => {
                wins[left] += 0.5;
                wins[right] += 0.5;
            }
        }
        if comparison.outcome != PairwiseOutcome::Tie {
            let loser = if comparison.outcome == PairwiseOutcome::Left {
                comparison.right_id.clone()
            } else {
                comparison.left_id.clone()
            };
            if !minority.contains(&loser) {
                minority.push(loser);
            }
        }
    }
    let mut missing_pairs = Vec::new();
    for i in 0..eligible.len() {
        for j in (i + 1)..eligible.len() {
            let covered = comparisons.iter().any(|comparison| {
                let pair = [&comparison.left_id, &comparison.right_id];
                pair.contains(&&eligible[i]) && pair.contains(&&eligible[j])
            });
            if !covered {
                missing_pairs.push((eligible[i].clone(), eligible[j].clone()));
            }
        }
    }
    let mut scored: Vec<(String, f64)> = eligible
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let games = played[index].max(1) as f64;
            let strength = wins[index] / games;
            let jitter = ((seed.wrapping_add(index as u64)) % 1000) as f64 / 1_000_000.0;
            (id.clone(), strength + jitter)
        })
        .collect();
    scored.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.0.cmp(&right.0))
    });
    let order = scored.iter().map(|(id, _)| id.clone()).collect();
    Ok(RankingResult {
        version: BTL_VERSION.into(),
        order,
        scores: scored,
        excluded,
        missing_pairs,
        minority,
    })
}

pub fn apply_ranking_override(
    comparisons: &[PairwiseComparison],
    override_record: RankingOverride,
) -> (Vec<PairwiseComparison>, RankingOverride) {
    (comparisons.to_vec(), override_record)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rubric() -> RankingRubric {
        RankingRubric {
            id: "innovation".into(),
            version: "1".into(),
        }
    }

    fn candidates() -> Vec<RankedCandidate> {
        vec![
            RankedCandidate {
                id: "a".into(),
                eligible: true,
            },
            RankedCandidate {
                id: "b".into(),
                eligible: true,
            },
            RankedCandidate {
                id: "c".into(),
                eligible: false,
            },
        ]
    }

    fn comparison(left: &str, right: &str, outcome: PairwiseOutcome) -> PairwiseComparison {
        PairwiseComparison {
            left_id: left.into(),
            right_id: right.into(),
            rubric_version: "1".into(),
            outcome,
            rationale: "stronger evidence".into(),
            evidence_refs: vec!["ev-1".into()],
            judge_id: "judge".into(),
            recorded_at: 1,
        }
    }

    #[test]
    fn fixed_comparisons_rank_deterministically() {
        let comparisons = vec![comparison("a", "b", PairwiseOutcome::Left)];
        let first = rank_hypotheses(&rubric(), &candidates(), &comparisons, 7).unwrap();
        let second = rank_hypotheses(&rubric(), &candidates(), &comparisons, 7).unwrap();
        assert_eq!(first.order, second.order);
        assert_eq!(first.order[0], "a");
        assert!(first.excluded.iter().any(|(id, _)| id == "c"));
        assert!(first.minority.iter().any(|id| id == "b"));
    }

    #[test]
    fn a_missing_pair_is_reported() {
        let result = rank_hypotheses(&rubric(), &candidates(), &[], 1).unwrap();
        assert_eq!(result.missing_pairs, vec![("a".into(), "b".into())]);
    }

    #[test]
    fn an_override_does_not_rewrite_comparisons() {
        let comparisons = vec![comparison("a", "b", PairwiseOutcome::Left)];
        let (kept, decision) = apply_ranking_override(
            &comparisons,
            RankingOverride {
                candidate_id: "b".into(),
                decision_id: "decision-1".into(),
                note: "human prefers b".into(),
                recorded_at: 9,
            },
        );
        assert_eq!(kept, comparisons);
        assert_eq!(decision.decision_id, "decision-1");
    }
}

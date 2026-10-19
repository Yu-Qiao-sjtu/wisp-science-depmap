//! Versioned hypothesis population. Critiques stay critiques and are not measured evidence.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PopulationSnapshot {
    pub candidate_ids: Vec<String>,
    pub cycle_version: String,
    pub rubric_version: String,
    pub budget: u32,
    pub seed: u64,
    pub model_config: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReflectionPart {
    Claim,
    Assumption,
    Objection,
    MissingEvidence,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reflection {
    pub candidate_id: String,
    pub part: ReflectionPart,
    pub text: String,
    pub evidence_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimilarityEdge {
    pub left: String,
    pub right: String,
    pub method: String,
    pub method_version: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvolutionOperator {
    Mutation,
    Refinement,
    Decomposition,
    Recombination,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvolutionOutcome {
    Improved,
    Regressed,
    Unresolved,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvolvedCandidate {
    pub id: String,
    pub parents: Vec<String>,
    pub operator: EvolutionOperator,
    pub config_digest: String,
    pub rationale: String,
    pub outcome: EvolutionOutcome,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetaReview {
    pub critique: String,
    pub minority: bool,
    pub citeable_as_evidence: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    BudgetExhausted,
    CoverageComplete,
}

pub fn schedule_pairs(ids: &[String], seed: u64, budget: usize) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for (index, left) in ids.iter().enumerate() {
        for right in ids.iter().skip(index + 1) {
            pairs.push((left.clone(), right.clone()));
        }
    }
    pairs.sort_by(|left, right| {
        let left_key = pair_key(seed, &left.0, &left.1);
        let right_key = pair_key(seed, &right.0, &right.1);
        left_key
            .cmp(&right_key)
            .then(left.0.cmp(&right.0))
            .then(left.1.cmp(&right.1))
    });
    pairs.truncate(budget);
    pairs
}

pub fn similarity_edge(
    left: &str,
    right: &str,
    method: &str,
    version: &str,
) -> Result<SimilarityEdge, String> {
    if method.is_empty() || version.is_empty() {
        return Err("similarity requires a method and version".into());
    }
    Ok(SimilarityEdge {
        left: left.into(),
        right: right.into(),
        method: method.into(),
        method_version: version.into(),
    })
}

fn pair_key(seed: u64, left: &str, right: &str) -> u64 {
    let mut hash = seed;
    for byte in left.bytes().chain(right.bytes()) {
        hash = hash.wrapping_mul(0x100_0000_01b3) ^ u64::from(byte);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_scheduling_is_stable_and_stops_on_budget() {
        let snapshot = PopulationSnapshot {
            candidate_ids: vec!["a".into(), "b".into(), "c".into()],
            cycle_version: "cycle-v1".into(),
            rubric_version: "rubric-v1".into(),
            budget: 1,
            seed: 7,
            model_config: "offline".into(),
        };
        let first = schedule_pairs(
            &snapshot.candidate_ids,
            snapshot.seed,
            snapshot.budget as usize,
        );
        let second = schedule_pairs(
            &snapshot.candidate_ids,
            snapshot.seed,
            snapshot.budget as usize,
        );
        assert_eq!(first, second);
        assert_eq!(first.len(), 1);
        assert_eq!(StopReason::BudgetExhausted, StopReason::BudgetExhausted);
    }

    #[test]
    fn evolution_similarity_and_meta_review_keep_their_records() {
        let evolved = EvolvedCandidate {
            id: "child".into(),
            parents: vec!["a".into(), "b".into()],
            operator: EvolutionOperator::Recombination,
            config_digest: "fnv64:1".into(),
            rationale: "combine the two mechanisms".into(),
            outcome: EvolutionOutcome::Regressed,
        };
        let edge = similarity_edge("a", "a-copy", "embedding", "emb-v1").unwrap();
        assert!(similarity_edge("a", "b", "", "emb-v1").is_err());
        let review = MetaReview {
            critique: "the assay does not match the claim".into(),
            minority: true,
            citeable_as_evidence: false,
        };
        let reflection = Reflection {
            candidate_id: "a".into(),
            part: ReflectionPart::Objection,
            text: "no held-out perturbation".into(),
            evidence_ids: vec!["paper-1".into()],
        };
        assert_eq!(evolved.outcome, EvolutionOutcome::Regressed);
        assert_eq!(edge.method_version, "emb-v1");
        assert!(!review.citeable_as_evidence);
        assert!(review.minority);
        assert_eq!(reflection.part, ReflectionPart::Objection);
    }
}

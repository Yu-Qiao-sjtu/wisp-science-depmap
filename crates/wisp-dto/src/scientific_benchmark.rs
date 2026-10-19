//! Offline ablation and concordance checks. Live model runs are not executed here.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BenchmarkComparison {
    SpecialistVsGeneral,
    ShallowVsDeepReview,
    OneVsManyTrajectories,
    DirectVsPairwise,
    AgentVsWorkflow,
    CrossModelStability,
    LlmVsExpert,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BenchmarkOutcome {
    Supported,
    Null,
    Negative,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkSuite {
    pub id: String,
    pub fixture_digest: String,
    pub comparisons: Vec<BenchmarkComparison>,
    pub stopping_rule: String,
    pub held_out_perturbation: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkMeasures {
    pub correctness: f64,
    pub evidence_coverage: f64,
    pub claim_violations: u32,
    pub stability: f64,
    pub disagreement: f64,
    pub cost: f64,
    pub latency_ms: u64,
    pub tool_errors: u32,
    pub expert_concordance: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkCase {
    pub id: String,
    pub suite_digest: String,
    pub outcome: BenchmarkOutcome,
    pub mechanism_claim: bool,
    pub held_out: bool,
    pub live_model: bool,
    pub measures: BenchmarkMeasures,
}

pub fn suite_digest(suite: &BenchmarkSuite) -> String {
    fnv64(serde_json::to_string(suite).unwrap_or_default().as_bytes())
}

pub fn evaluate_case(suite: &BenchmarkSuite, case: &BenchmarkCase) -> Result<(), String> {
    if suite.stopping_rule.trim().is_empty() || suite.comparisons.is_empty() {
        return Err("a benchmark must declare its comparisons and stopping rule".into());
    }
    if suite.held_out_perturbation.trim().is_empty() || !case.held_out {
        return Err("a benchmark needs a held-out perturbation".into());
    }
    if case.suite_digest != suite_digest(suite) {
        return Err("benchmark fixture digest changed; results are not comparable".into());
    }
    if case.mechanism_claim
        && matches!(
            case.outcome,
            BenchmarkOutcome::Null | BenchmarkOutcome::Negative
        )
    {
        return Err("a null or negative case cannot become a mechanism claim".into());
    }
    if case.live_model {
        return Err("this evaluator only accepts an offline fixture".into());
    }
    Ok(())
}

fn fnv64(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    format!("fnv64:{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn suite() -> BenchmarkSuite {
        BenchmarkSuite {
            id: "depmap-ablation".into(),
            fixture_digest: "fixture-1".into(),
            comparisons: vec![BenchmarkComparison::OneVsManyTrajectories],
            stopping_rule: "stop after the declared repetitions".into(),
            held_out_perturbation: "gene wording".into(),
        }
    }

    fn case(suite: &BenchmarkSuite, outcome: BenchmarkOutcome, claim: bool) -> BenchmarkCase {
        BenchmarkCase {
            id: "case-1".into(),
            suite_digest: suite_digest(suite),
            outcome,
            mechanism_claim: claim,
            held_out: true,
            live_model: false,
            measures: BenchmarkMeasures {
                correctness: 1.0,
                evidence_coverage: 1.0,
                claim_violations: 0,
                stability: 1.0,
                disagreement: 0.0,
                cost: 0.0,
                latency_ms: 1,
                tool_errors: 0,
                expert_concordance: 1.0,
            },
        }
    }

    #[test]
    fn an_offline_supported_case_passes() {
        let suite = suite();
        evaluate_case(&suite, &case(&suite, BenchmarkOutcome::Supported, true)).unwrap();
    }

    #[test]
    fn a_null_case_cannot_claim_a_mechanism() {
        let suite = suite();
        let error = evaluate_case(&suite, &case(&suite, BenchmarkOutcome::Null, true)).unwrap_err();
        assert!(error.contains("mechanism"));
    }

    #[test]
    fn a_changed_digest_is_not_comparable() {
        let suite = suite();
        let mut changed = case(&suite, BenchmarkOutcome::Supported, false);
        changed.suite_digest = "fnv64:0".into();
        let error = evaluate_case(&suite, &changed).unwrap_err();
        assert!(error.contains("not comparable"));
    }
}

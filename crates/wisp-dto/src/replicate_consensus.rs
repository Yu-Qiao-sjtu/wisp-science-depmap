//! Blinded replicate groups and a deterministic consensus reducer.
//! Failed or missing trajectories are excluded. They never count as agreement.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplicateIndependence {
    FreshContext,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsensusPolicy {
    Unanimous,
    Majority,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrajectoryState {
    Completed,
    Failed,
    Missing,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsensusState {
    Agreement,
    Majority,
    Conflict,
    Null,
    Incompatible,
    Partial,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicateGroup {
    pub id: String,
    pub trajectory_count: u32,
    pub independence: ReplicateIndependence,
    pub input_digest: String,
    pub policy: ConsensusPolicy,
    pub metric: String,
    pub unit: String,
    pub cohort: String,
    pub release: String,
    pub sample_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReplicateTrajectory {
    pub id: String,
    pub state: TrajectoryState,
    pub run_id: Option<String>,
    pub artifact_version: Option<String>,
    pub estimate: Option<f64>,
    pub direction: Option<String>,
    pub metric: Option<String>,
    pub unit: Option<String>,
    pub cohort: Option<String>,
    pub release: Option<String>,
    pub sample_id: Option<String>,
    pub failure_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConsensusReport {
    pub state: ConsensusState,
    pub agreement_rate: f64,
    pub included: Vec<String>,
    pub dissent: Vec<String>,
    pub excluded: Vec<(String, String)>,
    pub input_digest: String,
}

pub fn validate_replicate_group(group: &ReplicateGroup) -> Result<(), String> {
    if group.trajectory_count == 0 {
        return Err("a replicate group needs at least one trajectory".into());
    }
    if group.input_digest.trim().is_empty() {
        return Err("a replicate group needs a frozen input digest".into());
    }
    Ok(())
}

pub fn reduce_replicates(
    group: &ReplicateGroup,
    trajectories: &[ReplicateTrajectory],
) -> Result<ConsensusReport, String> {
    validate_replicate_group(group)?;
    if trajectories.len() as u32 != group.trajectory_count {
        return Err("trajectory count does not match the replicate group".into());
    }
    let mut seen_ids = std::collections::BTreeSet::new();
    for trajectory in trajectories {
        if !seen_ids.insert(trajectory.id.as_str()) {
            return Err("trajectory ids must be unique".into());
        }
    }
    let mut included = Vec::new();
    let mut excluded = Vec::new();
    let mut directions = Vec::new();
    for trajectory in trajectories {
        match trajectory.state {
            TrajectoryState::Failed => {
                excluded.push((
                    trajectory.id.clone(),
                    trajectory
                        .failure_reason
                        .clone()
                        .unwrap_or_else(|| "failed".into()),
                ));
            }
            TrajectoryState::Missing => {
                excluded.push((trajectory.id.clone(), "missing".into()));
            }
            TrajectoryState::Cancelled => {
                excluded.push((trajectory.id.clone(), "cancelled".into()));
            }
            TrajectoryState::Completed => {
                if !identity_matches(group, trajectory) {
                    excluded.push((trajectory.id.clone(), "incompatible identity".into()));
                    continue;
                }
                if trajectory.run_id.as_deref().unwrap_or("").is_empty()
                    || trajectory
                        .artifact_version
                        .as_deref()
                        .unwrap_or("")
                        .is_empty()
                {
                    excluded.push((
                        trajectory.id.clone(),
                        "missing run or artifact version".into(),
                    ));
                    continue;
                }
                let direction = trajectory.direction.as_deref().unwrap_or("").trim();
                if direction.is_empty() {
                    let reason = if trajectory.estimate.is_none() {
                        "null result"
                    } else {
                        "missing direction"
                    };
                    excluded.push((trajectory.id.clone(), reason.into()));
                    continue;
                }
                included.push(trajectory.id.clone());
                directions.push(direction.to_string());
            }
        }
    }
    let comparable = included.len();
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for direction in &directions {
        if let Some(entry) = counts
            .iter_mut()
            .find(|(name, _)| *name == direction.as_str())
        {
            entry.1 += 1;
        } else {
            counts.push((direction.as_str(), 1));
        }
    }
    counts.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(right.0)));
    let top_count = counts.first().map(|(_, count)| *count).unwrap_or(0);
    let tied = counts
        .iter()
        .filter(|(_, count)| *count == top_count)
        .count()
        > 1;
    let majority = if tied {
        ""
    } else {
        counts.first().map(|(name, _)| *name).unwrap_or("")
    };
    let majority_count = if majority.is_empty() { 0 } else { top_count };
    let agreement_rate = if comparable == 0 {
        0.0
    } else {
        majority_count as f64 / comparable as f64
    };
    let incompatible = excluded
        .iter()
        .any(|(_, reason)| reason == "incompatible identity");
    let nulls = excluded.iter().any(|(_, reason)| reason == "null result");
    let non_success = excluded.iter().any(|(_, reason)| {
        reason == "failed"
            || reason == "missing"
            || reason == "cancelled"
            || reason == "timeout"
            || !matches!(
                reason.as_str(),
                "incompatible identity" | "null result" | "missing run or artifact version"
            )
    });
    let state = if comparable == 0 && nulls && !incompatible {
        ConsensusState::Null
    } else if comparable == 0 && incompatible {
        ConsensusState::Incompatible
    } else if comparable == 0 {
        ConsensusState::Failed
    } else if tied {
        ConsensusState::Conflict
    } else if group.policy == ConsensusPolicy::Majority && agreement_rate > 0.5 && counts.len() > 1
    {
        ConsensusState::Majority
    } else if counts.len() > 1 {
        ConsensusState::Conflict
    } else if group.policy == ConsensusPolicy::Unanimous && comparable != trajectories.len() {
        ConsensusState::Partial
    } else if group.policy == ConsensusPolicy::Majority && agreement_rate <= 0.5 {
        ConsensusState::Conflict
    } else if non_success {
        ConsensusState::Partial
    } else {
        ConsensusState::Agreement
    };
    let dissent = if counts.len() > 1 {
        included
            .iter()
            .zip(directions.iter())
            .filter(|(_, direction)| direction.as_str() != majority)
            .map(|(id, _)| id.clone())
            .collect()
    } else {
        Vec::new()
    };
    Ok(ConsensusReport {
        state,
        agreement_rate,
        included,
        dissent,
        excluded,
        input_digest: group.input_digest.clone(),
    })
}

fn identity_matches(group: &ReplicateGroup, trajectory: &ReplicateTrajectory) -> bool {
    field_matches(&group.metric, trajectory.metric.as_deref())
        && field_matches(&group.unit, trajectory.unit.as_deref())
        && field_matches(&group.cohort, trajectory.cohort.as_deref())
        && field_matches(&group.release, trajectory.release.as_deref())
        && field_matches(&group.sample_id, trajectory.sample_id.as_deref())
}

fn field_matches(expected: &str, actual: Option<&str>) -> bool {
    actual.is_some_and(|value| value == expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(policy: ConsensusPolicy) -> ReplicateGroup {
        ReplicateGroup {
            id: "g".into(),
            trajectory_count: 3,
            independence: ReplicateIndependence::FreshContext,
            input_digest: "sha256:input".into(),
            policy,
            metric: "dependency".into(),
            unit: "score".into(),
            cohort: "lineage".into(),
            release: "24q2".into(),
            sample_id: "sample".into(),
        }
    }

    fn done(id: &str, direction: &str) -> ReplicateTrajectory {
        ReplicateTrajectory {
            id: id.into(),
            state: TrajectoryState::Completed,
            run_id: Some(format!("run-{id}")),
            artifact_version: Some("v1".into()),
            estimate: Some(1.0),
            direction: Some(direction.into()),
            metric: Some("dependency".into()),
            unit: Some("score".into()),
            cohort: Some("lineage".into()),
            release: Some("24q2".into()),
            sample_id: Some("sample".into()),
            failure_reason: None,
        }
    }

    #[test]
    fn unanimous_agreement_keeps_the_input_digest() {
        let report = reduce_replicates(
            &group(ConsensusPolicy::Unanimous),
            &[done("a", "up"), done("b", "up"), done("c", "up")],
        )
        .unwrap();
        assert_eq!(report.state, ConsensusState::Agreement);
        assert_eq!(report.agreement_rate, 1.0);
        assert_eq!(report.input_digest, "sha256:input");
        assert!(report.dissent.is_empty());
    }

    #[test]
    fn a_failed_trajectory_is_not_agreement() {
        let mut failed = done("c", "up");
        failed.state = TrajectoryState::Failed;
        failed.estimate = None;
        failed.failure_reason = Some("timeout".into());
        let report = reduce_replicates(
            &group(ConsensusPolicy::Unanimous),
            &[done("a", "up"), done("b", "up"), failed],
        )
        .unwrap();
        assert_eq!(report.state, ConsensusState::Partial);
        assert_eq!(report.excluded, vec![("c".into(), "timeout".into())]);
        assert!((report.agreement_rate - 1.0).abs() < 1e-9);
    }

    #[test]
    fn conflicting_directions_are_retained() {
        let report = reduce_replicates(
            &group(ConsensusPolicy::Majority),
            &[done("a", "up"), done("b", "down"), done("c", "up")],
        )
        .unwrap();
        assert_eq!(report.state, ConsensusState::Majority);
        assert!((report.agreement_rate - 2.0 / 3.0).abs() < 1e-9);
        assert_eq!(report.dissent, vec!["b".to_string()]);
    }

    #[test]
    fn estimate_only_results_are_not_unanimous_agreement() {
        let mut rows = [done("a", "up"), done("b", "up"), done("c", "up")];
        for (index, row) in rows.iter_mut().enumerate() {
            row.direction = None;
            row.estimate = Some(index as f64);
        }
        let report = reduce_replicates(&group(ConsensusPolicy::Unanimous), &rows).unwrap();
        assert_ne!(report.state, ConsensusState::Agreement);
        assert!(report.included.is_empty());
    }

    #[test]
    fn duplicate_trajectory_ids_are_rejected() {
        let error = reduce_replicates(
            &group(ConsensusPolicy::Unanimous),
            &[done("a", "up"), done("a", "up"), done("c", "up")],
        );
        assert!(error.is_err());
    }

    #[test]
    fn null_and_incompatible_results_stay_distinct() {
        let mut null_result = done("a", "");
        null_result.estimate = None;
        null_result.direction = None;
        let mut other = done("b", "");
        other.estimate = None;
        other.direction = None;
        let mut third = done("c", "");
        third.estimate = None;
        third.direction = None;
        let report = reduce_replicates(
            &group(ConsensusPolicy::Unanimous),
            &[null_result, other, third],
        )
        .unwrap();
        assert_eq!(report.state, ConsensusState::Null);

        let mut wrong = done("a", "up");
        wrong.unit = Some("rank".into());
        let report = reduce_replicates(
            &group(ConsensusPolicy::Unanimous),
            &[wrong, done("b", "up"), done("c", "up")],
        )
        .unwrap();
        assert!(report
            .excluded
            .iter()
            .any(|(_, reason)| reason == "incompatible identity"));
    }
}

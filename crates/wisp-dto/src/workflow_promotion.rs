//! Turn a validated agent trace into a draft Workflow. The draft is not activated.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceStepKind {
    Reasoning,
    Retrieval,
    Run,
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceStep {
    pub id: String,
    pub kind: TraceStepKind,
    pub tool: String,
    pub depends_on: Vec<String>,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowTaskKind {
    Agent,
    Fixed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowDraftTask {
    pub id: String,
    pub kind: WorkflowTaskKind,
    pub tool: String,
    pub instruction: String,
    pub depends_on: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowPromotion {
    pub draft: bool,
    pub revision_digest: String,
    pub source_trajectory_ids: Vec<String>,
    pub tasks: Vec<WorkflowDraftTask>,
    pub unsupported: Vec<String>,
    pub lossy: Vec<String>,
}

pub fn promote_trace(trajectory_id: &str, steps: &[TraceStep]) -> WorkflowPromotion {
    let mut tasks = Vec::new();
    let mut unsupported = Vec::new();
    let mut lossy = Vec::new();
    let kept: Vec<&TraceStep> = steps
        .iter()
        .filter(|step| step.kind != TraceStepKind::Unsupported)
        .collect();
    let kept_ids: Vec<&str> = kept.iter().map(|step| step.id.as_str()).collect();
    for step in steps {
        if step.kind == TraceStepKind::Unsupported {
            unsupported.push(step.id.clone());
            continue;
        }
        let instruction = sanitize_instruction(&step.text);
        if instruction != step.text {
            lossy.push(step.id.clone());
        }
        let depends_on = step
            .depends_on
            .iter()
            .filter(|id| kept_ids.contains(&id.as_str()))
            .cloned()
            .collect();
        let kind = if step.kind == TraceStepKind::Reasoning {
            WorkflowTaskKind::Agent
        } else {
            WorkflowTaskKind::Fixed
        };
        let tool = if step.kind == TraceStepKind::Reasoning {
            String::new()
        } else {
            step.tool.clone()
        };
        tasks.push(WorkflowDraftTask {
            id: step.id.clone(),
            kind,
            tool,
            instruction,
            depends_on,
        });
    }
    let body = serde_json::to_string(&tasks).unwrap_or_default();
    WorkflowPromotion {
        draft: true,
        revision_digest: fnv64(body.as_bytes()),
        source_trajectory_ids: vec![trajectory_id.to_string()],
        tasks,
        unsupported,
        lossy,
    }
}

fn sanitize_instruction(text: &str) -> String {
    if contains_secret(text) || contains_host_path(text) {
        String::new()
    } else {
        text.to_string()
    }
}

fn contains_secret(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("api_key") || lower.contains("token") || lower.contains("password")
}

fn contains_host_path(text: &str) -> bool {
    text.contains("/home/")
        || text.contains("/Users/")
        || text.contains("\\\\")
        || text.as_bytes().windows(3).any(|window| {
            window[0].is_ascii_alphabetic()
                && window[1] == b':'
                && (window[2] == b'\\' || window[2] == b'/')
        })
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

    fn step(id: &str, kind: TraceStepKind, tool: &str, text: &str) -> TraceStep {
        TraceStep {
            id: id.into(),
            kind,
            tool: tool.into(),
            depends_on: vec![],
            text: text.into(),
        }
    }

    #[test]
    fn promotion_stays_a_draft_and_replays_the_same_stages() {
        let steps = vec![
            step(
                "think",
                TraceStepKind::Reasoning,
                "",
                "consider the dependency",
            ),
            step("fetch", TraceStepKind::Retrieval, "depmap_query", "query"),
            step("run", TraceStepKind::Run, "run_in_context", "analyze"),
        ];
        let first = promote_trace("traj-1", &steps);
        let second = promote_trace("traj-1", &steps);
        assert!(first.draft);
        assert_eq!(first.revision_digest, second.revision_digest);
        assert_eq!(first.source_trajectory_ids, vec!["traj-1".to_string()]);
        assert_eq!(
            first.tasks.iter().map(|task| task.kind).collect::<Vec<_>>(),
            vec![
                WorkflowTaskKind::Agent,
                WorkflowTaskKind::Fixed,
                WorkflowTaskKind::Fixed
            ]
        );
        assert_eq!(first.tasks[1].instruction, "query");
        assert!(first.lossy.is_empty());
    }

    #[test]
    fn secrets_paths_and_unsupported_calls_are_reported() {
        let steps = vec![
            step(
                "secret",
                TraceStepKind::Retrieval,
                "depmap_query",
                "api_key=abc",
            ),
            step(
                "path",
                TraceStepKind::Run,
                "run_in_context",
                "C:\\Users\\secret",
            ),
            step(
                "unknown",
                TraceStepKind::Unsupported,
                "shell",
                "do anything",
            ),
        ];
        let promotion = promote_trace("traj-2", &steps);
        assert_eq!(
            promotion.lossy,
            vec!["secret".to_string(), "path".to_string()]
        );
        assert_eq!(promotion.unsupported, vec!["unknown".to_string()]);
        assert!(promotion
            .tasks
            .iter()
            .all(|task| task.instruction.is_empty() || task.id == "unknown"));
        let mut follow = step("run", TraceStepKind::Run, "run_in_context", "analyze");
        follow.depends_on = vec!["unknown".into()];
        let slash = step(
            "slash",
            TraceStepKind::Run,
            "run_in_context",
            "C:/Users/alice/data.csv",
        );
        let rewritten = promote_trace("traj-3", &[follow, slash]);
        assert!(rewritten.tasks[0].depends_on.is_empty());
        assert_eq!(rewritten.lossy, vec!["slash".to_string()]);
    }
}

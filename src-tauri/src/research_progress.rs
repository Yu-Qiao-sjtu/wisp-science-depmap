use serde::{Deserialize, Serialize};
use wisp_llm::ToolSchema;
use wisp_tools::{Tool, ToolEnv, ToolResult};

pub(crate) const REPORT_RESEARCH_PROGRESS: &str = "report_research_progress";
const MAX_TEXT_CHARS: usize = 1_000;
const MAX_GAPS: usize = 20;
const MAX_GAP_CHARS: usize = 500;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResearchProgressSnapshot {
    pub(crate) schema_version: u32,
    pub(crate) phase: String,
    #[serde(default)]
    pub(crate) facets_completed: u32,
    pub(crate) facets_total: Option<u32>,
    #[serde(default)]
    pub(crate) queries_completed: u32,
    pub(crate) queries_total: Option<u32>,
    #[serde(default)]
    pub(crate) candidate_sources: u32,
    #[serde(default)]
    pub(crate) screened_sources: u32,
    #[serde(default)]
    pub(crate) accepted_sources: u32,
    #[serde(default)]
    pub(crate) claims_covered: u32,
    pub(crate) claims_total: Option<u32>,
    #[serde(default)]
    pub(crate) unresolved_gaps: Vec<String>,
    pub(crate) current_query: Option<String>,
    pub(crate) note: Option<String>,
    pub(crate) updated_at: i64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResearchProgressInput {
    phase: String,
    #[serde(default)]
    facets_completed: u32,
    #[serde(default)]
    facets_total: Option<u32>,
    #[serde(default)]
    queries_completed: u32,
    #[serde(default)]
    queries_total: Option<u32>,
    #[serde(default)]
    candidate_sources: u32,
    #[serde(default)]
    screened_sources: u32,
    #[serde(default)]
    accepted_sources: u32,
    #[serde(default)]
    claims_covered: u32,
    #[serde(default)]
    claims_total: Option<u32>,
    #[serde(default)]
    unresolved_gaps: Vec<String>,
    #[serde(default)]
    current_query: Option<String>,
    #[serde(default)]
    note: Option<String>,
}

impl ResearchProgressInput {
    fn validate(&self, previous: Option<&ResearchProgressSnapshot>) -> Result<(), String> {
        if !matches!(
            self.phase.as_str(),
            "planning"
                | "searching"
                | "screening"
                | "gathering_evidence"
                | "checking_gaps"
                | "synthesizing"
                | "auditing_citations"
                | "complete"
        ) {
            return Err("research progress phase is not supported".into());
        }
        for (completed, total, label) in [
            (self.facets_completed, self.facets_total, "facets"),
            (self.queries_completed, self.queries_total, "queries"),
            (self.claims_covered, self.claims_total, "claims"),
        ] {
            if total.is_some_and(|total| completed > total) {
                return Err(format!("completed {label} cannot exceed total {label}"));
            }
        }
        if self.accepted_sources > self.screened_sources
            || self.screened_sources > self.candidate_sources
        {
            return Err("source counts must satisfy accepted <= screened <= candidate".into());
        }
        if self.unresolved_gaps.len() > MAX_GAPS {
            return Err(format!("research progress accepts at most {MAX_GAPS} gaps"));
        }
        for (label, value, limit) in [
            (
                "current_query",
                self.current_query.as_deref(),
                MAX_TEXT_CHARS,
            ),
            ("note", self.note.as_deref(), MAX_TEXT_CHARS),
        ] {
            if value.is_some_and(|value| value.chars().count() > limit) {
                return Err(format!("{label} exceeds {limit} characters"));
            }
        }
        if self
            .unresolved_gaps
            .iter()
            .any(|gap| gap.trim().is_empty() || gap.chars().count() > MAX_GAP_CHARS)
        {
            return Err(format!(
                "research gaps must be non-empty and at most {MAX_GAP_CHARS} characters"
            ));
        }
        if let Some(previous) = previous {
            for (current, old, label) in [
                (self.facets_completed, previous.facets_completed, "facets"),
                (
                    self.queries_completed,
                    previous.queries_completed,
                    "queries",
                ),
                (
                    self.candidate_sources,
                    previous.candidate_sources,
                    "candidate sources",
                ),
                (
                    self.screened_sources,
                    previous.screened_sources,
                    "screened sources",
                ),
                (
                    self.accepted_sources,
                    previous.accepted_sources,
                    "accepted sources",
                ),
                (
                    self.claims_covered,
                    previous.claims_covered,
                    "covered claims",
                ),
            ] {
                if current < old {
                    return Err(format!("cumulative {label} cannot decrease"));
                }
            }
        }
        Ok(())
    }

    fn into_snapshot(self) -> ResearchProgressSnapshot {
        ResearchProgressSnapshot {
            schema_version: 1,
            phase: self.phase,
            facets_completed: self.facets_completed,
            facets_total: self.facets_total,
            queries_completed: self.queries_completed,
            queries_total: self.queries_total,
            candidate_sources: self.candidate_sources,
            screened_sources: self.screened_sources,
            accepted_sources: self.accepted_sources,
            claims_covered: self.claims_covered,
            claims_total: self.claims_total,
            unresolved_gaps: self.unresolved_gaps,
            current_query: self.current_query,
            note: self.note,
            updated_at: chrono::Utc::now().timestamp(),
        }
    }
}

pub(crate) struct ReportResearchProgressTool {
    store: wisp_store::Store,
    frame_id: String,
}

impl ReportResearchProgressTool {
    pub(crate) fn new(store: wisp_store::Store, frame_id: impl Into<String>) -> Self {
        Self {
            store,
            frame_id: frame_id.into(),
        }
    }
}

#[async_trait::async_trait]
impl Tool for ReportResearchProgressTool {
    fn name(&self) -> &str {
        REPORT_RESEARCH_PROGRESS
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            REPORT_RESEARCH_PROGRESS,
            "Persist concise, user-visible research progress. Report observable work and cumulative counts only; never include hidden reasoning. Call before retrieval and after each bounded retrieval or evidence-review batch.",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "phase": {"type":"string","enum":["planning","searching","screening","gathering_evidence","checking_gaps","synthesizing","auditing_citations","complete"]},
                    "facets_completed": {"type":"integer","minimum":0},
                    "facets_total": {"type":"integer","minimum":0},
                    "queries_completed": {"type":"integer","minimum":0},
                    "queries_total": {"type":"integer","minimum":0},
                    "candidate_sources": {"type":"integer","minimum":0},
                    "screened_sources": {"type":"integer","minimum":0},
                    "accepted_sources": {"type":"integer","minimum":0},
                    "claims_covered": {"type":"integer","minimum":0},
                    "claims_total": {"type":"integer","minimum":0},
                    "unresolved_gaps": {"type":"array","maxItems":20,"items":{"type":"string","maxLength":500}},
                    "current_query": {"type":"string","maxLength":1000},
                    "note": {"type":"string","maxLength":1000}
                },
                "required": ["phase"]
            }),
        )
    }

    fn preview(&self, args: &serde_json::Value) -> String {
        args.get("current_query")
            .and_then(serde_json::Value::as_str)
            .or_else(|| args.get("phase").and_then(serde_json::Value::as_str))
            .unwrap_or_default()
            .chars()
            .take(120)
            .collect()
    }

    async fn run(&self, args: &serde_json::Value, _env: &dyn ToolEnv) -> ToolResult {
        let input = match serde_json::from_value::<ResearchProgressInput>(args.clone()) {
            Ok(input) => input,
            Err(error) => return ToolResult::fail(format!("invalid research progress: {error}")),
        };
        let previous = match latest_research_progress(&self.store, &self.frame_id).await {
            Ok(previous) => previous,
            Err(error) => {
                return ToolResult::fail(format!("could not read prior research progress: {error}"))
            }
        };
        if let Err(error) = input.validate(previous.as_ref()) {
            return ToolResult::fail(error);
        }
        ToolResult::ok(
            serde_json::to_string(&input.into_snapshot()).expect("research progress serializes"),
        )
    }
}

pub(crate) async fn latest_research_progress(
    store: &wisp_store::Store,
    frame_id: &str,
) -> anyhow::Result<Option<ResearchProgressSnapshot>> {
    Ok(store
        .recent_tool_result_texts(frame_id, REPORT_RESEARCH_PROGRESS, 20)
        .await?
        .into_iter()
        .find_map(|(content, _)| serde_json::from_str(&content).ok()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct NoEnv(PathBuf);

    #[async_trait::async_trait]
    impl ToolEnv for NoEnv {
        fn project_root(&self) -> &std::path::Path {
            &self.0
        }

        async fn confirm(&self, _message: &str) -> bool {
            true
        }

        async fn emit(&self, _event: wisp_tools::ToolEvent) {}
    }

    async fn fixture() -> (wisp_store::Store, PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "wisp-research-progress-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let store = wisp_store::Store::open(&path).await.unwrap();
        store.create_project("p", "Project", ".").await.unwrap();
        store
            .create_frame("f", "p", "Research", "model")
            .await
            .unwrap();
        (store, path)
    }

    #[tokio::test]
    async fn progress_is_recovered_from_the_durable_tool_result() {
        let (store, path) = fixture().await;
        let tool = ReportResearchProgressTool::new(store.clone(), "f");
        let result = tool
            .run(
                &serde_json::json!({
                    "phase":"searching",
                    "queries_completed":2,
                    "queries_total":5,
                    "candidate_sources":7,
                    "screened_sources":4,
                    "accepted_sources":3,
                    "current_query":"PTK7 liver cancer"
                }),
                &NoEnv(PathBuf::from(".")),
            )
            .await;
        assert!(result.success, "{}", result.content);
        store
            .append_message(
                "f",
                1,
                &wisp_llm::Message::tool("call", REPORT_RESEARCH_PROGRESS, result.content),
            )
            .await
            .unwrap();
        store
            .append_message(
                "f",
                2,
                &wisp_llm::Message::tool(
                    "call-2",
                    REPORT_RESEARCH_PROGRESS,
                    "progress update rejected: malformed checkpoint",
                ),
            )
            .await
            .unwrap();

        let recovered = latest_research_progress(&store, "f")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(recovered.phase, "searching");
        assert_eq!(recovered.queries_completed, 2);
        assert_eq!(recovered.accepted_sources, 3);

        store.close().await;
        std::fs::remove_file(path).ok();
    }

    #[tokio::test]
    async fn progress_rejects_impossible_or_decreasing_counts() {
        let (store, path) = fixture().await;
        let tool = ReportResearchProgressTool::new(store.clone(), "f");
        let impossible = tool
            .run(
                &serde_json::json!({
                    "phase":"screening",
                    "candidate_sources":2,
                    "screened_sources":3
                }),
                &NoEnv(PathBuf::from(".")),
            )
            .await;
        assert!(!impossible.success);

        store
            .append_message(
                "f",
                1,
                &wisp_llm::Message::tool(
                    "call",
                    REPORT_RESEARCH_PROGRESS,
                    serde_json::json!({
                        "schema_version":1,
                        "phase":"searching",
                        "facets_completed":0,
                        "facets_total":null,
                        "queries_completed":4,
                        "queries_total":null,
                        "candidate_sources":8,
                        "screened_sources":5,
                        "accepted_sources":3,
                        "claims_covered":0,
                        "claims_total":null,
                        "unresolved_gaps":[],
                        "current_query":null,
                        "note":null,
                        "updated_at":1
                    })
                    .to_string(),
                ),
            )
            .await
            .unwrap();
        let decreasing = tool
            .run(
                &serde_json::json!({
                    "phase":"checking_gaps",
                    "queries_completed":3,
                    "candidate_sources":8,
                    "screened_sources":5,
                    "accepted_sources":3
                }),
                &NoEnv(PathBuf::from(".")),
            )
            .await;
        assert!(!decreasing.success);
        assert!(decreasing.content.contains("cannot decrease"));

        store.close().await;
        std::fs::remove_file(path).ok();
    }
}

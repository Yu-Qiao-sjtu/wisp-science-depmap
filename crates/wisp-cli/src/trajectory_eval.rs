use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use wisp_dto::{TrajectoryCellDto, TrajectorySnapshotDto};

const RUBRIC_SCHEMA: &str = "wisp.depmap-trajectory-rubric.v1";
const REPORT_SCHEMA: &str = "wisp.depmap-trajectory-report.v1";
const DEFAULT_RUBRIC: &str = include_str!("../eval-suites/depmap-trajectory-v1.yaml");

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    pub input: Option<PathBuf>,
    pub rubric: Option<PathBuf>,
    pub save: Option<PathBuf>,
    pub allow_failures: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct Rubric {
    schema: String,
    id: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    limits: GlobalLimits,
    #[serde(default)]
    turns: Vec<TurnRule>,
    #[serde(default)]
    forbidden_model_text: Vec<String>,
    #[serde(default)]
    memory_markers: Vec<String>,
    #[serde(default)]
    invalid_query_markers: Vec<String>,
    #[serde(default)]
    remote_failure_markers: Vec<String>,
    #[serde(default)]
    manual_review: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct GlobalLimits {
    #[serde(default)]
    max_tool_error_rate_percent: Option<f64>,
    #[serde(default)]
    max_invalid_depmap_queries: Option<u64>,
    #[serde(default)]
    max_remote_query_failures: Option<u64>,
    #[serde(default)]
    max_duplicate_tool_calls: Option<u64>,
    #[serde(default)]
    max_memory_marker_mentions: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct TurnRule {
    turn: i64,
    #[serde(default)]
    max_model_rounds: Option<u64>,
    #[serde(default)]
    max_input_tokens: Option<i64>,
    #[serde(default)]
    required_tools: Vec<String>,
    #[serde(default)]
    forbidden_tools: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
struct ToolMetrics {
    calls: u64,
    errors: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
struct TurnMetrics {
    turn: i64,
    model_rounds: u64,
    input_tokens: i64,
    output_tokens: i64,
    tool_calls: u64,
    tool_errors: u64,
    tools: BTreeMap<String, ToolMetrics>,
}

#[derive(Debug, Clone, Default, Serialize)]
struct Metrics {
    turns: usize,
    model_rounds: u64,
    input_tokens: i64,
    output_tokens: i64,
    cached_input_tokens: i64,
    tool_calls: u64,
    tool_errors: u64,
    tool_error_rate_percent: f64,
    invalid_depmap_queries: u64,
    remote_query_failures: u64,
    duplicate_tool_calls: u64,
    memory_marker_mentions: u64,
    turn_metrics: Vec<TurnMetrics>,
    tools: BTreeMap<String, ToolMetrics>,
}

#[derive(Debug, Clone, Serialize)]
struct Finding {
    gate: String,
    expected: String,
    actual: String,
}

#[derive(Debug, Clone, Serialize)]
struct Report {
    schema: &'static str,
    rubric_id: String,
    rubric_description: String,
    source: String,
    frame_id: String,
    model: Option<String>,
    passed: bool,
    metrics: Metrics,
    failures: Vec<Finding>,
    manual_review: Vec<String>,
}

pub fn run(options: &Options) -> Result<()> {
    let input = options
        .input
        .as_ref()
        .context("trajectory-eval requires --input")?;
    let rubric_text = match &options.rubric {
        Some(path) => std::fs::read_to_string(path)
            .with_context(|| format!("could not read rubric {}", path.display()))?,
        None => DEFAULT_RUBRIC.to_string(),
    };
    let rubric: Rubric = serde_yaml::from_str(&rubric_text).context("invalid trajectory rubric")?;
    if rubric.schema != RUBRIC_SCHEMA {
        bail!(
            "unsupported trajectory rubric schema '{}'; expected {RUBRIC_SCHEMA}",
            rubric.schema
        );
    }
    let snapshot = load_snapshot(input)?;
    let metrics = collect_metrics(&snapshot, &rubric);
    let failures = verify(&snapshot, &metrics, &rubric);
    let report = Report {
        schema: REPORT_SCHEMA,
        rubric_id: rubric.id,
        rubric_description: rubric.description,
        source: input.to_string_lossy().into_owned(),
        frame_id: snapshot.frame_id.clone(),
        model: snapshot.model.clone(),
        passed: failures.is_empty(),
        metrics,
        failures,
        manual_review: rubric.manual_review,
    };
    let rendered = serde_json::to_string_pretty(&report)?;
    println!("{rendered}");
    if let Some(path) = &options.save {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("could not create {}", parent.display()))?;
        }
        std::fs::write(path, format!("{rendered}\n"))
            .with_context(|| format!("could not save {}", path.display()))?;
    }
    if !report.passed && !options.allow_failures {
        bail!(
            "trajectory failed {} benchmark gate(s)",
            report.failures.len()
        );
    }
    Ok(())
}

fn load_snapshot(path: &Path) -> Result<TrajectorySnapshotDto> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("could not read trajectory {}", path.display()))?;
    if path.extension().and_then(|value| value.to_str()) == Some("json") {
        return serde_json::from_str(&text).context("invalid trajectory JSON");
    }
    let marker = "<details class=\"raw\">";
    let details = text
        .find(marker)
        .map(|index| &text[index + marker.len()..])
        .context("trajectory HTML has no raw snapshot block")?;
    let pre_start = details
        .find("<pre>")
        .map(|index| index + "<pre>".len())
        .context("trajectory HTML raw snapshot has no <pre>")?;
    let body = &details[pre_start..];
    let pre_end = body
        .find("</pre>")
        .context("trajectory HTML raw snapshot has no </pre>")?;
    let json = unescape_html(&body[..pre_end]);
    serde_json::from_str(&json).context("invalid raw trajectory snapshot JSON")
}

fn unescape_html(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn tool_name(cell: &TrajectoryCellDto) -> String {
    cell.summary
        .split_whitespace()
        .next()
        .unwrap_or("unknown")
        .to_string()
}

fn canonical_arguments(cell: &TrajectoryCellDto) -> String {
    let Some(input) = cell.detail_input.as_deref() else {
        return String::new();
    };
    match serde_json::from_str::<Value>(input) {
        Ok(value) => canonical_json(&value),
        Err(_) => input.split_whitespace().collect::<Vec<_>>().join(" "),
    }
}

fn canonical_json(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<_> = map.keys().collect();
            keys.sort();
            let parts: Vec<_> = keys
                .into_iter()
                .map(|key| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap(),
                        canonical_json(&map[key])
                    )
                })
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(canonical_json)
                .collect::<Vec<_>>()
                .join(",")
        ),
        _ => serde_json::to_string(value).unwrap_or_default(),
    }
}

fn count_case_insensitive(haystack: &str, needle: &str) -> u64 {
    if needle.is_empty() {
        return 0;
    }
    haystack
        .to_lowercase()
        .match_indices(&needle.to_lowercase())
        .count() as u64
}

fn model_authored_text(snapshot: &TrajectorySnapshotDto) -> String {
    let mut text = String::new();
    for turn in &snapshot.turns {
        for cell in &turn.cells {
            if cell.kind == "assistant" {
                if let Some(output) = &cell.detail_output {
                    text.push_str(output);
                    text.push('\n');
                }
            } else if cell.kind == "tool" {
                if let Some(input) = &cell.detail_input {
                    text.push_str(input);
                    text.push('\n');
                }
            }
        }
    }
    text
}

fn collect_metrics(snapshot: &TrajectorySnapshotDto, rubric: &Rubric) -> Metrics {
    let mut metrics = Metrics {
        turns: snapshot.turns.len(),
        ..Default::default()
    };
    let mut duplicate_calls = 0u64;
    for turn in &snapshot.turns {
        let mut turn_metrics = TurnMetrics {
            turn: turn.index,
            ..Default::default()
        };
        let mut signatures: HashMap<String, u64> = HashMap::new();
        for cell in &turn.cells {
            if let Some(usage) = &cell.usage {
                turn_metrics.model_rounds += 1;
                turn_metrics.input_tokens += usage.input_tokens;
                turn_metrics.output_tokens += usage.output_tokens;
                metrics.cached_input_tokens += usage.cached_input_tokens;
            }
            if cell.kind != "tool" {
                continue;
            }
            let name = tool_name(cell);
            let failed = cell.is_error || cell.ok == Some(false);
            turn_metrics.tool_calls += 1;
            if failed {
                turn_metrics.tool_errors += 1;
            }
            let per_turn = turn_metrics.tools.entry(name.clone()).or_default();
            per_turn.calls += 1;
            per_turn.errors += u64::from(failed);
            let global = metrics.tools.entry(name.clone()).or_default();
            global.calls += 1;
            global.errors += u64::from(failed);
            let signature = format!("{name}:{}", canonical_arguments(cell));
            let seen = signatures.entry(signature).or_default();
            if *seen > 0 {
                duplicate_calls += 1;
            }
            *seen += 1;
            let output = cell.detail_output.as_deref().unwrap_or_default();
            metrics.invalid_depmap_queries += rubric
                .invalid_query_markers
                .iter()
                .map(|marker| count_case_insensitive(output, marker))
                .sum::<u64>();
            metrics.remote_query_failures += rubric
                .remote_failure_markers
                .iter()
                .map(|marker| count_case_insensitive(output, marker))
                .sum::<u64>();
        }
        metrics.model_rounds += turn_metrics.model_rounds;
        metrics.input_tokens += turn_metrics.input_tokens;
        metrics.output_tokens += turn_metrics.output_tokens;
        metrics.tool_calls += turn_metrics.tool_calls;
        metrics.tool_errors += turn_metrics.tool_errors;
        metrics.turn_metrics.push(turn_metrics);
    }
    metrics.duplicate_tool_calls = duplicate_calls;
    let assistant_text = snapshot
        .turns
        .iter()
        .flat_map(|turn| &turn.cells)
        .filter(|cell| cell.kind == "assistant")
        .filter_map(|cell| cell.detail_output.as_deref())
        .collect::<Vec<_>>()
        .join("\n");
    metrics.memory_marker_mentions = rubric
        .memory_markers
        .iter()
        .map(|marker| count_case_insensitive(&assistant_text, marker))
        .sum();
    metrics.tool_error_rate_percent = if metrics.tool_calls == 0 {
        0.0
    } else {
        metrics.tool_errors as f64 * 100.0 / metrics.tool_calls as f64
    };
    metrics
}

fn verify(snapshot: &TrajectorySnapshotDto, metrics: &Metrics, rubric: &Rubric) -> Vec<Finding> {
    let mut failures = Vec::new();
    macro_rules! max_gate {
        ($name:expr, $limit:expr, $actual:expr) => {
            if let Some(limit) = $limit {
                if $actual > limit {
                    failures.push(Finding {
                        gate: $name.into(),
                        expected: format!("<= {limit}"),
                        actual: $actual.to_string(),
                    });
                }
            }
        };
    }
    if let Some(limit) = rubric.limits.max_tool_error_rate_percent {
        if metrics.tool_error_rate_percent > limit {
            failures.push(Finding {
                gate: "tool_error_rate_percent".into(),
                expected: format!("<= {limit:.2}"),
                actual: format!("{:.2}", metrics.tool_error_rate_percent),
            });
        }
    }
    max_gate!(
        "invalid_depmap_queries",
        rubric.limits.max_invalid_depmap_queries,
        metrics.invalid_depmap_queries
    );
    max_gate!(
        "remote_query_failures",
        rubric.limits.max_remote_query_failures,
        metrics.remote_query_failures
    );
    max_gate!(
        "duplicate_tool_calls",
        rubric.limits.max_duplicate_tool_calls,
        metrics.duplicate_tool_calls
    );
    max_gate!(
        "memory_marker_mentions",
        rubric.limits.max_memory_marker_mentions,
        metrics.memory_marker_mentions
    );
    for rule in &rubric.turns {
        let Some(actual) = metrics
            .turn_metrics
            .iter()
            .find(|turn| turn.turn == rule.turn)
        else {
            failures.push(Finding {
                gate: format!("turn_{}_present", rule.turn),
                expected: "present".into(),
                actual: "missing".into(),
            });
            continue;
        };
        max_gate!(
            format!("turn_{}_model_rounds", rule.turn),
            rule.max_model_rounds,
            actual.model_rounds
        );
        max_gate!(
            format!("turn_{}_input_tokens", rule.turn),
            rule.max_input_tokens,
            actual.input_tokens
        );
        for tool in &rule.required_tools {
            if !actual.tools.contains_key(tool) {
                failures.push(Finding {
                    gate: format!("turn_{}_required_tool", rule.turn),
                    expected: tool.clone(),
                    actual: "not called".into(),
                });
            }
        }
        for tool in &rule.forbidden_tools {
            if actual.tools.contains_key(tool) {
                failures.push(Finding {
                    gate: format!("turn_{}_forbidden_tool", rule.turn),
                    expected: format!("{tool} not called"),
                    actual: format!("{} call(s)", actual.tools[tool].calls),
                });
            }
        }
    }
    let authored = model_authored_text(snapshot);
    for fragment in &rubric.forbidden_model_text {
        let count = count_case_insensitive(&authored, fragment);
        if count > 0 {
            failures.push(Finding {
                gate: "forbidden_model_text".into(),
                expected: format!("no '{fragment}'"),
                actual: format!("{count} occurrence(s)"),
            });
        }
    }
    failures
}

#[cfg(test)]
mod tests {
    use super::*;
    use wisp_dto::{TrajectoryStatsDto, TrajectoryTurnDto, TrajectoryUsageDto};

    fn snapshot() -> TrajectorySnapshotDto {
        TrajectorySnapshotDto {
            frame_id: "frame-1".into(),
            model: Some("model-a".into()),
            turns: vec![TrajectoryTurnDto {
                index: 1,
                started_at: None,
                cells: vec![
                    TrajectoryCellDto {
                        kind: "usage".into(),
                        usage: Some(TrajectoryUsageDto {
                            round: 1,
                            model: Some("model-a".into()),
                            input_tokens: 100,
                            output_tokens: 10,
                            reasoning_tokens: 0,
                            cached_input_tokens: 50,
                        }),
                        ..Default::default()
                    },
                    TrajectoryCellDto {
                        kind: "tool".into(),
                        summary: "depmap_query {...}".into(),
                        detail_input: Some(r#"{"mode":"core","gene":"TP53"}"#.into()),
                        detail_output: Some(r#"{"state":"precomputed_query"}"#.into()),
                        ok: Some(true),
                        ..Default::default()
                    },
                ],
            }],
            stats: TrajectoryStatsDto::default(),
        }
    }

    fn rubric() -> Rubric {
        serde_yaml::from_str(DEFAULT_RUBRIC).unwrap()
    }

    #[test]
    fn loads_json_and_exported_html_snapshots() {
        let snapshot = snapshot();
        let root =
            std::env::temp_dir().join(format!("wisp-trajectory-eval-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let json_path = root.join("trace.json");
        std::fs::write(&json_path, serde_json::to_string(&snapshot).unwrap()).unwrap();
        assert_eq!(load_snapshot(&json_path).unwrap().frame_id, "frame-1");
        let escaped = serde_json::to_string_pretty(&snapshot)
            .unwrap()
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;");
        let html_path = root.join("trace.html");
        std::fs::write(
            &html_path,
            format!("<details class=\"raw\"><summary>raw</summary><pre>{escaped}</pre></details>"),
        )
        .unwrap();
        assert_eq!(load_snapshot(&html_path).unwrap().frame_id, "frame-1");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn metrics_count_rounds_tools_errors_and_duplicates() {
        let mut snapshot = snapshot();
        let duplicate = snapshot.turns[0].cells[1].clone();
        snapshot.turns[0].cells.push(duplicate);
        snapshot.turns[0].cells.push(TrajectoryCellDto {
            kind: "tool".into(),
            summary: "depmap_query {...}".into(),
            detail_input: Some(r#"{"mode":"pair"}"#.into()),
            detail_output: Some(r#"{"code":"invalid_query"}"#.into()),
            ok: Some(false),
            is_error: true,
            ..Default::default()
        });
        let metrics = collect_metrics(&snapshot, &rubric());
        assert_eq!(metrics.model_rounds, 1);
        assert_eq!(metrics.input_tokens, 100);
        assert_eq!(metrics.tool_calls, 3);
        assert_eq!(metrics.tool_errors, 1);
        assert_eq!(metrics.duplicate_tool_calls, 1);
        assert_eq!(metrics.invalid_depmap_queries, 1);
    }

    #[test]
    fn rubric_rejects_forbidden_text_and_turn_budget_overrun() {
        let mut snapshot = snapshot();
        snapshot.turns[0].cells.push(TrajectoryCellDto {
            kind: "assistant".into(),
            detail_output: Some("证据链已闭环".into()),
            ..Default::default()
        });
        for round in 2..=9 {
            snapshot.turns[0].cells.push(TrajectoryCellDto {
                kind: "usage".into(),
                usage: Some(TrajectoryUsageDto {
                    round,
                    model: None,
                    input_tokens: 10,
                    output_tokens: 1,
                    reasoning_tokens: 0,
                    cached_input_tokens: 0,
                }),
                ..Default::default()
            });
        }
        let rubric = rubric();
        let metrics = collect_metrics(&snapshot, &rubric);
        let failures = verify(&snapshot, &metrics, &rubric);
        assert!(failures
            .iter()
            .any(|failure| failure.gate == "forbidden_model_text"));
        assert!(failures
            .iter()
            .any(|failure| failure.gate == "turn_1_model_rounds"));
    }
}

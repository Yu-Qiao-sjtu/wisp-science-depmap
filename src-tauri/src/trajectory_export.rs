//! Export a session trajectory as a self-contained HTML document.
//!
//! The HTML is generated from a freshly folded snapshot (persisted messages
//! + UI events), not from the frontend's filtered inspector view. The Gantt
//! timeline is the reason this is HTML rather than Markdown.

use crate::trajectory::{
    fold_trajectory, TrajectoryCell, TrajectorySnapshot, TrajectoryStats, TrajectoryUsage,
};
use crate::AppState;
use serde::Serialize;
use std::fmt::Write as _;
use std::path::Path;
use tauri::{AppHandle, State};
use wisp_core::observability::{TRACE_FORMAT, TRACE_FORMAT_VERSION};
use wisp_core::{Span, SpanKind, SpanStatus};

#[derive(Clone, Debug, Default, Serialize)]
struct TraceMetric {
    /// Exact total when every successful model span reported this metric.
    value: Option<u64>,
    /// Sum of the spans that did report it. This is diagnostic only when
    /// `value` is unavailable and is never presented as the complete total.
    known_subtotal: u64,
    omitted_spans: usize,
}

#[derive(Clone, Debug, Serialize)]
struct ExportModelSpan {
    span_id: String,
    turn_id: String,
    model: Option<String>,
    status: SpanStatus,
    start_unix_ms: u64,
    end_unix_ms: u64,
    latency_ms: Option<u64>,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cached_input_tokens: Option<u64>,
}

#[derive(Clone, Debug, Default, Serialize)]
struct TraceExportSummary {
    model_spans: Vec<ExportModelSpan>,
    successful_model_spans: usize,
    excluded_model_spans: usize,
    malformed_model_spans: usize,
    unreadable_trace_files: usize,
    llm_ms: TraceMetric,
    input_tokens: TraceMetric,
    output_tokens: TraceMetric,
    cached_input_tokens: TraceMetric,
}

#[derive(Debug, Default)]
struct TraceDocuments {
    documents: Vec<String>,
    unreadable_files: usize,
}

impl TraceMetric {
    fn from_values(values: impl IntoIterator<Item = Option<u64>>) -> Self {
        let mut metric = Self::default();
        let mut count = 0usize;
        for value in values {
            count += 1;
            match value {
                Some(value) => metric.known_subtotal = metric.known_subtotal.saturating_add(value),
                None => metric.omitted_spans += 1,
            }
        }
        if count > 0 && metric.omitted_spans == 0 {
            metric.value = Some(metric.known_subtotal);
        }
        metric
    }
}

fn trace_summary_from_jsonl<'a>(
    frame_id: &str,
    documents: impl IntoIterator<Item = &'a str>,
) -> TraceExportSummary {
    let mut summary = TraceExportSummary::default();
    for document in documents {
        for line in document.lines().filter(|line| !line.trim().is_empty()) {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            let is_selected_model = value.get("record_type").and_then(|v| v.as_str())
                == Some("span")
                && value.get("kind").and_then(|v| v.as_str()) == Some("model")
                && value.get("session_id").and_then(|v| v.as_str()) == Some(frame_id);
            if !is_selected_model {
                continue;
            }
            if value.get("format").and_then(|v| v.as_str()) != Some(TRACE_FORMAT)
                || value.get("format_version").and_then(|v| v.as_u64())
                    != Some(u64::from(TRACE_FORMAT_VERSION))
            {
                summary.malformed_model_spans += 1;
                continue;
            }
            let Ok(span) = serde_json::from_value::<Span>(value) else {
                summary.malformed_model_spans += 1;
                continue;
            };
            if span.kind != SpanKind::Model {
                continue;
            }
            let Some(end_unix_ms) = span.end_unix_ms.filter(|end| *end >= span.start_unix_ms)
            else {
                summary.malformed_model_spans += 1;
                continue;
            };
            if span.status == SpanStatus::Ok {
                summary.successful_model_spans += 1;
            } else {
                summary.excluded_model_spans += 1;
            }
            summary.model_spans.push(ExportModelSpan {
                span_id: span.span_id,
                turn_id: span.turn_id,
                model: span.attributes.model_id,
                status: span.status,
                start_unix_ms: span.start_unix_ms,
                end_unix_ms,
                latency_ms: span.latency_ms,
                input_tokens: span.attributes.input_tokens,
                output_tokens: span.attributes.output_tokens,
                cached_input_tokens: span.attributes.cached_input_tokens,
            });
        }
    }
    summary
        .model_spans
        .sort_by_key(|span| (span.start_unix_ms, span.end_unix_ms, span.span_id.clone()));
    let successful: Vec<_> = summary
        .model_spans
        .iter()
        .filter(|span| span.status == SpanStatus::Ok)
        .collect();
    summary.llm_ms = TraceMetric::from_values(successful.iter().map(|span| span.latency_ms));
    summary.input_tokens =
        TraceMetric::from_values(successful.iter().map(|span| span.input_tokens));
    summary.output_tokens =
        TraceMetric::from_values(successful.iter().map(|span| span.output_tokens));
    summary.cached_input_tokens =
        TraceMetric::from_values(successful.iter().map(|span| span.cached_input_tokens));
    summary
}

async fn read_trace_documents(project_root: &Path) -> TraceDocuments {
    let mut result = TraceDocuments::default();
    for leaf in ["traces", "traces-sensitive"] {
        let dir = project_root.join(".wisp").join(leaf);
        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => {
                result.unreadable_files += 1;
                continue;
            }
        };
        loop {
            let entry = match entries.next_entry().await {
                Ok(Some(entry)) => entry,
                Ok(None) => break,
                Err(_) => {
                    result.unreadable_files += 1;
                    break;
                }
            };
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            match tokio::fs::read_to_string(&path).await {
                Ok(body) => result.documents.push(body),
                Err(_) => result.unreadable_files += 1,
            }
        }
    }
    result
}

fn trace_summary_from_documents(frame_id: &str, documents: TraceDocuments) -> TraceExportSummary {
    let mut trace =
        trace_summary_from_jsonl(frame_id, documents.documents.iter().map(String::as_str));
    trace.unreadable_trace_files = documents.unreadable_files;
    if trace.unreadable_trace_files > 0 {
        trace.llm_ms.value = None;
        trace.input_tokens.value = None;
        trace.output_tokens.value = None;
        trace.cached_input_tokens.value = None;
    }
    trace
}

const EXPORT_CSS: &str = r#"
:root {
  --bg: #f6f4ef; --card: #fff; --text: #2b2a27; --muted: #6d6a63;
  --line: #e4e0d6; --input: #c5c1b8; --model: #4d84c4; --tools: #c45a3a;
  --user: #2f6f9f; --assistant: #6b4ea3; --error: #b42318;
}
@media (prefers-color-scheme: dark) {
  :root {
    --bg: #1c1b18; --card: #26241f; --text: #ece8df; --muted: #a8a39a;
    --line: #3a372f; --input: #8c877c; --model: #7aa7d9; --tools: #d57a5e;
    --user: #7eb3d6; --assistant: #b39adf; --error: #f2b8b5;
  }
}
* { box-sizing: border-box; }
html { scroll-behavior: smooth; }
body {
  margin: 0; background: var(--bg); color: var(--text);
  font: 14px/1.5 ui-sans-serif, system-ui, -apple-system, sans-serif;
}
main { max-width: 980px; margin: 0 auto; padding: 28px 20px 64px; }
header.meta { margin-bottom: 22px; }
header.meta h1 { margin: 0 0 8px; font-size: 22px; letter-spacing: -0.02em; }
.meta-line { display: flex; flex-wrap: wrap; gap: 8px 18px; color: var(--muted); font-size: 13px; }
.meta-line strong { color: var(--text); font-weight: 600; }
.stats {
  margin: 12px 0 0; padding: 10px 12px; background: var(--card);
  border: 1px solid var(--line); border-radius: 8px; color: var(--muted); font-size: 13px;
}
.timeline { margin: 22px 0 28px; }
.timeline h2, .turn h2 { margin: 0 0 10px; font-size: 15px; }
.gantt { display: flex; flex-direction: column; gap: 8px; }
.lane { display: flex; align-items: center; gap: 10px; }
.lane-label { width: 56px; flex: 0 0 auto; color: var(--muted); font-size: 12px; }
.track {
  position: relative; flex: 1 1 auto; height: 14px;
  background: color-mix(in srgb, var(--line) 70%, transparent);
  border-radius: 4px;
}
.seg {
  position: absolute; top: 0; height: 100%; border-radius: 3px; min-width: 3px;
}
.seg.input { background: var(--input); }
.seg.model { background: var(--model); }
.seg.tools { background: var(--tools); }
.seg.error { outline: 1px solid var(--error); }
.turn {
  margin: 0 0 22px; padding: 14px 16px 8px; background: var(--card);
  border: 1px solid var(--line); border-radius: 10px;
}
.event { margin: 0 0 14px; padding-bottom: 12px; border-bottom: 1px solid var(--line); }
.event:last-child { border-bottom: 0; margin-bottom: 0; }
.event-head { display: flex; flex-wrap: wrap; align-items: baseline; gap: 8px 12px; }
.badge {
  display: inline-block; padding: 1px 7px; border-radius: 999px;
  font-size: 11px; font-weight: 700; letter-spacing: 0.04em;
}
.badge.user { color: var(--user); background: color-mix(in srgb, var(--user) 14%, transparent); }
.badge.assistant { color: var(--assistant); background: color-mix(in srgb, var(--assistant) 14%, transparent); }
.badge.tool { color: var(--tools); background: color-mix(in srgb, var(--tools) 14%, transparent); }
.badge.usage { color: var(--model); background: color-mix(in srgb, var(--model) 14%, transparent); }
.badge.error { color: var(--error); background: color-mix(in srgb, var(--error) 16%, transparent); }
.event-head .summary { color: var(--muted); font-size: 13px; }
.kv { display: flex; flex-wrap: wrap; gap: 6px 16px; margin: 6px 0 8px; color: var(--muted); font-size: 12px; }
pre {
  margin: 6px 0 0; padding: 10px 12px; overflow: auto;
  background: color-mix(in srgb, var(--bg) 70%, var(--card));
  border: 1px solid var(--line); border-radius: 6px;
  font: 12.5px/1.45 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  white-space: pre-wrap; overflow-wrap: anywhere;
}
.block-label { margin: 10px 0 0; color: var(--muted); font-size: 12px; font-weight: 600; }
.empty { color: var(--muted); padding: 24px 0; }
.raw { margin-top: 28px; }
.raw summary { cursor: pointer; color: var(--muted); }
"#;

/// Safe default file name for the native save dialog.
pub(crate) fn trajectory_file_name(frame_id: &str) -> String {
    let safe: String = frame_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .take(64)
        .collect();
    let safe = safe.trim_matches('-');
    if safe.is_empty() {
        "wisp-trajectory.html".into()
    } else {
        format!("wisp-trajectory-{safe}.html")
    }
}

#[derive(Clone, Copy)]
struct Labels {
    lang: &'static str,
    title: &'static str,
    session: &'static str,
    model: &'static str,
    exported: &'static str,
    timeline: &'static str,
    input: &'static str,
    model_lane: &'static str,
    tools: &'static str,
    turn: &'static str,
    user: &'static str,
    assistant: &'static str,
    tool: &'static str,
    usage: &'static str,
    arguments: &'static str,
    result: &'static str,
    status: &'static str,
    duration: &'static str,
    timestamp: &'static str,
    completed: &'static str,
    error: &'static str,
    pending: &'static str,
    empty: &'static str,
    raw: &'static str,
    unknown_model: &'static str,
    model_spans: &'static str,
    unavailable: &'static str,
    token_denominator: &'static str,
    omitted: &'static str,
}

fn labels(locale: &str) -> Labels {
    if locale.eq_ignore_ascii_case("zh") || locale.starts_with("zh-") || locale.starts_with("zh_") {
        Labels {
            lang: "zh",
            title: "轨迹",
            session: "会话",
            model: "模型",
            exported: "导出时间",
            timeline: "时间线",
            input: "输入",
            model_lane: "模型",
            tools: "工具",
            turn: "第 {n} 轮",
            user: "用户",
            assistant: "助手",
            tool: "工具",
            usage: "用量",
            arguments: "参数",
            result: "结果",
            status: "状态",
            duration: "耗时",
            timestamp: "时间",
            completed: "已完成",
            error: "错误",
            pending: "等待中",
            empty: "暂无轨迹事件。",
            raw: "原始快照（JSON）",
            unknown_model: "未知",
            model_spans: "模型调用跨度",
            unavailable: "不可用",
            token_denominator: "缓存占比 = cached input / input",
            omitted: "缺失",
        }
    } else {
        Labels {
            lang: "en",
            title: "Trajectory",
            session: "Session",
            model: "Model",
            exported: "Exported",
            timeline: "Timeline",
            input: "Input",
            model_lane: "Model",
            tools: "Tools",
            turn: "Turn {n}",
            user: "User",
            assistant: "Assistant",
            tool: "Tool",
            usage: "Usage",
            arguments: "Arguments",
            result: "Result",
            status: "Status",
            duration: "Duration",
            timestamp: "Time",
            completed: "Completed",
            error: "Error",
            pending: "Pending",
            empty: "No trajectory events.",
            raw: "Raw snapshot (JSON)",
            unknown_model: "unknown",
            model_spans: "Model spans",
            unavailable: "unavailable",
            token_denominator: "cache share = cached input / input",
            omitted: "omitted",
        }
    }
}

fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn fmt_tokens(n: i64) -> String {
    if n.abs() < 1000 {
        n.to_string()
    } else {
        format!("{:.1}k", n as f64 / 1000.0)
    }
}

fn format_duration_ms(ms: i64) -> String {
    let ms = ms.max(0) as u64;
    if ms < 1000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{}s", ms / 1000)
    } else {
        let mins = ms / 60_000;
        let secs = (ms % 60_000) / 1000;
        if secs == 0 {
            format!("{mins}m")
        } else {
            format!("{mins}m {secs}s")
        }
    }
}

fn format_ts(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_else(|| ms.to_string())
}

fn cell_status<'a>(cell: &'a TrajectoryCell, l: &Labels) -> &'a str {
    if cell.is_error || cell.ok == Some(false) {
        l.error
    } else if cell.kind == "tool" && cell.ok.is_none() {
        l.pending
    } else {
        l.completed
    }
}

fn kind_label<'a>(kind: &'a str, l: &'a Labels) -> &'a str {
    match kind {
        "user" => l.user,
        "assistant" => l.assistant,
        "tool" => l.tool,
        "usage" => l.usage,
        _ => kind,
    }
}

fn metric_text(
    metric: &TraceMetric,
    unreadable_trace_files: usize,
    l: &Labels,
    duration: bool,
) -> String {
    if unreadable_trace_files == 0 {
        if let Some(value) = metric.value {
            return if duration {
                format_duration_ms(value.min(i64::MAX as u64) as i64)
            } else {
                value.to_string()
            };
        }
    }
    if metric.omitted_spans > 0 || unreadable_trace_files > 0 {
        let mut details = Vec::new();
        if metric.known_subtotal > 0 || metric.omitted_spans > 0 {
            let known = if duration {
                format_duration_ms(metric.known_subtotal.min(i64::MAX as u64) as i64)
            } else {
                metric.known_subtotal.to_string()
            };
            details.push(format!("known subtotal {known}"));
        }
        if metric.omitted_spans > 0 {
            details.push(format!("{} {} span(s)", metric.omitted_spans, l.omitted));
        }
        if unreadable_trace_files > 0 {
            details.push(format!("{unreadable_trace_files} unreadable trace file(s)"));
        }
        format!("{} ({})", l.unavailable, details.join("; "))
    } else {
        l.unavailable.into()
    }
}

fn stats_line(stats: &TrajectoryStats, trace: &TraceExportSummary, l: &Labels) -> String {
    let cache = match (trace.cached_input_tokens.value, trace.input_tokens.value) {
        (Some(cached), Some(input)) if input > 0 => format!(
            "{:.1}% ({})",
            cached as f64 * 100.0 / input as f64,
            l.token_denominator
        ),
        _ => format!("{} ({})", l.unavailable, l.token_denominator),
    };
    let tok_s = match (trace.output_tokens.value, trace.llm_ms.value) {
        (Some(output), Some(llm_ms)) if llm_ms > 0 => {
            format!("{:.1}", output as f64 / (llm_ms as f64 / 1000.0))
        }
        _ => l.unavailable.into(),
    };
    format!(
        "{} · {} successful model spans | LLM {} · {} {} | {} tok/s | cache {cache} | input {} · output {} · cached input {}",
        l.turn.replace("{n}", &stats.turns.to_string()),
        trace.successful_model_spans,
        metric_text(&trace.llm_ms, trace.unreadable_trace_files, l, true),
        l.tools,
        format_duration_ms(stats.tool_ms),
        tok_s,
        metric_text(
            &trace.input_tokens,
            trace.unreadable_trace_files,
            l,
            false
        ),
        metric_text(
            &trace.output_tokens,
            trace.unreadable_trace_files,
            l,
            false
        ),
        metric_text(
            &trace.cached_input_tokens,
            trace.unreadable_trace_files,
            l,
            false
        ),
    )
}

fn usage_line(usage: &TrajectoryUsage) -> String {
    let mut line = format!(
        "round {} · in {} · out {}",
        usage.round,
        fmt_tokens(usage.input_tokens),
        fmt_tokens(usage.output_tokens)
    );
    if usage.input_tokens > 0 && usage.cached_input_tokens > 0 {
        let pct = (usage.cached_input_tokens as f64 * 100.0 / usage.input_tokens as f64).round();
        let _ = write!(line, " · cached {pct:.0}%");
    }
    if usage.reasoning_tokens > 0 {
        let _ = write!(line, " · reasoning {}", fmt_tokens(usage.reasoning_tokens));
    }
    line
}

struct GanttSeg {
    id: String,
    lane: &'static str,
    left_pct: f64,
    width_pct: f64,
    error: bool,
}

fn gantt_segments(snapshot: &TrajectorySnapshot, trace: &TraceExportSummary) -> Vec<GanttSeg> {
    let mut events: Vec<(String, &'static str, u64, u64, bool)> = Vec::new();
    for turn in &snapshot.turns {
        for (ci, cell) in turn.cells.iter().enumerate() {
            let lane = match cell.kind.as_str() {
                "user" => "input",
                "tool" => "tools",
                _ => continue,
            };
            let Some(start) = cell.ts.and_then(|ts| u64::try_from(ts).ok()) else {
                continue;
            };
            let duration = cell.duration_ms.unwrap_or(0).max(0) as u64;
            events.push((
                format!("t{}-c{ci}", turn.index),
                lane,
                start,
                start.saturating_add(duration),
                cell.is_error || cell.ok == Some(false),
            ));
        }
    }
    for (index, span) in trace.model_spans.iter().enumerate() {
        events.push((
            format!("model-span-{index}"),
            "model",
            span.start_unix_ms,
            span.end_unix_ms,
            span.status != SpanStatus::Ok,
        ));
    }
    if events.is_empty() {
        return Vec::new();
    }
    events.sort_by_key(|(_, lane, start, end, _)| (*start, *end, *lane));
    let min_start = events.iter().map(|event| event.2).min().unwrap_or(0);
    let max_end = events
        .iter()
        .map(|event| event.3.max(event.2))
        .max()
        .unwrap_or(min_start)
        .max(min_start.saturating_add(1));
    let elapsed = max_end.saturating_sub(min_start).max(1) as f64;
    events
        .into_iter()
        .map(|(id, lane, start, end, error)| {
            let mut left = start.saturating_sub(min_start) as f64 / elapsed * 100.0;
            left = left.clamp(0.0, 99.8);
            let raw_width = end.saturating_sub(start) as f64 / elapsed * 100.0;
            let width = raw_width.max(0.2).min(100.0 - left);
            GanttSeg {
                id,
                lane,
                left_pct: left,
                width_pct: width,
                error,
            }
        })
        .collect()
}

fn write_gantt(
    out: &mut String,
    snapshot: &TrajectorySnapshot,
    trace: &TraceExportSummary,
    l: &Labels,
) {
    let segs = gantt_segments(snapshot, trace);
    if segs.is_empty() {
        return;
    }
    let _ = write!(
        out,
        "<section class=\"timeline\">\n<h2>{}</h2>\n<div class=\"gantt\">\n",
        escape_html(l.timeline)
    );
    for (lane, label) in [
        ("input", l.input),
        ("model", l.model_lane),
        ("tools", l.tools),
    ] {
        let _ = write!(
            out,
            "<div class=\"lane\"><span class=\"lane-label\">{}</span><div class=\"track\">",
            escape_html(label)
        );
        for seg in segs.iter().filter(|seg| seg.lane == lane) {
            let class = if seg.error {
                format!("seg {} error", seg.lane)
            } else {
                format!("seg {}", seg.lane)
            };
            let _ = write!(
                out,
                "<a class=\"{class}\" href=\"#{}\" style=\"left:{:.2}%;width:{:.2}%\"></a>",
                escape_html(&seg.id),
                seg.left_pct,
                seg.width_pct
            );
        }
        out.push_str("</div></div>\n");
    }
    out.push_str("</div>\n</section>\n");
}

fn optional_u64(value: Option<u64>, l: &Labels) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| l.unavailable.into())
}

fn write_model_spans(out: &mut String, trace: &TraceExportSummary, l: &Labels) {
    if trace.model_spans.is_empty()
        && trace.malformed_model_spans == 0
        && trace.excluded_model_spans == 0
        && trace.unreadable_trace_files == 0
    {
        return;
    }
    let _ = write!(
        out,
        "<section class=\"turn model-spans\">\n<h2>{}</h2>\n",
        escape_html(l.model_spans)
    );
    if trace.malformed_model_spans > 0
        || trace.excluded_model_spans > 0
        || trace.unreadable_trace_files > 0
    {
        let _ = write!(
            out,
            "<p class=\"stats\">{} malformed model span(s); {} non-successful span(s) excluded from totals; {} unreadable trace file(s).</p>\n",
            trace.malformed_model_spans,
            trace.excluded_model_spans,
            trace.unreadable_trace_files
        );
    }
    for (index, span) in trace.model_spans.iter().enumerate() {
        let id = format!("model-span-{index}");
        let model = span.model.as_deref().unwrap_or(l.unknown_model);
        let duration = span
            .latency_ms
            .map(|value| format_duration_ms(value.min(i64::MAX as u64) as i64))
            .unwrap_or_else(|| l.unavailable.into());
        let _ = write!(
            out,
            "<article class=\"event\" id=\"{}\">\n<div class=\"event-head\"><span class=\"badge usage\">{}</span><span class=\"summary\">{}</span></div>\n<div class=\"kv\"><span>{} {}</span><span>{} {}</span><span>turn {}</span></div>\n<pre>input {} · output {} · cached input {}</pre>\n</article>\n",
            escape_html(&id),
            escape_html(l.model_lane),
            escape_html(model),
            escape_html(l.status),
            span.status.as_str(),
            escape_html(l.duration),
            escape_html(&duration),
            escape_html(&span.turn_id),
            optional_u64(span.input_tokens, l),
            optional_u64(span.output_tokens, l),
            optional_u64(span.cached_input_tokens, l),
        );
    }
    out.push_str("</section>\n");
}

fn write_pre(out: &mut String, label: &str, text: &str) {
    let _ = write!(
        out,
        "<div class=\"block-label\">{}</div>\n<pre>{}</pre>\n",
        escape_html(label),
        escape_html(text)
    );
}

fn write_cell(out: &mut String, turn: i64, index: usize, cell: &TrajectoryCell, l: &Labels) {
    let id = format!("t{turn}-c{index}");
    let kind = cell.kind.as_str();
    let badge_class = if cell.is_error || cell.ok == Some(false) {
        format!("badge {kind} error")
    } else {
        format!("badge {kind}")
    };
    let _ = write!(
        out,
        "<article class=\"event\" id=\"{}\">\n<div class=\"event-head\">\
         <span class=\"{badge_class}\">{}</span>\
         <span class=\"summary\">{}</span>\n</div>\n<div class=\"kv\">",
        escape_html(&id),
        escape_html(kind_label(kind, l)),
        escape_html(&cell.summary)
    );
    let _ = write!(
        out,
        "<span>{} {}</span>",
        escape_html(l.status),
        escape_html(cell_status(cell, l))
    );
    if let Some(ts) = cell.ts {
        let _ = write!(
            out,
            "<span>{} {}</span>",
            escape_html(l.timestamp),
            escape_html(&format_ts(ts))
        );
    }
    if let Some(ms) = cell.duration_ms {
        let _ = write!(
            out,
            "<span>{} {}</span>",
            escape_html(l.duration),
            escape_html(&format_duration_ms(ms))
        );
    }
    out.push_str("</div>\n");
    match kind {
        "tool" => {
            if let Some(input) = cell.detail_input.as_deref() {
                write_pre(out, l.arguments, input);
            }
            if let Some(output) = cell.detail_output.as_deref() {
                write_pre(out, l.result, output);
            }
        }
        "usage" => {
            if let Some(usage) = &cell.usage {
                let mut body = usage_line(usage);
                if let Some(model) = &usage.model {
                    let _ = write!(body, "\nmodel {model}");
                }
                write_pre(out, l.usage, &body);
            }
        }
        _ => {
            if let Some(text) = cell
                .detail_output
                .as_deref()
                .filter(|text| !text.trim().is_empty())
            {
                write_pre(out, kind_label(kind, l), text);
            }
        }
    }
    out.push_str("</article>\n");
}

/// Build a self-contained HTML document for the folded trajectory.
#[derive(Serialize)]
struct RawExport<'a> {
    trajectory: &'a TrajectorySnapshot,
    trace: &'a TraceExportSummary,
}

pub(crate) fn render_native_trajectory_html(
    snapshot: &TrajectorySnapshot,
    locale: &str,
    exported_at: &str,
) -> String {
    render_trajectory_html(
        snapshot,
        &TraceExportSummary::default(),
        locale,
        exported_at,
    )
}

fn render_trajectory_html(
    snapshot: &TrajectorySnapshot,
    trace: &TraceExportSummary,
    locale: &str,
    exported_at: &str,
) -> String {
    let l = labels(locale);
    let model = snapshot
        .model
        .as_deref()
        .filter(|model| !model.is_empty())
        .unwrap_or(l.unknown_model);
    let mut out = String::with_capacity(8192);
    let _ = write!(
        out,
        "<!doctype html>\n<html lang=\"{}\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{} · {}</title>\n<style>\n{EXPORT_CSS}\n</style>\n</head>\n<body>\n<main>\n\
         <header class=\"meta\">\n<h1>{}</h1>\n<div class=\"meta-line\">\
         <span>{} <strong>{}</strong></span>\
         <span>{} <strong>{}</strong></span>\
         <span>{} <strong>{}</strong></span></div>\n<p class=\"stats\">{}</p>\n</header>\n",
        escape_html(l.lang),
        escape_html(l.title),
        escape_html(&snapshot.frame_id),
        escape_html(l.title),
        escape_html(l.session),
        escape_html(&snapshot.frame_id),
        escape_html(l.model),
        escape_html(model),
        escape_html(l.exported),
        escape_html(exported_at),
        escape_html(&stats_line(&snapshot.stats, trace, &l)),
    );
    write_gantt(&mut out, snapshot, trace, &l);
    write_model_spans(&mut out, trace, &l);
    if snapshot.turns.is_empty() {
        let _ = write!(out, "<p class=\"empty\">{}</p>\n", escape_html(l.empty));
    } else {
        for turn in &snapshot.turns {
            let heading = l.turn.replace("{n}", &turn.index.to_string());
            let _ = write!(
                out,
                "<section class=\"turn\" id=\"turn-{}\">\n<h2>{}</h2>\n",
                turn.index,
                escape_html(&heading)
            );
            for (ci, cell) in turn.cells.iter().enumerate() {
                write_cell(&mut out, turn.index, ci, cell, &l);
            }
            out.push_str("</section>\n");
        }
    }
    let raw = serde_json::to_string_pretty(&RawExport {
        trajectory: snapshot,
        trace,
    })
    .unwrap_or_else(|_| "{}".into());
    let _ = write!(
        out,
        "<details class=\"raw\"><summary>{}</summary>\n<pre>{}</pre>\n</details>\n\
         </main>\n</body>\n</html>\n",
        escape_html(l.raw),
        escape_html(&raw)
    );
    out
}

/// Reload the persisted trajectory and save it as HTML via the native dialog.
/// Returns the saved path, or `None` when the user cancels.
#[tauri::command]
pub(super) async fn export_session_trajectory(
    app: AppHandle,
    state: State<'_, AppState>,
    frame_id: String,
    locale: Option<String>,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    if frame_id.trim().is_empty() {
        return Err("No session to export.".into());
    }
    let messages = state
        .store
        .load_messages_with_seq(&frame_id)
        .await
        .map_err(|error| error.to_string())?;
    let events = state
        .store
        .load_session_ui_events_timed(&frame_id)
        .await
        .map_err(|error| error.to_string())?;
    let model = state
        .store
        .frame_model(&frame_id)
        .await
        .map_err(|error| error.to_string())?;
    let snapshot = fold_trajectory(&frame_id, model, &messages, &events);
    let project_root = crate::exploration_commands::working_project_for_frame(&state, &frame_id)
        .await?
        .0
        .root;
    let documents = read_trace_documents(&project_root).await;
    let trace = trace_summary_from_documents(&frame_id, documents);
    let locale = locale.unwrap_or_else(|| "en".into());
    let exported_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let html = render_trajectory_html(&snapshot, &trace, &locale, &exported_at);
    let default_name = trajectory_file_name(&frame_id);
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_file_name(&default_name)
        .save_file(move |path| {
            let _ = tx.send(path);
        });
    let Some(dest) = rx.await.map_err(|e| format!("{e}"))? else {
        return Ok(None);
    };
    let dest_path = std::path::PathBuf::from(dest.to_string());
    tokio::fs::write(&dest_path, html)
        .await
        .map_err(|e| format!("write failed: {e}"))?;
    Ok(Some(dest_path.to_string_lossy().into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trajectory::{TrajectoryTurn, TrajectoryUsage};

    fn cell(kind: &str, summary: &str) -> TrajectoryCell {
        TrajectoryCell {
            kind: kind.into(),
            summary: summary.into(),
            ..Default::default()
        }
    }

    fn sample_snapshot() -> TrajectorySnapshot {
        TrajectorySnapshot {
            frame_id: "sess-42".into(),
            model: Some("deepseek-v4-pro".into()),
            turns: vec![TrajectoryTurn {
                index: 1,
                started_at: Some(1_755_000_000_000),
                cells: vec![
                    TrajectoryCell {
                        kind: "user".into(),
                        summary: "Analyze fixture data".into(),
                        detail_output: Some(
                            "Analyze the fixture dataset\nwith fences ```md```".into(),
                        ),
                        ts: Some(1_755_000_000_000),
                        ..Default::default()
                    },
                    TrajectoryCell {
                        kind: "tool".into(),
                        summary: "python · describe".into(),
                        detail_input: Some(r#"{"code":"df.describe()"}"#.into()),
                        detail_output: Some(
                            "count  612.0\n```html\n</pre><script>alert(1)</script>\n```".into(),
                        ),
                        ok: Some(true),
                        ts: Some(1_755_000_002_000),
                        duration_ms: Some(3400),
                        ..Default::default()
                    },
                    TrajectoryCell {
                        kind: "tool".into(),
                        summary: "python · boom".into(),
                        detail_input: Some(r#"{"code":"1/0"}"#.into()),
                        detail_output: Some("ZeroDivisionError".into()),
                        ok: Some(false),
                        is_error: true,
                        ts: Some(1_755_000_006_000),
                        duration_ms: Some(80),
                        ..Default::default()
                    },
                    TrajectoryCell {
                        kind: "assistant".into(),
                        summary: "Here is the answer".into(),
                        detail_output: Some(
                            "Here is the full answer that is longer than the preview.".into(),
                        ),
                        ts: Some(1_755_000_007_000),
                        duration_ms: Some(1200),
                        ..Default::default()
                    },
                    TrajectoryCell {
                        kind: "usage".into(),
                        summary: "round 1".into(),
                        ts: Some(1_755_000_008_000),
                        usage: Some(TrajectoryUsage {
                            round: 1,
                            model: Some("deepseek-v4-pro".into()),
                            input_tokens: 12300,
                            output_tokens: 1400,
                            reasoning_tokens: 300,
                            cached_input_tokens: 9225,
                        }),
                        ..Default::default()
                    },
                ],
            }],
            stats: TrajectoryStats {
                turns: 1,
                steps: 1,
                llm_ms: 3300,
                tool_ms: 3480,
                input_tokens: 12300,
                output_tokens: 1400,
                cached_input_tokens: 9225,
                cache_hit_pct: Some(75.0),
                tokens_per_sec: Some(12.5),
            },
        }
    }

    fn sample_trace() -> TraceExportSummary {
        TraceExportSummary {
            model_spans: vec![ExportModelSpan {
                span_id: "model-1".into(),
                turn_id: "turn-1".into(),
                model: Some("deepseek-v4-pro".into()),
                status: SpanStatus::Ok,
                start_unix_ms: 1_755_000_003_200,
                end_unix_ms: 1_755_000_006_500,
                latency_ms: Some(3300),
                input_tokens: Some(12300),
                output_tokens: Some(1400),
                cached_input_tokens: Some(9225),
            }],
            successful_model_spans: 1,
            llm_ms: TraceMetric::from_values([Some(3300)]),
            input_tokens: TraceMetric::from_values([Some(12300)]),
            output_tokens: TraceMetric::from_values([Some(1400)]),
            cached_input_tokens: TraceMetric::from_values([Some(9225)]),
            ..Default::default()
        }
    }

    fn model_span_json(
        span_id: &str,
        turn_id: &str,
        status: &str,
        start: u64,
        end: u64,
        latency: Option<u64>,
        input: Option<u64>,
        output: Option<u64>,
        cached: Option<u64>,
    ) -> String {
        serde_json::json!({
            "format": TRACE_FORMAT,
            "format_version": 1,
            "record_type": "span",
            "trace_id": format!("trace-{turn_id}"),
            "span_id": span_id,
            "run_id": "run-1",
            "turn_id": turn_id,
            "session_id": "session-fixture",
            "kind": "model",
            "name": "agent.model",
            "status": status,
            "start_unix_ms": start,
            "end_unix_ms": end,
            "latency_ms": latency,
            "attributes": {
                "component": "test",
                "retry_count": 0,
                "model_id": "fixture-model",
                "input_tokens": input,
                "output_tokens": output,
                "cached_input_tokens": cached
            }
        })
        .to_string()
    }

    fn non_model_span_json(span_id: &str, turn_id: &str, kind: &str, name: &str) -> String {
        serde_json::json!({
            "format": TRACE_FORMAT,
            "format_version": TRACE_FORMAT_VERSION,
            "record_type": "span",
            "trace_id": format!("trace-{turn_id}"),
            "span_id": span_id,
            "run_id": "run-1",
            "turn_id": turn_id,
            "session_id": "session-fixture",
            "kind": kind,
            "name": name,
            "status": "ok",
            "start_unix_ms": 1_700,
            "end_unix_ms": 1_800,
            "latency_ms": 100,
            "attributes": {"component": "test"}
        })
        .to_string()
    }

    #[test]
    fn two_turn_trace_reconciles_model_latency_tokens_and_resumed_spans() {
        let turn_one = [
            model_span_json(
                "model-1",
                "turn-before-question",
                "ok",
                1_000,
                1_500,
                Some(500),
                Some(100),
                Some(10),
                Some(40),
            ),
            serde_json::json!({
                "format": TRACE_FORMAT,
                "format_version": 1,
                "record_type": "span",
                "trace_id": "trace-turn-before-question",
                "span_id": "ask-user",
                "run_id": "run-1",
                "turn_id": "turn-before-question",
                "session_id": "session-fixture",
                "kind": "approval",
                "name": "ask_user",
                "status": "blocked",
                "start_unix_ms": 1_600,
                "end_unix_ms": 1_700,
                "latency_ms": 100,
                "attributes": {"component": "test"}
            })
            .to_string(),
            non_model_span_json("tool-1", "turn-before-question", "tool", "read"),
            non_model_span_json("mcp-1", "turn-before-question", "mcp", "query"),
        ]
        .join("\n");
        let turn_two = [
            model_span_json(
                "model-2",
                "turn-after-question",
                "ok",
                2_000,
                2_750,
                Some(750),
                Some(200),
                Some(20),
                Some(100),
            ),
            model_span_json(
                "model-error",
                "turn-after-question",
                "error",
                2_100,
                2_300,
                Some(200),
                Some(999),
                Some(999),
                Some(999),
            ),
        ]
        .join("\n");

        let trace =
            trace_summary_from_jsonl("session-fixture", [turn_one.as_str(), turn_two.as_str()]);

        assert_eq!(trace.model_spans.len(), 3);
        assert_eq!(trace.successful_model_spans, 2);
        assert_eq!(trace.excluded_model_spans, 1);
        assert_eq!(trace.llm_ms.value, Some(1_250));
        assert_eq!(trace.input_tokens.value, Some(300));
        assert_eq!(trace.output_tokens.value, Some(30));
        assert_eq!(trace.cached_input_tokens.value, Some(140));
        assert_eq!(trace.model_spans[1].turn_id, "turn-after-question");

        let html = render_trajectory_html(&sample_snapshot(), &trace, "en", "t");
        assert_eq!(html.matches("class=\"seg model\"").count(), 2);
        assert_eq!(html.matches("class=\"seg model error\"").count(), 1);
        assert!(html.contains("&quot;known_subtotal&quot;: 1250"));
        assert!(html.contains("input 300 · output 30 · cached input 140"));
    }

    #[test]
    fn missing_or_malformed_model_metrics_are_explicitly_unavailable() {
        let complete = model_span_json(
            "complete",
            "turn-1",
            "ok",
            1_000,
            1_500,
            Some(500),
            Some(100),
            Some(10),
            Some(40),
        );
        let legacy = model_span_json(
            "legacy",
            "turn-2",
            "ok",
            2_000,
            2_600,
            None,
            Some(200),
            Some(20),
            None,
        );
        let malformed = serde_json::json!({
            "format": TRACE_FORMAT,
            "record_type": "span",
            "kind": "model",
            "session_id": "session-fixture"
        })
        .to_string();
        let missing_format = serde_json::json!({
            "record_type": "span",
            "kind": "model",
            "session_id": "session-fixture"
        })
        .to_string();
        let incompatible_format = serde_json::json!({
            "format": "wisp.agent-trace.v0",
            "format_version": TRACE_FORMAT_VERSION,
            "record_type": "span",
            "kind": "model",
            "session_id": "session-fixture"
        })
        .to_string();
        let incompatible_version = serde_json::json!({
            "format": TRACE_FORMAT,
            "format_version": TRACE_FORMAT_VERSION + 1,
            "record_type": "span",
            "kind": "model",
            "session_id": "session-fixture"
        })
        .to_string();
        let other_session = serde_json::json!({
            "record_type": "span",
            "kind": "model",
            "session_id": "another-session"
        })
        .to_string();
        let document = [
            complete,
            legacy,
            malformed,
            missing_format,
            incompatible_format,
            incompatible_version,
            other_session,
        ]
        .join("\n");

        let trace = trace_summary_from_jsonl("session-fixture", [document.as_str()]);

        assert_eq!(trace.malformed_model_spans, 4);
        assert_eq!(trace.llm_ms.value, None);
        assert_eq!(trace.llm_ms.known_subtotal, 500);
        assert_eq!(trace.llm_ms.omitted_spans, 1);
        assert_eq!(trace.input_tokens.value, Some(300));
        assert_eq!(trace.cached_input_tokens.value, None);
        assert_eq!(trace.cached_input_tokens.known_subtotal, 40);
        assert_eq!(trace.cached_input_tokens.omitted_spans, 1);

        let html = render_trajectory_html(&sample_snapshot(), &trace, "en", "t");
        assert!(html.contains("LLM unavailable (known subtotal 500ms; 1 omitted span(s))"));
        assert!(html.contains("cached input unavailable (known subtotal 40; 1 omitted span(s))"));
        assert!(!html.contains("LLM 0ms"));
    }

    #[tokio::test]
    async fn unreadable_trace_file_keeps_export_available_and_totals_fail_closed() {
        let root = std::env::temp_dir().join(format!(
            "wisp-trajectory-export-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let trace_dir = root.join(".wisp").join("traces");
        std::fs::create_dir_all(&trace_dir).unwrap();
        let readable_path = trace_dir.join("readable.jsonl");
        let unreadable_path = trace_dir.join("invalid-utf8.jsonl");
        std::fs::write(
            &readable_path,
            model_span_json(
                "complete",
                "turn-1",
                "ok",
                1_000,
                1_500,
                Some(500),
                Some(100),
                Some(10),
                Some(40),
            ),
        )
        .unwrap();
        std::fs::write(&unreadable_path, [0xff, 0xfe]).unwrap();

        let documents = read_trace_documents(&root).await;
        assert_eq!(documents.documents.len(), 1);
        assert_eq!(documents.unreadable_files, 1);
        let trace = trace_summary_from_documents("session-fixture", documents);
        assert_eq!(trace.model_spans.len(), 1);
        assert_eq!(trace.unreadable_trace_files, 1);
        assert_eq!(trace.llm_ms.value, None);
        assert_eq!(trace.llm_ms.known_subtotal, 500);

        let html = render_trajectory_html(&sample_snapshot(), &trace, "en", "t");
        assert!(html.contains("LLM unavailable (known subtotal 500ms; 1 unreadable trace file(s))"));
        assert!(html.contains("1 unreadable trace file(s)."));

        std::fs::remove_file(readable_path).unwrap();
        std::fs::remove_file(unreadable_path).unwrap();
        std::fs::remove_dir(trace_dir).unwrap();
        std::fs::remove_dir(root.join(".wisp")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn file_name_keeps_only_safe_components() {
        assert_eq!(
            trajectory_file_name("abc-123"),
            "wisp-trajectory-abc-123.html"
        );
        assert_eq!(
            trajectory_file_name("../../etc/passwd"),
            "wisp-trajectory-etc-passwd.html"
        );
        assert_eq!(trajectory_file_name(""), "wisp-trajectory.html");
        assert_eq!(trajectory_file_name(".."), "wisp-trajectory.html");
    }

    #[test]
    fn html_includes_session_model_stats_and_timeline() {
        let html = render_trajectory_html(
            &sample_snapshot(),
            &sample_trace(),
            "en",
            "2026-08-24T00:00:00Z",
        );
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("lang=\"en\""));
        assert!(html.contains("sess-42"));
        assert!(html.contains("deepseek-v4-pro"));
        assert!(html.contains("2026-08-24T00:00:00Z"));
        assert!(html.contains("Turn 1"));
        assert!(html.contains("424.2 tok/s"));
        assert!(html.contains("cache 75.0% (cache share = cached input / input)"));
        assert!(html.contains("class=\"gantt\""));
        assert!(html.contains("href=\"#t1-c1\""));
        assert!(html.contains("id=\"t1-c1\""));
        assert!(html.ends_with("</html>\n"));
    }

    #[test]
    fn html_keeps_full_payloads_and_escapes_hostile_tool_output() {
        let html = render_trajectory_html(&sample_snapshot(), &sample_trace(), "en", "t");
        assert!(html.contains("Analyze the fixture dataset"));
        assert!(html.contains("Here is the full answer that is longer than the preview."));
        assert!(html.contains(&escape_html(r#"{"code":"df.describe()"}"#)));
        assert!(html.contains("count  612.0"));
        assert!(html.contains("```html"));
        assert!(!html.contains("</pre><script>alert(1)</script>"));
        assert!(html.contains("&lt;/pre&gt;&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(html.contains("ZeroDivisionError"));
        assert!(html.contains("badge tool error"));
        let ts = chrono::DateTime::from_timestamp_millis(1_755_000_000_000)
            .unwrap()
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        assert!(html.contains(&ts));
        assert!(html.contains("round 1 · in 12.3k · out 1.4k · cached 75%"));
        assert!(html.contains("reasoning 300"));
        assert!(html.contains("Raw snapshot (JSON)"));
        assert!(html.contains("&quot;frame_id&quot;"));
    }

    #[test]
    fn zh_locale_uses_chinese_labels() {
        let html = render_trajectory_html(&sample_snapshot(), &sample_trace(), "zh", "t");
        assert!(html.contains("lang=\"zh\""));
        assert!(html.contains("轨迹"));
        assert!(html.contains("第 1 轮"));
        assert!(html.contains("参数"));
        assert!(html.contains("结果"));
        assert!(html.contains("时间线"));
        assert!(html.contains("原始快照（JSON）"));
    }

    #[test]
    fn unicode_summary_and_detail_are_exported_as_utf8_without_loss() {
        let text = "肝癌中的 GPX4 与 α/β？🧬";
        let mut snapshot = sample_snapshot();
        snapshot.turns[0].cells[0].summary = text.into();
        snapshot.turns[0].cells[0].detail_output = Some(text.into());

        let html = render_trajectory_html(&snapshot, &sample_trace(), "zh-CN", "t");
        assert!(html.contains("<meta charset=\"utf-8\">"));
        assert!(
            html.matches(text).count() >= 3,
            "summary, detail, and raw JSON must all preserve the same text"
        );
        assert!(!html.contains('\u{fffd}'));
        let bytes = html.into_bytes();
        assert!(std::str::from_utf8(&bytes).unwrap().contains(text));
    }

    #[test]
    fn empty_snapshot_still_produces_a_document() {
        let html = render_trajectory_html(
            &TrajectorySnapshot {
                frame_id: "empty".into(),
                ..Default::default()
            },
            &TraceExportSummary::default(),
            "en",
            "t",
        );
        assert!(html.contains("empty"));
        assert!(html.contains("No trajectory events."));
        assert!(!html.contains("class=\"gantt\""));
    }

    #[test]
    fn unused_preview_is_not_what_gets_exported() {
        let mut snap = sample_snapshot();
        snap.turns[0].cells[0].summary = "truncated…".into();
        let html = render_trajectory_html(&snap, &sample_trace(), "en", "t");
        assert!(html.contains("Analyze the fixture dataset"));
        assert!(html.contains("truncated…"));
    }

    #[test]
    fn gantt_skips_usage_and_marks_errors() {
        let segs = gantt_segments(&sample_snapshot(), &sample_trace());
        assert_eq!(segs.len(), 4);
        assert!(segs.iter().all(|seg| !seg.id.contains("c4")));
        assert!(segs.iter().any(|seg| seg.error && seg.lane == "tools"));
        assert!(segs.iter().any(|seg| seg.lane == "model"));
        assert!(segs.iter().all(|seg| {
            seg.left_pct >= 0.0 && seg.width_pct > 0.0 && seg.left_pct + seg.width_pct <= 100.0001
        }));
        assert!(segs
            .windows(2)
            .all(|pair| pair[0].left_pct <= pair[1].left_pct));
    }

    #[test]
    fn cell_helpers_cover_pending_tools() {
        let l = labels("en");
        let pending = cell("tool", "wait");
        assert_eq!(cell_status(&pending, &l), "Pending");
        assert_eq!(kind_label("unknown", &l), "unknown");
    }
}

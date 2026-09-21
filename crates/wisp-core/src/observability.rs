//! Agent observability: stable traces, parent-child spans, and local export.
//!
//! Desktop, CLI, eval, delegated, and resumed hosts share
//! [`host_agent_observability`] and [`AgentTrace`]. The default capture policy
//! records bounded operational attributes only — never prompts, tool arguments,
//! scientific rows, credentials, paths, or model output. Sensitive capture is
//! an explicit local opt-in (`WISP_TRACE_SENSITIVE=1`) with a stricter
//! retention window. Nothing is uploaded.

use crate::archive::{prune_dir, ArchiveRetention, DEFAULT_MAX_DIR_BYTES};
use crate::AgentLoopOutcome;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use wisp_tools::MCP_EVENT_PREFIX;

/// Document format id written on every trace header and span record.
pub const TRACE_FORMAT: &str = "wisp.agent-trace.v1";
/// Integer format version. Bump when the span/header shape changes.
pub const TRACE_FORMAT_VERSION: u32 = 1;
/// Shared host contract id. CLI, desktop, eval, and delegated children call
/// [`host_agent_observability`] rather than forking exporters.
pub const OBSERVABILITY_CONTRACT_ID: &str = "agent.observability.v1";

/// Default operational-trace retention (days). Same window as archive spills.
pub const TRACE_RETENTION_DAYS: u64 = 7;
/// Opt-in sensitive-capture retention. Shorter, and documented as such.
pub const SENSITIVE_TRACE_RETENTION_DAYS: u64 = 1;
/// Default directory budget for `.wisp/traces`.
pub const TRACE_RETENTION_BYTES: u64 = DEFAULT_MAX_DIR_BYTES;
/// Opt-in sensitive directory budget.
pub const SENSITIVE_TRACE_RETENTION_BYTES: u64 = 32 * 1024 * 1024;

const TRACES_DIR: &str = "traces";
const SENSITIVE_TRACES_DIR: &str = "traces-sensitive";
const SENSITIVE_ENV: &str = "WISP_TRACE_SENSITIVE";
const OTEL_JSON_ENV: &str = "WISP_TRACE_OTEL_JSON";

fn skip_empty_string(value: &Option<String>) -> bool {
    value.as_ref().is_none_or(|text| text.is_empty())
}

fn skip_empty_map<K, V>(value: &BTreeMap<K, V>) -> bool {
    value.is_empty()
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

fn new_trace_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn new_span_id() -> String {
    format!(
        "{:016x}",
        u64::from_be_bytes(
            uuid::Uuid::new_v4().as_bytes()[..8]
                .try_into()
                .expect("uuid has 8 bytes"),
        )
    )
}

/// Host that constructed this tracer. Recorded as the `component` attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservabilityHost {
    Cli,
    Desktop,
    Eval,
    Delegated,
    Test,
}

impl ObservabilityHost {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Desktop => "desktop",
            Self::Eval => "eval",
            Self::Delegated => "delegated",
            Self::Test => "test",
        }
    }
}

/// Kind of a recorded span. Stable names; hosts must not invent gene- or
/// ticket-specific kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanKind {
    Turn,
    Model,
    Tool,
    Mcp,
    Approval,
    Delegation,
    Workflow,
    Run,
    Retry,
    Compaction,
    Completion,
}

impl SpanKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Turn => "turn",
            Self::Model => "model",
            Self::Tool => "tool",
            Self::Mcp => "mcp",
            Self::Approval => "approval",
            Self::Delegation => "delegation",
            Self::Workflow => "workflow",
            Self::Run => "run",
            Self::Retry => "retry",
            Self::Compaction => "compaction",
            Self::Completion => "completion",
        }
    }
}

/// Terminal (or in-flight blocked) status. Retry and cancellation are
/// distinct: a retry hop is [`SpanKind::Retry`] plus [`Self::Error`] or
/// [`Self::Ok`]; a user/host stop is [`Self::Cancelled`]. External work
/// whose outcome is unknown is [`Self::Uncertain`], never [`Self::Ok`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanStatus {
    Ok,
    Error,
    Cancelled,
    Blocked,
    Uncertain,
}

impl SpanStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Error => "error",
            Self::Cancelled => "cancelled",
            Self::Blocked => "blocked",
            Self::Uncertain => "uncertain",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorClass {
    Cancelled,
    RetryExhausted,
    ToolSchema,
    McpStaleContract,
    McpReconnect,
    Provider,
    Compaction,
    Timeout,
    ApprovalDenied,
    Unknown,
}

impl ErrorClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cancelled => "cancelled",
            Self::RetryExhausted => "retry_exhausted",
            Self::ToolSchema => "tool_schema",
            Self::McpStaleContract => "mcp_stale_contract",
            Self::McpReconnect => "mcp_reconnect",
            Self::Provider => "provider",
            Self::Compaction => "compaction",
            Self::Timeout => "timeout",
            Self::ApprovalDenied => "approval_denied",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheOutcome {
    Hit,
    Miss,
    Disabled,
}

impl CacheOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hit => "hit",
            Self::Miss => "miss",
            Self::Disabled => "disabled",
        }
    }
}

/// Serializable identity that pause/resume, delegated children, and persisted
/// Run monitoring restore so causality survives a yield.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceContext {
    pub format: String,
    pub format_version: u32,
    pub trace_id: String,
    pub run_id: String,
    pub turn_id: String,
    pub span_id: String,
    #[serde(default, skip_serializing_if = "skip_empty_string")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "skip_empty_string")]
    pub parent_span_id: Option<String>,
}

impl TraceContext {
    fn fresh(_host: ObservabilityHost, identity: &TurnIdentity) -> Self {
        let parent = identity.parent.clone();
        Self {
            format: TRACE_FORMAT.into(),
            format_version: TRACE_FORMAT_VERSION,
            trace_id: parent
                .as_ref()
                .map(|ctx| ctx.trace_id.clone())
                .unwrap_or_else(new_trace_id),
            run_id: identity
                .run_id
                .clone()
                .or_else(|| parent.as_ref().map(|ctx| ctx.run_id.clone()))
                .unwrap_or_default(),
            turn_id: identity.turn_id.clone().unwrap_or_else(new_trace_id),
            span_id: String::new(),
            session_id: identity
                .session_id
                .clone()
                .or_else(|| parent.as_ref().and_then(|ctx| ctx.session_id.clone())),
            parent_span_id: parent.map(|ctx| ctx.span_id).filter(|id| !id.is_empty()),
        }
    }
}

/// Host-owned ids for one Agent request.
#[derive(Debug, Clone, Default)]
pub struct TurnIdentity {
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub run_id: Option<String>,
    pub project_id: Option<String>,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub parent: Option<TraceContext>,
}

/// Bounded operational attributes. Default serialization never carries
/// prompts, arguments, rows, credentials, paths, or model text.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SpanAttributes {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub component: String,
    #[serde(default, skip_serializing_if = "skip_empty_string")]
    pub tool_id: Option<String>,
    #[serde(default, skip_serializing_if = "skip_empty_string")]
    pub capability_id: Option<String>,
    #[serde(default, skip_serializing_if = "skip_empty_string")]
    pub provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "skip_empty_string")]
    pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "skip_empty_string")]
    pub contract_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "skip_empty_string")]
    pub contract_version: Option<String>,
    pub retry_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_outcome: Option<CacheOutcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_class: Option<ErrorClass>,
    #[serde(default, skip_serializing_if = "skip_empty_map")]
    pub extras: BTreeMap<String, String>,
}

/// One recorded span. `attributes` is the only payload; default capture
/// strips anything not on the operational allow-list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub format: String,
    pub format_version: u32,
    pub record_type: String,
    pub trace_id: String,
    pub span_id: String,
    #[serde(default, skip_serializing_if = "skip_empty_string")]
    pub parent_span_id: Option<String>,
    pub run_id: String,
    pub turn_id: String,
    #[serde(default, skip_serializing_if = "skip_empty_string")]
    pub session_id: Option<String>,
    pub kind: SpanKind,
    pub name: String,
    pub status: SpanStatus,
    pub start_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_unix_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    pub attributes: SpanAttributes,
}

impl Span {
    fn open(ctx: &TraceContext, kind: SpanKind, name: String, component: &str) -> Self {
        Self {
            format: TRACE_FORMAT.into(),
            format_version: TRACE_FORMAT_VERSION,
            record_type: "span".into(),
            trace_id: ctx.trace_id.clone(),
            span_id: new_span_id(),
            parent_span_id: if ctx.span_id.is_empty() {
                ctx.parent_span_id.clone()
            } else {
                Some(ctx.span_id.clone())
            },
            run_id: ctx.run_id.clone(),
            turn_id: ctx.turn_id.clone(),
            session_id: ctx.session_id.clone(),
            kind,
            name,
            status: SpanStatus::Uncertain,
            start_unix_ms: now_unix_ms(),
            end_unix_ms: None,
            latency_ms: None,
            attributes: SpanAttributes {
                component: component.into(),
                contract_fingerprint: Some(contract_fingerprint(&[
                    TRACE_FORMAT,
                    &TRACE_FORMAT_VERSION.to_string(),
                    OBSERVABILITY_CONTRACT_ID,
                ])),
                contract_version: Some(TRACE_FORMAT_VERSION.to_string()),
                ..SpanAttributes::default()
            },
        }
    }
}

/// Header written beside span records so a file is self-describing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraceHeader {
    pub record_type: String,
    pub format: String,
    pub format_version: u32,
    pub contract: String,
    pub trace_id: String,
    pub run_id: String,
    pub turn_id: String,
    #[serde(default, skip_serializing_if = "skip_empty_string")]
    pub session_id: Option<String>,
    pub sensitive_capture: bool,
    pub retention: RetentionPolicy,
}

/// Retention window copied onto every exported document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetentionPolicy {
    pub max_age_days: u64,
    pub max_bytes: u64,
    pub directory: String,
}

impl RetentionPolicy {
    pub fn operational() -> Self {
        Self {
            max_age_days: TRACE_RETENTION_DAYS,
            max_bytes: TRACE_RETENTION_BYTES,
            directory: format!(".wisp/{TRACES_DIR}"),
        }
    }

    pub fn sensitive() -> Self {
        Self {
            max_age_days: SENSITIVE_TRACE_RETENTION_DAYS,
            max_bytes: SENSITIVE_TRACE_RETENTION_BYTES,
            directory: format!(".wisp/{SENSITIVE_TRACES_DIR}"),
        }
    }

    pub fn archive(self) -> ArchiveRetention {
        ArchiveRetention {
            max_age_days: self.max_age_days,
            max_bytes: self.max_bytes,
        }
    }
}

/// Local capture policy. Sensitive content is off unless the host opts in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapturePolicy {
    pub include_sensitive: bool,
}

impl Default for CapturePolicy {
    fn default() -> Self {
        Self {
            include_sensitive: false,
        }
    }
}

impl CapturePolicy {
    pub fn from_env() -> Self {
        Self {
            include_sensitive: env_flag(SENSITIVE_ENV),
        }
    }

    pub fn retention(self) -> RetentionPolicy {
        if self.include_sensitive {
            RetentionPolicy::sensitive()
        } else {
            RetentionPolicy::operational()
        }
    }

    fn allow_extra(self, key: &str) -> bool {
        if self.include_sensitive {
            return true;
        }
        OPERATIONAL_EXTRA_KEYS.contains(&key)
    }
}

const OPERATIONAL_EXTRA_KEYS: &[&str] = &[
    "stop_reason",
    "finish_reason",
    "connector_id",
    "workflow_id",
    "step_id",
    "schema_valid",
    "mcp_stale_contract",
    "reconnect",
    "first_progress_ms",
    "context_tokens_before",
    "context_tokens_after",
    "compaction_strategy",
    "repeated_call",
    "host",
];

fn env_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

/// SHA-256 prefix of stable contract parts (format, version, tool schema).
pub fn contract_fingerprint(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    let digest = hasher.finalize();
    hex_encode(&digest[..8])
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Local span sink. Implementations must not open a network connection.
pub trait SpanExporter: Send + Sync {
    fn export_header(&self, header: &TraceHeader);
    fn export_span(&self, span: &Span);
}

/// In-process exporter for tests and SLO aggregation. No collector.
#[derive(Debug, Default)]
pub struct MemoryExporter {
    header: Mutex<Option<TraceHeader>>,
    spans: Mutex<Vec<Span>>,
}

impl MemoryExporter {
    pub fn header(&self) -> Option<TraceHeader> {
        self.header
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub fn spans(&self) -> Vec<Span> {
        self.spans.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    pub fn document(&self) -> TraceDocument {
        TraceDocument {
            header: self.header().unwrap_or(TraceHeader {
                record_type: "trace".into(),
                format: TRACE_FORMAT.into(),
                format_version: TRACE_FORMAT_VERSION,
                contract: OBSERVABILITY_CONTRACT_ID.into(),
                trace_id: String::new(),
                run_id: String::new(),
                turn_id: String::new(),
                session_id: None,
                sensitive_capture: false,
                retention: RetentionPolicy::operational(),
            }),
            spans: self.spans(),
        }
    }
}

impl SpanExporter for MemoryExporter {
    fn export_header(&self, header: &TraceHeader) {
        *self.header.lock().unwrap_or_else(|p| p.into_inner()) = Some(header.clone());
    }

    fn export_span(&self, span: &Span) {
        self.spans
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(span.clone());
    }
}

/// JSONL writer under `.wisp/traces` (or `traces-sensitive`). Never uploads.
pub struct FileExporter {
    dir: PathBuf,
    retention: RetentionPolicy,
}

impl FileExporter {
    pub fn new(project_root: &Path, retention: RetentionPolicy) -> Self {
        let leaf = if retention.directory.contains("sensitive") {
            SENSITIVE_TRACES_DIR
        } else {
            TRACES_DIR
        };
        Self {
            dir: project_root.join(".wisp").join(leaf),
            retention,
        }
    }

    fn path_for(&self, trace_id: &str) -> PathBuf {
        self.dir.join(format!("{trace_id}.jsonl"))
    }

    fn append_json(&self, trace_id: &str, value: &impl Serialize) {
        if std::fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        let Ok(line) = serde_json::to_string(value) else {
            return;
        };
        let path = self.path_for(trace_id);
        let mut body = if path.exists() {
            std::fs::read_to_string(&path).unwrap_or_default()
        } else {
            String::new()
        };
        body.push_str(&line);
        body.push('\n');
        let _ = std::fs::write(&path, body);
        prune_dir(&self.dir, self.retention.clone().archive());
    }
}

impl SpanExporter for FileExporter {
    fn export_header(&self, header: &TraceHeader) {
        self.append_json(&header.trace_id, header);
    }

    fn export_span(&self, span: &Span) {
        self.append_json(&span.trace_id, span);
    }
}

/// OpenTelemetry-shaped JSON written to a local path. No collector, no network.
pub struct OtelJsonExporter {
    path: PathBuf,
    spans: Mutex<Vec<Span>>,
}

impl OtelJsonExporter {
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            spans: Mutex::new(Vec::new()),
        }
    }

    fn flush(&self, spans: &[Span]) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let doc = otel_document(spans);
        if let Ok(json) = serde_json::to_string_pretty(&doc) {
            let _ = std::fs::write(&self.path, json);
        }
    }
}

impl SpanExporter for OtelJsonExporter {
    fn export_header(&self, _header: &TraceHeader) {}

    fn export_span(&self, span: &Span) {
        let mut spans = self.spans.lock().unwrap_or_else(|p| p.into_inner());
        spans.push(span.clone());
        self.flush(&spans);
    }
}

fn otel_document(spans: &[Span]) -> serde_json::Value {
    serde_json::json!({
        "resourceSpans": [{
            "resource": {
                "attributes": [
                    {"key": "service.name", "value": {"stringValue": "wisp-science"}},
                    {"key": "telemetry.sdk.language", "value": {"stringValue": "rust"}},
                ]
            },
            "scopeSpans": [{
                "scope": {
                    "name": OBSERVABILITY_CONTRACT_ID,
                    "version": TRACE_FORMAT_VERSION.to_string(),
                },
                "spans": spans.iter().map(otel_span).collect::<Vec<_>>(),
            }]
        }]
    })
}

fn otel_span(span: &Span) -> serde_json::Value {
    serde_json::json!({
        "traceId": span.trace_id,
        "spanId": span.span_id,
        "parentSpanId": span.parent_span_id,
        "name": span.name,
        "kind": span.kind.as_str(),
        "startTimeUnixNano": span.start_unix_ms.saturating_mul(1_000_000),
        "endTimeUnixNano": span.end_unix_ms.unwrap_or(span.start_unix_ms).saturating_mul(1_000_000),
        "status": {"code": span.status.as_str()},
        "attributes": otel_attributes(&span.attributes),
    })
}

fn otel_attributes(attrs: &SpanAttributes) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    let mut push = |key: &str, value: serde_json::Value| {
        out.push(serde_json::json!({"key": key, "value": value}));
    };
    if !attrs.component.is_empty() {
        push(
            "component",
            serde_json::json!({"stringValue": attrs.component}),
        );
    }
    if let Some(tool) = &attrs.tool_id {
        push("tool_id", serde_json::json!({"stringValue": tool}));
    }
    if let Some(provider) = &attrs.provider_id {
        push("provider_id", serde_json::json!({"stringValue": provider}));
    }
    if let Some(model) = &attrs.model_id {
        push("model_id", serde_json::json!({"stringValue": model}));
    }
    push(
        "retry_count",
        serde_json::json!({"intValue": attrs.retry_count.to_string()}),
    );
    if let Some(class) = attrs.error_class {
        push(
            "error_class",
            serde_json::json!({"stringValue": class.as_str()}),
        );
    }
    out
}

struct CompositeExporter {
    inner: Vec<Arc<dyn SpanExporter>>,
}

impl SpanExporter for CompositeExporter {
    fn export_header(&self, header: &TraceHeader) {
        for exporter in &self.inner {
            exporter.export_header(header);
        }
    }

    fn export_span(&self, span: &Span) {
        for exporter in &self.inner {
            exporter.export_span(span);
        }
    }
}

/// Configuration for the shared host constructor.
#[derive(Debug, Clone)]
pub struct HostObservabilityConfig {
    pub host: ObservabilityHost,
    pub project_root: Option<PathBuf>,
    pub identity: TurnIdentity,
    pub capture: CapturePolicy,
    pub otel_json_path: Option<PathBuf>,
}

impl HostObservabilityConfig {
    pub fn for_host(host: ObservabilityHost, project_root: impl Into<PathBuf>) -> Self {
        Self {
            host,
            project_root: Some(project_root.into()),
            identity: TurnIdentity::default(),
            capture: CapturePolicy::from_env(),
            otel_json_path: std::env::var(OTEL_JSON_ENV)
                .ok()
                .filter(|v| !v.is_empty())
                .map(PathBuf::from),
        }
    }

    pub fn memory(host: ObservabilityHost) -> Self {
        Self {
            host,
            project_root: None,
            identity: TurnIdentity::default(),
            capture: CapturePolicy::default(),
            otel_json_path: None,
        }
    }

    pub fn with_session(mut self, session_id: impl AsRef<str>) -> Self {
        self.identity.session_id = Some(session_id.as_ref().to_string());
        self
    }

    pub fn with_turn(mut self, turn_id: impl AsRef<str>) -> Self {
        self.identity.turn_id = Some(turn_id.as_ref().to_string());
        self
    }

    pub fn with_run(mut self, run_id: impl AsRef<str>) -> Self {
        self.identity.run_id = Some(run_id.as_ref().to_string());
        self
    }

    pub fn with_parent(mut self, parent: TraceContext) -> Self {
        self.identity.parent = Some(parent);
        self
    }
}

/// Shared host entry point. CLI, desktop, eval, and delegated children call
/// this instead of constructing exporters themselves.
pub fn host_agent_observability(config: HostObservabilityConfig) -> AgentTrace {
    let mut exporters: Vec<Arc<dyn SpanExporter>> = Vec::new();
    let memory = Arc::new(MemoryExporter::default());
    exporters.push(memory.clone());
    if let Some(root) = &config.project_root {
        exporters.push(Arc::new(FileExporter::new(
            root,
            config.capture.retention(),
        )));
    }
    if let Some(path) = config.otel_json_path {
        exporters.push(Arc::new(OtelJsonExporter::file(path)));
    }
    AgentTrace::new(
        Arc::new(CompositeExporter { inner: exporters }),
        config.capture,
        config.host,
        config.identity,
        Some(memory),
    )
}

/// Cloneable handle held by CLI/desktop/eval [`crate::Output`] implementations.
#[derive(Clone)]
pub struct AgentTrace {
    inner: Arc<TraceInner>,
}

struct TraceInner {
    context: Mutex<TraceContext>,
    exporter: Arc<dyn SpanExporter>,
    capture: CapturePolicy,
    host: ObservabilityHost,
    memory: Option<Arc<MemoryExporter>>,
    header_written_for: Mutex<Option<String>>,
}

impl AgentTrace {
    fn new(
        exporter: Arc<dyn SpanExporter>,
        capture: CapturePolicy,
        host: ObservabilityHost,
        identity: TurnIdentity,
        memory: Option<Arc<MemoryExporter>>,
    ) -> Self {
        Self {
            inner: Arc::new(TraceInner {
                context: Mutex::new(TraceContext::fresh(host, &identity)),
                exporter,
                capture,
                host,
                memory,
                header_written_for: Mutex::new(None),
            }),
        }
    }

    /// In-memory tracer for tests. No files, no collector, no network.
    pub fn in_memory() -> Self {
        host_agent_observability(HostObservabilityConfig::memory(ObservabilityHost::Test))
    }

    pub fn context(&self) -> TraceContext {
        self.inner
            .context
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub fn capture(&self) -> CapturePolicy {
        self.inner.capture
    }

    pub fn host(&self) -> ObservabilityHost {
        self.inner.host
    }

    pub fn memory(&self) -> Option<&MemoryExporter> {
        self.inner.memory.as_deref()
    }

    /// Continue a paused turn (approval yield, host resume, Run monitor).
    pub fn resume_from(&self, parent: TraceContext) {
        let mut ctx = self.inner.context.lock().unwrap_or_else(|p| p.into_inner());
        *ctx = parent;
    }

    /// Child work on the same trace, new session id. Unrelated sessions stay
    /// distinct; parent_span_id retains causality.
    pub fn delegate_session(&self, child_session_id: &str) -> AgentTrace {
        let parent = self.context();
        let child_identity = TurnIdentity {
            session_id: Some(child_session_id.into()),
            turn_id: Some(new_trace_id()),
            run_id: if parent.run_id.is_empty() {
                None
            } else {
                Some(parent.run_id.clone())
            },
            parent: Some(parent),
            ..TurnIdentity::default()
        };
        AgentTrace::new(
            self.inner.exporter.clone(),
            self.inner.capture,
            ObservabilityHost::Delegated,
            child_identity,
            self.inner.memory.clone(),
        )
    }

    pub fn start_turn(&self, identity: TurnIdentity) -> SpanGuard {
        {
            let mut ctx = self.inner.context.lock().unwrap_or_else(|p| p.into_inner());
            if identity.parent.is_some() {
                *ctx = TraceContext::fresh(self.inner.host, &identity);
            } else if ctx.parent_span_id.is_some() && !ctx.trace_id.is_empty() {
                if let Some(turn_id) = identity.turn_id.clone() {
                    ctx.turn_id = turn_id;
                }
                if let Some(session) = identity.session_id.clone() {
                    ctx.session_id = Some(session);
                }
                ctx.span_id.clear();
            } else {
                ctx.trace_id = new_trace_id();
                ctx.turn_id = identity.turn_id.clone().unwrap_or_else(new_trace_id);
                if let Some(session) = identity.session_id.clone() {
                    ctx.session_id = Some(session);
                }
                if let Some(run_id) = identity.run_id.clone() {
                    ctx.run_id = run_id;
                }
                ctx.parent_span_id = None;
                ctx.span_id.clear();
            }
        }
        self.write_header();
        let span = self.start_span(SpanKind::Turn, "agent.turn");
        span.set_str("host", self.inner.host.as_str());
        if let Some(provider) = identity.provider_id {
            span.set_provider(&provider);
        }
        if let Some(model) = identity.model_id {
            span.set_model(&model);
        }
        span
    }

    pub fn start_span(&self, kind: SpanKind, name: impl Into<String>) -> SpanGuard {
        let ctx = self.context();
        self.open_span(&ctx, kind, name.into())
    }

    fn write_header(&self) {
        let ctx = self.context();
        let mut written = self
            .inner
            .header_written_for
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if written.as_deref() == Some(ctx.trace_id.as_str()) {
            return;
        }
        self.inner.exporter.export_header(&TraceHeader {
            record_type: "trace".into(),
            format: TRACE_FORMAT.into(),
            format_version: TRACE_FORMAT_VERSION,
            contract: OBSERVABILITY_CONTRACT_ID.into(),
            trace_id: ctx.trace_id.clone(),
            run_id: ctx.run_id,
            turn_id: ctx.turn_id,
            session_id: ctx.session_id,
            sensitive_capture: self.inner.capture.include_sensitive,
            retention: self.inner.capture.retention(),
        });
        *written = Some(ctx.trace_id);
    }

    fn open_span(&self, ctx: &TraceContext, kind: SpanKind, name: String) -> SpanGuard {
        let span = Span::open(ctx, kind, name, self.inner.host.as_str());
        {
            let mut current = self.inner.context.lock().unwrap_or_else(|p| p.into_inner());
            current.span_id = span.span_id.clone();
            current.trace_id = span.trace_id.clone();
            current.turn_id = span.turn_id.clone();
        }
        SpanGuard {
            inner: Arc::new(Mutex::new(OpenSpan {
                span,
                ended: false,
                started: Instant::now(),
                exporter: self.inner.exporter.clone(),
                capture: self.inner.capture,
            })),
        }
    }
}

struct OpenSpan {
    span: Span,
    ended: bool,
    started: Instant,
    exporter: Arc<dyn SpanExporter>,
    capture: CapturePolicy,
}

/// RAII span. Dropping an unfinished span records [`SpanStatus::Uncertain`],
/// never success.
#[derive(Clone)]
pub struct SpanGuard {
    inner: Arc<Mutex<OpenSpan>>,
}

impl SpanGuard {
    pub fn span_id(&self) -> String {
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .span
            .span_id
            .clone()
    }

    pub fn trace_id(&self) -> String {
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .span
            .trace_id
            .clone()
    }

    pub fn parent_span_id(&self) -> Option<String> {
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .span
            .parent_span_id
            .clone()
    }

    pub fn context(&self) -> TraceContext {
        let open = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        TraceContext {
            format: TRACE_FORMAT.into(),
            format_version: TRACE_FORMAT_VERSION,
            trace_id: open.span.trace_id.clone(),
            run_id: open.span.run_id.clone(),
            turn_id: open.span.turn_id.clone(),
            span_id: open.span.span_id.clone(),
            session_id: open.span.session_id.clone(),
            parent_span_id: open.span.parent_span_id.clone(),
        }
    }

    pub fn child(&self, kind: SpanKind, name: impl Into<String>) -> SpanGuard {
        let open = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let ctx = TraceContext {
            format: TRACE_FORMAT.into(),
            format_version: TRACE_FORMAT_VERSION,
            trace_id: open.span.trace_id.clone(),
            run_id: open.span.run_id.clone(),
            turn_id: open.span.turn_id.clone(),
            span_id: open.span.span_id.clone(),
            session_id: open.span.session_id.clone(),
            parent_span_id: Some(open.span.span_id.clone()),
        };
        let mut span = Span::open(&ctx, kind, name.into(), &open.span.attributes.component);
        span.parent_span_id = Some(open.span.span_id.clone());
        SpanGuard {
            inner: Arc::new(Mutex::new(OpenSpan {
                span,
                ended: false,
                started: Instant::now(),
                exporter: open.exporter.clone(),
                capture: open.capture,
            })),
        }
    }

    pub fn set_str(&self, key: &str, value: &str) {
        let mut open = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        match key {
            "tool_id" => open.span.attributes.tool_id = Some(value.into()),
            "capability_id" => open.span.attributes.capability_id = Some(value.into()),
            "connector_id"
            | "host"
            | "stop_reason"
            | "finish_reason"
            | "workflow_id"
            | "step_id"
            | "schema_valid"
            | "mcp_stale_contract"
            | "reconnect"
            | "compaction_strategy"
            | "repeated_call" => {
                if open.capture.allow_extra(key) {
                    open.span.attributes.extras.insert(key.into(), value.into());
                }
            }
            other if open.capture.allow_extra(other) => {
                open.span
                    .attributes
                    .extras
                    .insert(other.into(), value.into());
            }
            _ => {}
        }
    }

    pub fn set_provider(&self, provider_id: &str) {
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .span
            .attributes
            .provider_id = Some(provider_id.into());
    }

    pub fn set_model(&self, model_id: &str) {
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .span
            .attributes
            .model_id = Some(model_id.into());
    }

    pub fn set_retry_count(&self, count: u32) {
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .span
            .attributes
            .retry_count = count;
    }

    pub fn set_tokens(&self, input: u64, output: u64, reasoning: u64, cached: u64) {
        let mut open = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        open.span.attributes.input_tokens = Some(input);
        open.span.attributes.output_tokens = Some(output);
        open.span.attributes.reasoning_tokens = Some(reasoning);
        open.span.attributes.cached_input_tokens = Some(cached);
        open.span.attributes.cache_outcome = Some(if cached > 0 {
            CacheOutcome::Hit
        } else {
            CacheOutcome::Miss
        });
    }

    pub fn set_error_class(&self, class: ErrorClass) {
        let mut open = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        if matches!(
            open.span.attributes.error_class,
            Some(ErrorClass::RetryExhausted)
        ) && class != ErrorClass::Cancelled
        {
            return;
        }
        open.span.attributes.error_class = Some(class);
    }

    pub fn mark_blocked(&self) {
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .span
            .status = SpanStatus::Blocked;
    }

    pub fn is_ended(&self) -> bool {
        self.inner.lock().unwrap_or_else(|p| p.into_inner()).ended
    }

    pub fn end(&self, status: SpanStatus) {
        let mut open = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        if open.ended {
            return;
        }
        open.ended = true;
        open.span.status = status;
        let end = now_unix_ms();
        open.span.end_unix_ms = Some(end);
        open.span.latency_ms = Some(open.started.elapsed().as_millis() as u64);
        let exported = redact_span(&open.span, open.capture);
        open.exporter.export_span(&exported);
    }
}

impl Drop for SpanGuard {
    fn drop(&mut self) {
        if Arc::strong_count(&self.inner) == 1 {
            self.end(SpanStatus::Uncertain);
        }
    }
}

fn redact_span(span: &Span, capture: CapturePolicy) -> Span {
    let mut span = span.clone();
    if capture.include_sensitive {
        return span;
    }
    span.attributes
        .extras
        .retain(|key, _| capture.allow_extra(key));
    span
}

/// Map a loop error into a terminal status. Cancellation is never a retry.
pub fn classify_loop_error(error: &str) -> (SpanStatus, ErrorClass) {
    let lower = error.to_ascii_lowercase();
    if lower.contains("stopped by user") || lower.contains("interrupted by user") {
        (SpanStatus::Cancelled, ErrorClass::Cancelled)
    } else if lower.contains("retry exhausted") {
        (SpanStatus::Error, ErrorClass::RetryExhausted)
    } else if lower.contains("schema") {
        (SpanStatus::Error, ErrorClass::ToolSchema)
    } else if lower.contains("stale") && lower.contains("mcp") {
        (SpanStatus::Error, ErrorClass::McpStaleContract)
    } else if lower.contains("reconnect") && lower.contains("mcp") {
        (SpanStatus::Error, ErrorClass::McpReconnect)
    } else if lower.contains("compaction") {
        (SpanStatus::Error, ErrorClass::Compaction)
    } else if lower.contains("timeout") {
        (SpanStatus::Error, ErrorClass::Timeout)
    } else {
        (SpanStatus::Error, ErrorClass::Unknown)
    }
}

pub fn finish_turn_span(span: &SpanGuard, result: &anyhow::Result<AgentLoopOutcome>) {
    match result {
        Ok(outcome) => {
            if let Some(reason) = outcome.stop_reason() {
                span.set_str("stop_reason", reason);
            }
            span.end(SpanStatus::Ok);
        }
        Err(error) => {
            let (status, class) = classify_loop_error(&format!("{error:#}"));
            span.set_error_class(class);
            span.end(status);
        }
    }
}

/// Decide tool vs run/workflow kind and whether an MCP child span is required.
pub fn plan_tool_spans(name: &str, event_name: &str) -> (SpanKind, bool) {
    let is_mcp = event_name.starts_with(MCP_EVENT_PREFIX)
        || name == "use_mcp_tool"
        || name == "search_mcp_tools";
    let kind = match name {
        "run_in_context" | "monitor_run" | "get_run" | "cancel_run" => SpanKind::Run,
        "run_workflow" | "create_workflow" | "explain_workflow" => SpanKind::Workflow,
        "delegate_tasks" => SpanKind::Delegation,
        _ => SpanKind::Tool,
    };
    (kind, is_mcp)
}

pub fn mcp_capability_id(event_name: &str) -> String {
    event_name
        .strip_prefix(MCP_EVENT_PREFIX)
        .unwrap_or(event_name)
        .to_string()
}

/// Closed trace used by SLO aggregation and tests.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraceDocument {
    pub header: TraceHeader,
    pub spans: Vec<Span>,
}

impl TraceDocument {
    pub fn tree_kinds(&self) -> Vec<SpanKind> {
        self.spans.iter().map(|span| span.kind).collect()
    }

    pub fn encoded(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// Initial Agent SLO snapshot derived from a closed trace. Counts only; no
/// payload text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentSloSnapshot {
    pub completion_rate: f64,
    pub blocked_rate: f64,
    pub error_rate: f64,
    pub time_to_first_progress_ms: Option<u64>,
    pub tool_schema_failure_rate: f64,
    pub mcp_stale_contract_rate: f64,
    pub approval_wait_ms: u64,
    pub context_growth_tokens: i64,
    pub repeated_call_rate: f64,
}

pub fn slo_from_document(doc: &TraceDocument) -> AgentSloSnapshot {
    let turns: Vec<_> = doc
        .spans
        .iter()
        .filter(|span| span.kind == SpanKind::Turn)
        .collect();
    let turn_n = turns.len().max(1) as f64;
    let completed = turns
        .iter()
        .filter(|span| span.status == SpanStatus::Ok)
        .count() as f64;
    let blocked = doc
        .spans
        .iter()
        .filter(|span| span.status == SpanStatus::Blocked || span.kind == SpanKind::Approval)
        .count() as f64;
    let errors = doc
        .spans
        .iter()
        .filter(|span| span.status == SpanStatus::Error)
        .count() as f64;
    let tools = doc
        .spans
        .iter()
        .filter(|span| span.kind == SpanKind::Tool)
        .count()
        .max(1) as f64;
    let schema_failures = doc
        .spans
        .iter()
        .filter(|span| span.attributes.error_class == Some(ErrorClass::ToolSchema))
        .count() as f64;
    let mcp_spans = doc
        .spans
        .iter()
        .filter(|span| span.kind == SpanKind::Mcp)
        .count()
        .max(1) as f64;
    let stale = doc
        .spans
        .iter()
        .filter(|span| span.attributes.error_class == Some(ErrorClass::McpStaleContract))
        .count() as f64;
    let approval_wait_ms = doc
        .spans
        .iter()
        .filter(|span| span.kind == SpanKind::Approval)
        .filter_map(|span| span.latency_ms)
        .sum();
    let first_progress = doc
        .spans
        .iter()
        .find(|span| matches!(span.kind, SpanKind::Model | SpanKind::Tool))
        .and_then(|span| span.latency_ms);
    let before = extra_i64(&doc.spans, "context_tokens_before");
    let after = extra_i64(&doc.spans, "context_tokens_after");
    let repeated = doc
        .spans
        .iter()
        .filter(|span| span.attributes.extras.get("repeated_call").is_some())
        .count() as f64;
    AgentSloSnapshot {
        completion_rate: completed / turn_n,
        blocked_rate: blocked / turn_n,
        error_rate: errors / turn_n,
        time_to_first_progress_ms: first_progress,
        tool_schema_failure_rate: schema_failures / tools,
        mcp_stale_contract_rate: stale / mcp_spans,
        approval_wait_ms,
        context_growth_tokens: after.saturating_sub(before),
        repeated_call_rate: repeated / tools,
    }
}

fn extra_i64(spans: &[Span], key: &str) -> i64 {
    spans
        .iter()
        .rev()
        .find_map(|span| span.attributes.extras.get(key)?.parse().ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::Output;
    use crate::{agent_loop, ContextManager};
    use async_trait::async_trait;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use wisp_llm::{
        scripted::ScriptedCompletion, scripted::ScriptedProvider, scripted::ScriptedToolCall,
        ToolSchema,
    };
    use wisp_tools::{Approval, ConfirmDecision, Registry, Tool, ToolEnv, ToolResult};

    const SECRET: &str = "sk-fixture-secret-do-not-export";
    const SCIENTIFIC_ROW: &str = "PTK7,Liver,0.91";
    const SECRET_PATH: &str = r"C:\Users\secret\cohort.csv";

    struct TracingOutput {
        trace: AgentTrace,
        approval: Approval,
        release: Option<Arc<tokio::sync::Notify>>,
        paused: Option<Arc<tokio::sync::Notify>>,
        decisions: std::sync::Mutex<Vec<bool>>,
    }

    impl TracingOutput {
        fn new(trace: AgentTrace) -> Self {
            Self {
                trace,
                approval: Approval::Allow,
                release: None,
                paused: None,
                decisions: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    impl Output for TracingOutput {
        fn agent_trace(&self) -> Option<&AgentTrace> {
            Some(&self.trace)
        }
        fn approval_mode(&self, _tool: &str) -> Approval {
            self.approval
        }
        fn confirm(&self, _message: &str) -> bool {
            self.decisions.lock().unwrap().pop().unwrap_or(true)
        }
        fn confirm_async<'a>(&'a self, message: &'a str) -> crate::OutputFuture<'a, bool> {
            Box::pin(async move {
                if let Some(paused) = &self.paused {
                    paused.notify_waiters();
                }
                if let Some(release) = &self.release {
                    release.notified().await;
                }
                self.confirm(message)
            })
        }
        fn confirm_decision_async<'a>(
            &'a self,
            message: &'a str,
        ) -> crate::OutputFuture<'a, ConfirmDecision> {
            Box::pin(async move {
                if self.confirm_async(message).await {
                    ConfirmDecision::Approved
                } else {
                    ConfirmDecision::Denied { feedback: None }
                }
            })
        }
    }

    struct NamedTool {
        name: &'static str,
        mcp: bool,
        runs: Arc<AtomicUsize>,
        result: String,
    }

    #[async_trait]
    impl Tool for NamedTool {
        fn name(&self) -> &str {
            self.name
        }
        fn schema(&self) -> ToolSchema {
            ToolSchema::new(
                self.name,
                "offline fake tool",
                serde_json::json!({"type": "object"}),
            )
        }
        fn defer_schema(&self) -> bool {
            self.mcp
        }
        fn connector_id(&self) -> Option<&str> {
            self.mcp.then_some("fake-mcp")
        }
        fn read_only(&self) -> bool {
            true
        }
        async fn run(&self, _args: &serde_json::Value, _env: &dyn ToolEnv) -> ToolResult {
            self.runs.fetch_add(1, Ordering::SeqCst);
            ToolResult::ok(&self.result)
        }
    }

    fn scripted_turn() -> ScriptedProvider {
        ScriptedProvider::new(
            "scripted-observability",
            vec![
                ScriptedCompletion {
                    tool_calls: vec![ScriptedToolCall {
                        id: "lookup-1".into(),
                        name: "lookup".into(),
                        arguments: serde_json::json!({"path": SECRET_PATH}),
                    }],
                    finish_reason: Some("tool_calls".into()),
                    input_tokens: 11,
                    output_tokens: 3,
                    ..ScriptedCompletion::default()
                },
                ScriptedCompletion {
                    tool_calls: vec![ScriptedToolCall {
                        id: "mcp-1".into(),
                        name: "fixture_mcp_query".into(),
                        arguments: serde_json::json!({"query": SCIENTIFIC_ROW}),
                    }],
                    finish_reason: Some("tool_calls".into()),
                    input_tokens: 15,
                    output_tokens: 4,
                    ..ScriptedCompletion::default()
                },
                ScriptedCompletion {
                    content: format!("final response without {SECRET}"),
                    finish_reason: Some("stop".into()),
                    input_tokens: 18,
                    output_tokens: 6,
                    ..ScriptedCompletion::default()
                },
            ],
        )
    }

    fn registry(mcp_result: &str) -> (Registry, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let lookup_runs = Arc::new(AtomicUsize::new(0));
        let mcp_runs = Arc::new(AtomicUsize::new(0));
        let mut tools = Registry::builtins().filtered(&[]);
        tools.add(Box::new(NamedTool {
            name: "lookup",
            mcp: false,
            runs: lookup_runs.clone(),
            result: "lookup-ok".into(),
        }));
        tools.add(Box::new(NamedTool {
            name: "fixture_mcp_query",
            mcp: true,
            runs: mcp_runs.clone(),
            result: mcp_result.into(),
        }));
        (tools, lookup_runs, mcp_runs)
    }

    async fn run_fake_turn(output: &TracingOutput) {
        let provider = scripted_turn();
        let (tools, _, _) = registry(SCIENTIFIC_ROW);
        let mut ctx = ContextManager::new(100_000);
        agent_loop(
            &mut ctx,
            &provider,
            None,
            &tools,
            Path::new("."),
            output,
            &format!("lookup then MCP. credential={SECRET} path={SECRET_PATH}"),
            8,
            None,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn offline_fake_turn_emits_model_tool_mcp_final_response_tree() {
        let trace = AgentTrace::in_memory();
        let output = TracingOutput::new(trace.clone());
        run_fake_turn(&output).await;
        let doc = trace.memory().unwrap().document();
        let kinds = doc.tree_kinds();
        assert!(
            kinds.contains(&SpanKind::Turn)
                && kinds.contains(&SpanKind::Model)
                && kinds.contains(&SpanKind::Tool)
                && kinds.contains(&SpanKind::Mcp)
                && kinds.contains(&SpanKind::Completion),
            "expected model -> tool -> fake MCP -> final response, got {kinds:?}"
        );
        let mcp = doc
            .spans
            .iter()
            .find(|span| span.kind == SpanKind::Mcp)
            .expect("mcp span");
        let tool = doc
            .spans
            .iter()
            .find(|span| {
                span.kind == SpanKind::Tool
                    && span.attributes.tool_id.as_deref() == Some("fixture_mcp_query")
            })
            .expect("mcp tool span");
        assert_eq!(mcp.parent_span_id.as_deref(), Some(tool.span_id.as_str()));
        let models: Vec<_> = doc
            .spans
            .iter()
            .filter(|span| span.kind == SpanKind::Model)
            .collect();
        assert!(models.len() >= 2, "model generation plus final response");
        assert!(models
            .iter()
            .any(|span| span.attributes.output_tokens == Some(6)));
        assert_eq!(doc.header.format, TRACE_FORMAT);
        assert_eq!(doc.header.format_version, TRACE_FORMAT_VERSION);
        assert!(!doc.header.sensitive_capture);
    }

    #[tokio::test]
    async fn default_spans_omit_sensitive_fixtures() {
        let trace = AgentTrace::in_memory();
        let output = TracingOutput::new(trace.clone());
        run_fake_turn(&output).await;
        let encoded = trace.memory().unwrap().document().encoded();
        for forbidden in [SECRET, SCIENTIFIC_ROW, SECRET_PATH, "cohort.csv"] {
            assert!(
                !encoded.contains(forbidden),
                "default capture leaked {forbidden}: {encoded}"
            );
        }
        assert!(!encoded.to_ascii_lowercase().contains("prompt"));
        assert!(!encoded.contains("arguments"));
    }

    #[tokio::test]
    async fn approval_pause_resume_keeps_parent_child_causality() {
        let trace = AgentTrace::in_memory();
        let paused = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let mut output = TracingOutput::new(trace.clone());
        output.approval = Approval::Ask;
        output.paused = Some(paused.clone());
        output.release = Some(release.clone());
        let provider = ScriptedProvider::new(
            "scripted-approval",
            vec![
                ScriptedCompletion {
                    tool_calls: vec![ScriptedToolCall {
                        id: "lookup-1".into(),
                        name: "lookup".into(),
                        arguments: serde_json::json!({}),
                    }],
                    finish_reason: Some("tool_calls".into()),
                    ..ScriptedCompletion::default()
                },
                ScriptedCompletion {
                    content: "approved".into(),
                    finish_reason: Some("stop".into()),
                    ..ScriptedCompletion::default()
                },
            ],
        );
        let (tools, runs, _) = registry("unused");
        let mut ctx = ContextManager::new(100_000);
        let turn = tokio::spawn({
            let output = output;
            async move {
                agent_loop(
                    &mut ctx,
                    &provider,
                    None,
                    &tools,
                    Path::new("."),
                    &output,
                    "needs approval",
                    8,
                    None,
                )
                .await
            }
        });
        paused.notified().await;
        release.notify_waiters();
        turn.await.unwrap().unwrap();
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        let doc = trace.memory().unwrap().document();
        let tool = doc
            .spans
            .iter()
            .find(|span| span.kind == SpanKind::Tool)
            .expect("tool");
        let approval = doc
            .spans
            .iter()
            .find(|span| span.kind == SpanKind::Approval)
            .expect("approval");
        assert_eq!(
            approval.parent_span_id.as_deref(),
            Some(tool.span_id.as_str())
        );
        assert_eq!(approval.trace_id, tool.trace_id);
        assert_eq!(approval.session_id, tool.session_id);
        assert_eq!(approval.status, SpanStatus::Ok);
        assert_eq!(tool.status, SpanStatus::Ok);
        assert!(
            approval.start_unix_ms >= tool.start_unix_ms
                && approval.end_unix_ms.unwrap_or(0) <= tool.end_unix_ms.unwrap_or(u64::MAX)
        );
    }

    #[test]
    fn delegated_child_work_shares_trace_without_merging_sessions() {
        let parent = AgentTrace::in_memory();
        let turn = parent.start_turn(TurnIdentity {
            session_id: Some("parent-session".into()),
            turn_id: Some("turn-a".into()),
            ..TurnIdentity::default()
        });
        let child = parent.delegate_session("child-session");
        let child_turn = child.start_turn(TurnIdentity {
            session_id: Some("child-session".into()),
            parent: Some(turn.context()),
            ..TurnIdentity::default()
        });
        assert_eq!(child_turn.trace_id(), turn.trace_id());
        assert_eq!(
            child_turn.parent_span_id().as_deref(),
            Some(turn.span_id().as_str())
        );
        assert_ne!(child.context().session_id, parent.context().session_id);
        assert_eq!(child.context().session_id.as_deref(), Some("child-session"));
        child_turn.end(SpanStatus::Ok);
        turn.end(SpanStatus::Ok);
        let sessions: BTreeMap<_, _> = parent
            .memory()
            .unwrap()
            .spans()
            .into_iter()
            .filter_map(|span| Some((span.span_id, span.session_id?)))
            .collect();
        assert!(sessions.values().any(|s| s == "parent-session"));
        assert!(sessions.values().any(|s| s == "child-session"));
    }

    #[test]
    fn retry_and_cancel_are_distinct_terminals() {
        let trace = AgentTrace::in_memory();
        let turn = trace.start_turn(TurnIdentity::default());
        let model = turn.child(SpanKind::Model, "agent.model");
        let retry = model.child(SpanKind::Retry, "agent.model.retry");
        retry.set_retry_count(1);
        retry.set_error_class(ErrorClass::Provider);
        retry.end(SpanStatus::Error);
        model.set_retry_count(1);
        model.set_error_class(ErrorClass::RetryExhausted);
        model.end(SpanStatus::Error);
        turn.end(SpanStatus::Error);

        let cancel_trace = AgentTrace::in_memory();
        let cancel_turn = cancel_trace.start_turn(TurnIdentity::default());
        let cancel_model = cancel_turn.child(SpanKind::Model, "agent.model");
        let cancel_retry = cancel_model.child(SpanKind::Retry, "agent.model.retry");
        cancel_retry.set_error_class(ErrorClass::Cancelled);
        cancel_retry.end(SpanStatus::Cancelled);
        cancel_model.set_error_class(ErrorClass::Cancelled);
        cancel_model.end(SpanStatus::Cancelled);
        cancel_turn.end(SpanStatus::Cancelled);

        let retry_status = trace
            .memory()
            .unwrap()
            .spans()
            .into_iter()
            .find(|span| span.kind == SpanKind::Retry)
            .unwrap()
            .status;
        let cancel_status = cancel_trace
            .memory()
            .unwrap()
            .spans()
            .into_iter()
            .find(|span| span.kind == SpanKind::Retry)
            .unwrap()
            .status;
        assert_eq!(retry_status, SpanStatus::Error);
        assert_eq!(cancel_status, SpanStatus::Cancelled);
        assert_ne!(retry_status, cancel_status);
        assert_ne!(SpanStatus::Cancelled, SpanStatus::Error);
    }

    #[test]
    fn uncertain_external_outcome_is_not_success() {
        let trace = AgentTrace::in_memory();
        let turn = trace.start_turn(TurnIdentity::default());
        let mcp = turn.child(SpanKind::Mcp, "agent.mcp");
        drop(mcp);
        turn.end(SpanStatus::Cancelled);
        let mcp = trace
            .memory()
            .unwrap()
            .spans()
            .into_iter()
            .find(|span| span.kind == SpanKind::Mcp)
            .unwrap();
        assert_eq!(mcp.status, SpanStatus::Uncertain);
        assert_ne!(mcp.status, SpanStatus::Ok);
    }

    #[test]
    fn desktop_cli_eval_share_the_observability_contract() {
        let root = std::env::temp_dir().join(format!(
            "wisp-obs-host-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).unwrap();
        for host in [
            ObservabilityHost::Cli,
            ObservabilityHost::Desktop,
            ObservabilityHost::Eval,
        ] {
            let trace = host_agent_observability(
                HostObservabilityConfig::for_host(host, &root).with_session(host.as_str()),
            );
            assert_eq!(trace.host(), host);
            let turn = trace.start_turn(TurnIdentity {
                session_id: Some(host.as_str().into()),
                ..TurnIdentity::default()
            });
            turn.end(SpanStatus::Ok);
            let header = trace.memory().unwrap().header().unwrap();
            assert_eq!(header.format, TRACE_FORMAT);
            assert_eq!(header.format_version, TRACE_FORMAT_VERSION);
            assert_eq!(header.contract, OBSERVABILITY_CONTRACT_ID);
            assert!(!header.sensitive_capture);
            assert_eq!(header.retention.max_age_days, TRACE_RETENTION_DAYS);
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn local_exporters_do_not_require_a_collector_or_network() {
        let root = std::env::temp_dir().join(format!(
            "wisp-obs-export-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let otel = root.join("otel.json");
        let mut config = HostObservabilityConfig::for_host(ObservabilityHost::Test, &root);
        config.otel_json_path = Some(otel.clone());
        let trace = host_agent_observability(config);
        let turn = trace.start_turn(TurnIdentity::default());
        turn.end(SpanStatus::Ok);
        let jsonl = std::fs::read_dir(root.join(".wisp").join(TRACES_DIR))
            .unwrap()
            .find_map(|entry| {
                let path = entry.ok()?.path();
                (path.extension()? == "jsonl").then_some(path)
            })
            .expect("local jsonl");
        let body = std::fs::read_to_string(jsonl).unwrap();
        assert!(body.contains(TRACE_FORMAT));
        assert!(body.contains("\"format_version\":1") || body.contains("\"format_version\": 1"));
        assert!(body.contains("max_age_days"));
        let otel_body = std::fs::read_to_string(&otel).unwrap();
        assert!(otel_body.contains("resourceSpans"));
        assert!(otel_body.contains(OBSERVABILITY_CONTRACT_ID));
        assert!(!otel_body.contains("http://"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn trace_format_version_and_retention_are_documented() {
        let docs = include_str!("../../../docs/agent-observability.md");
        assert!(docs.contains(TRACE_FORMAT));
        assert!(docs.contains("format_version"));
        assert!(docs.contains(&TRACE_FORMAT_VERSION.to_string()));
        assert!(docs.contains(&TRACE_RETENTION_DAYS.to_string()));
        assert!(docs.contains(&SENSITIVE_TRACE_RETENTION_DAYS.to_string()));
        assert!(docs.contains(SENSITIVE_ENV));
        assert!(docs.contains(OTEL_JSON_ENV));
        assert!(docs.contains("do not upload") || docs.contains("Do not upload"));
    }

    #[test]
    fn classify_retry_vs_cancel() {
        let (status, class) = classify_loop_error("stopped by user");
        assert_eq!(status, SpanStatus::Cancelled);
        assert_eq!(class, ErrorClass::Cancelled);
        let (status, class) = classify_loop_error("LLM stream retry exhausted");
        assert_eq!(status, SpanStatus::Error);
        assert_eq!(class, ErrorClass::RetryExhausted);
        assert_ne!(status, SpanStatus::Cancelled);
    }

    #[test]
    fn slo_snapshot_uses_operational_counts() {
        let trace = AgentTrace::in_memory();
        let turn = trace.start_turn(TurnIdentity::default());
        let model = turn.child(SpanKind::Model, "agent.model");
        model.end(SpanStatus::Ok);
        let approval = turn.child(SpanKind::Approval, "agent.approval");
        approval.end(SpanStatus::Ok);
        turn.end(SpanStatus::Ok);
        let slo = slo_from_document(&trace.memory().unwrap().document());
        assert_eq!(slo.completion_rate, 1.0);
        assert!(slo.blocked_rate > 0.0);
        assert_eq!(slo.error_rate, 0.0);
    }

    #[test]
    fn rotating_trace_ids_emit_a_header_per_trace() {
        let trace = AgentTrace::in_memory();
        let first = trace.start_turn(TurnIdentity::default());
        let first_id = first.trace_id();
        first.end(SpanStatus::Ok);
        let second = trace.start_turn(TurnIdentity::default());
        let second_id = second.trace_id();
        second.end(SpanStatus::Ok);
        assert_ne!(first_id, second_id);
        assert_eq!(
            trace.memory().unwrap().document().header.trace_id,
            second_id
        );
    }

    #[test]
    fn retry_exhausted_is_not_overwritten_by_a_generic_provider_class() {
        let trace = AgentTrace::in_memory();
        let turn = trace.start_turn(TurnIdentity::default());
        let model = turn.child(SpanKind::Model, "agent.model");
        model.set_error_class(ErrorClass::RetryExhausted);
        model.set_error_class(ErrorClass::Provider);
        model.end(SpanStatus::Error);
        turn.end(SpanStatus::Error);
        let recorded = trace
            .memory()
            .unwrap()
            .spans()
            .into_iter()
            .find(|span| span.kind == SpanKind::Model)
            .unwrap();
        assert_eq!(
            recorded.attributes.error_class,
            Some(ErrorClass::RetryExhausted)
        );
    }
}

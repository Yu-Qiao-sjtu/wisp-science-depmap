# Agent observability

Wisp records a local execution trace for each Agent request so operators can
follow one turn across model generations, tool calls, MCP requests, approval
waits, delegated children, Workflow/Run transitions, retries, compaction, and
completion.

This is an operational trace. It does **not** replace the user-facing
trajectory or the scientific provenance ledger, and it does **not** capture
chain-of-thought. Traces are written on disk by default and **do not upload**.

## Format and version

Every header and span record carries:

| Field | Value |
|---|---|
| `format` | `wisp.agent-trace.v1` |
| `format_version` | `1` |
| `contract` | `agent.observability.v1` |
| `record_type` | `trace` (header) or `span` |

The durable types live in `wisp-core` (`AgentTrace`, `Span`, `TraceHeader`).
CLI, desktop, eval, and delegated children call `host_agent_observability`
instead of forking exporters.

Stable identifiers:

- `trace_id` — one Agent request. Survives approval pause/resume and is shared
  with delegated child work.
- `run_id` — persisted Run or Workflow id when the host has one.
- `turn_id` — host-visible turn.
- `span_id` / `parent_span_id` — parent-child causality. Child sessions keep
  the parent `trace_id` and a distinct `session_id` so unrelated conversations
  are never merged.

Span kinds: `turn`, `model`, `tool`, `mcp`, `approval`, `delegation`,
`workflow`, `run`, `retry`, `compaction`, `completion`.

Terminal statuses: `ok`, `error`, `cancelled`, `blocked`, `uncertain`.

Retry and cancellation are distinct. A retry hop is a `retry` span that ends
`error` (attempt failed / exhausted) or `ok` (backoff finished). A user or
host stop ends `cancelled`. An MCP or tool call whose external outcome is
unknown ends `uncertain` and is never reported as `ok`.

## What is recorded

Default attributes are operational only:

component, tool/capability id, provider/model id, contract/version fingerprint,
status, latency, retry count, token/byte counts, cache outcome, and error class.

**Default spans do not contain** prompts, tool arguments, scientific rows,
credentials, filesystem paths, or model output.

Sensitive-content capture is an explicit local opt-in:
`WISP_TRACE_SENSITIVE=1`. The header then sets `sensitive_capture: true` and
uses the stricter retention window below. There is still no upload.

## Retention

| Capture | Directory | Age | Size |
|---|---|---|---|
| Default (operational) | `.wisp/traces/` | 7 days | 100 MiB |
| Sensitive opt-in | `.wisp/traces-sensitive/` | 1 day | 32 MiB |

Files are JSONL (one header, then one span per line). A best-effort prune runs
after each write: drop records older than `max_age_days`, then drop oldest
files until the directory is under `max_bytes`.

## Local exporters

- **JSONL file exporter** — always on when the host has a project root.
- **In-memory exporter** — tests and SLO aggregation. No collector.
- **Optional OpenTelemetry-shaped JSON** — set `WISP_TRACE_OTEL_JSON` to a
  local file path. Wisp writes OTLP-like JSON to that path and never opens a
  network connection. Do not upload.

## SLO signals

A closed `TraceDocument` yields:

completion / blocked / error rate, time to first visible progress, tool-schema
failure rate, MCP reconnect/stale-contract rate, approval wait, context growth,
and repeated-call rate.

## Tests

`cargo test -p wisp-core observability` exercises an offline fake turn
(model → tool → fake MCP → final response), approval pause/resume, delegated
child causality, redaction of sensitive fixtures, retry vs cancel terminals,
and local exporters. Those tests do not start a collector or use the network.

Durable continuation of a paused scientific plan is a separate contract:
[agent bridge checkpoints](agent-bridge-checkpoints.md). Trace causality is
preserved across reconnect and Run monitoring; checkpoint state is not inferred
from the transcript.

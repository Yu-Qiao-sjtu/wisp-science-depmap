# Tool execution cache and backpressure

`wisp-tools` coordinates every registered tool call by `(connector, tool)`.
Calls have a bounded concurrency limit and queue even when caching is disabled.
Queue overflow and cancellation return structured errors with stable codes:
`tool_queue_overflow`, `tool_execution_cancelled`, and
`tool_wait_cancelled`.

Caching is fail-closed. A tool must be read-only, must not carry an approval,
and must explicitly declare a certain, shareable result. Its fingerprint
includes normalized arguments, Agent identity, authorization and policy scope,
connector identity, a one-way credential/account revision, the discovered input
schema, the complete remote tool snapshot (including its output schema and
metadata), capability/schema versions, and release/index digests. The connector
revision covers both credentials and non-secret HTTP transport configuration,
including URL and proxy. Changing any credential, transport, or contract
prevents replay. Connector-backed caching is disabled when the host cannot
supply both a non-secret authorization revision and complete remote contract.
Stdio connectors currently bypass caching because their child processes inherit
ambient environment variables whose credentials cannot be fully represented in
that revision. OAuth connectors also bypass caching while token refresh can
rotate credentials after tool registration; static-header HTTP connectors
remain eligible.

MCP servers opt in through `_meta.wisp.cache`:

```json
{
  "enabled": true,
  "durable": true,
  "capabilityVersion": "depmap-query-v2",
  "schemaVersion": "2",
  "releaseDigest": "sha256:...",
  "indexDigest": "sha256:...",
  "ttlSeconds": 300,
  "maxResultBytes": 262144,
  "sharedAuthorization": true,
  "certainOutcome": true,
  "safeStructuredEvidence": true,
  "artifactFree": true,
  "maxConcurrency": 4,
  "maxQueue": 32
}
```

The version/digest, TTL, result-size, authorization, outcome, safe-evidence,
and artifact-free fields are required for eligibility; durability and queue
limits are optional.
`sharedAuthorization` means the
bounded result may be reused only within the host-provided authorization scope;
it never permits sharing across scopes. `safeStructuredEvidence` asserts that
the model-visible JSON contains no credentials, restricted raw matrices, or
hidden server paths. MCP App tools are never cached because replay would skip
their presentation and artifact side effects. `artifactFree` is an up-front
promise that the response cannot materialize HTML or another project artifact;
without it, the call also stays out of single-flight so each caller receives
its own artifact events.

Before returning an MCP cache hit, Wisp refreshes `tools/list` and compares the
live registration, metadata, and schema with the registered snapshot. A change
fails closed and marks the hit stale, so an old release/index result is not
served for the remainder of its TTL.

Memory entries and project entries under `.wisp/tool-cache/v1/` are bounded.
Only hashed filenames and a tool-provided structured JSON projection are
persisted. Every cache directory component is opened through a retained
capability directory handle, and file I/O uses relative no-follow opens; unsafe durable paths fall
back to memory-only caching and are never pruned. When a cancelled MCP wait
leaves provider work running, its connector/tool concurrency lease remains held
until that request actually completes. A cancelled single-flight leader releases
its conversation immediately while the independently owned provider request
continues for still-interested waiters; if cancellation happens while queued,
an active waiter takes over the flight. Cache-hit contract validation follows
the same rule: its caller returns promptly, but provider capacity remains held
until the live validation request completes. Operational spans record `hit`, `miss`, `stale`, `bypass`,
`coalesced`, and `evicted`, plus queue state; raw arguments and evidence are not
added to telemetry.

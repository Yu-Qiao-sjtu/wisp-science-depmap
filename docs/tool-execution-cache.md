# Tool execution cache and backpressure

`wisp-tools` coordinates every registered tool call by `(connector, tool)`.
Calls have a bounded concurrency limit and queue even when caching is disabled.
Queue overflow and cancellation return structured errors with stable codes:
`tool_queue_overflow`, `tool_execution_cancelled`, and
`tool_wait_cancelled`.

Caching is fail-closed. A tool must be read-only, must not carry an approval,
and must explicitly declare a certain, shareable result. Its fingerprint
includes normalized arguments, Agent identity, authorization and policy scope,
connector identity, the discovered input schema, capability/schema versions,
and release/index digests. Changing any contract digest prevents replay.

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
  "maxConcurrency": 4,
  "maxQueue": 32
}
```

The version/digest, TTL, result-size, authorization, outcome, and safe-evidence
fields are required for eligibility; durability and queue limits are optional.
`sharedAuthorization` means the
bounded result may be reused only within the host-provided authorization scope;
it never permits sharing across scopes. `safeStructuredEvidence` asserts that
the model-visible JSON contains no credentials, restricted raw matrices, or
hidden server paths. MCP App tools are never cached because replay would skip
their presentation and artifact side effects.

Memory entries and project entries under `.wisp/tool-cache/v1/` are bounded.
Only hashed filenames and a tool-provided structured JSON projection are
persisted. Operational spans record `hit`, `miss`, `stale`, `bypass`,
`coalesced`, and `evicted`, plus queue state; raw arguments and evidence are not
added to telemetry.

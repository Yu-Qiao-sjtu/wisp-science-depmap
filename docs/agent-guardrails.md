# Agent guardrail lifecycle

Host code validates Agent input, model-produced actions, tool input, tool
output, handoff/delegation, and optional typed final output through one
ordered contract: `wisp.agent-guardrail.v1`.

Each guardrail returns `allow`, `transform` (with an auditable reason),
`request_approval`, `recoverable_reject`, or `terminal_reject`. Later
middleware cannot turn a denial into an allow, reuse a stale approval, or
execute before required approval. Recoverable schema failures return control
to the Agent; policy and integrity failures stop the batch.

The same `evaluate_tool_input` path covers direct tools, deferred MCP
(`use_mcp_tool`), delegated children, and resumed turns. Tool-input schema
validation runs before dispatch; server-side validation remains defense in
depth. Ordinary chat replies are not required to be JSON; a typed final-output
contract, when present, can reject an unsupported structured claim before it
reaches the user.

Guardrail identity, version, stage, and outcome are recorded on the
operational trace from [agent observability](agent-observability.md). Default
spans do not include prompts, tool arguments, or scientific rows.

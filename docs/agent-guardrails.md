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
(`use_mcp_tool`), delegated children, and resumed turns. For `use_mcp_tool`,
the chain looks up the nested `tool_name` and validates `tool_input` against
that connector's discovered schema — not the gateway wrapper. A missing
target schema is a recoverable reject. Tool-input schema validation runs
before dispatch; server-side validation remains defense in depth.

Ordinary chat replies are not required to be JSON. When a typed final-output
contract is installed on the session, `evaluate_final_output` runs on the
empty-tool-call completion path and can reject an unsupported structured
claim before it reaches the user. The JSON Schema subset used for both tool
input and final output includes `required`, `additionalProperties`, `enum`,
`const`, string `minLength`/`maxLength`/`pattern`, array `minItems`/`maxItems`,
object `minProperties`/`maxProperties`, and numeric inclusive/exclusive
bounds.

Guardrail identity, version, stage, and the merged chain outcome are recorded
on the operational trace from [agent observability](agent-observability.md).
A later allow cannot overwrite a denial on those span keys. Default spans do
not include prompts, tool arguments, or scientific rows.

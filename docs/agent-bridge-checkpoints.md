# Agent bridge checkpoints

Nonterminal BridgePlanner decisions persist as `wisp.bridge-checkpoint.v1`.
Resume uses that durable record: intent id/version, capability id/version,
normalized arguments, release, and contract digests. Desktop, CLI, eval, and
the `plan_scientific_intent` tool share this path. A later “continue” does
not ask the model to recreate the scientific request from conversation text.

The checkpoint stores only stable references and a bounded JSON projection. It
does not store credentials, chain-of-thought, raw matrices, or unrestricted
tool output.

A changed manifest, intent catalog, MCP schema, provider contract, coverage
digest, project, or authorization scope yields `resume_contract_changed` before
tool dispatch. Clarification may fill unresolved fields only. Approval is bound
to the exact proposal, capability, arguments, project, and session.

Persistence writes, proposal creation, approval, and Run submission are keyed
by stable operation ids. Replaying the same operation is idempotent. An
uncertain external outcome stays typed and requires reconciliation; it is not
retried as if the effect never ran. Schema-version mismatches are the same
contract-change terminal, including after a process restart.

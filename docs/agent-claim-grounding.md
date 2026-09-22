# Scientific claim grounding

Final answers may include versioned `ClaimRecord`s (`wisp.claim-record.v1`).
Before presentation, measured and derived claims are checked against local
Evidence Ledger rows, validated Runs, Artifact versions, and Paper records.
The check does not recompute analyses, scan files, open SSH, or treat skill
text as evidence.

## What is checked

Each claim carries a kind, optional subject/predicate/scope, numeric value,
unit, direction, p-value, release, sample count, coverage status, and typed
source refs with a `source_version` digest.

| Kind | Hard check |
|---|---|
| `measured_fact` / `derived_result` | Must cite Evidence, Run, or Artifact; numerics, identity, metric, release, sample count, and coverage must match |
| `literature_statement` | Must cite a Paper record |
| `interpretation` / `hypothesis` | Allowed as unlabeled prose; must not carry a measured value |

`NOT_RETAINED` and other coverage statuses are not biological negatives. A
stale `source_version` after compaction or resume fails closed.

Ordinary chat without a `claims` array is not forced into this schema.

## Host wiring

Install a `ClaimGroundingCatalog` on the session `ContextManager`. The
production guardrail chain runs `claim_grounding` on the empty-tool-call
completion path. Durable copies live in SQLite `claim_records` so a claim
cannot silently detach from its source digest.

Reviewers can inspect claim-to-record links; the compact user answer stays
the `answer` field of the same JSON payload.

# DepMap capability release gate

DepMap evaluation suites run a deterministic release gate before any model
scenario is dispatched. The gate proves that each enabled scientific intent is
closed across the production specialist manifest, registered tool and schema,
Reader mode, release coverage, provider contract, evidence envelope, claim
validator, and at least one Agent Capability/Use-case unit (ACU).

## Versioned ACU corpus

The checked-in corpus lives at
`crates/wisp-core/fixtures/depmap-acu-v1.json`. Each de-identified question
family records its canonical intent and permitted ambiguity, expected
capability and query shape, allowed and forbidden tools, provider and coverage
fixture, terminal decisions, tool/context budgets, and typed evidence
invariants. The examples are regression fixtures; they do not create
gene-specific or lineage-specific planner branches.

Every enabled core capability and every terminal planner decision must be
represented. A real-world failure should be minimized into a new ACU before
the fix is considered complete.

The offline production-contract snapshot is
`crates/wisp-core/fixtures/depmap-mcp-contract-v1.json`. Its schemas are copied
from FastMCP discovery for the same release rather than inferred from an ACU.
When the provider registration, schema, Reader, or coverage contract changes,
update that snapshot and intentionally refresh
`depmap-release-lock-v1.json` in the same reviewed change.

## Coverage and terminal states

The gate keeps these states distinct:

- `computed`
- `not_retained`
- `not_tested`
- `not_computed`
- `annotation_unavailable`
- `bridge_unavailable`
- `provider_unavailable`
- `policy_blocked`

In particular, a missing bridge or provider must not be reported as a
scientific `not_computed` result. ACU grading compares typed decisions and
query objects; prose containing words such as “does not establish” is not
treated as a failure by substring matching.

## Release artifact and blockers

The eval JSON report includes `release_gate` for suites tagged `depmap`. It
contains the closure rows, deterministic replay results, current capability,
coverage, server-contract and ACU-suite digests, the optional canary policy,
and typed blockers. A release is blocked before model dispatch by invalid ACUs,
missing registrations or schemas, assembly drift, stale expected digests, a
forbidden fallback, or an unsupported terminal decision.

The checked-in release lock pins capability, coverage, discovered server
contract, and ACU-suite digests. This makes an unreviewed release, coverage, or
schema change fail before scientific dispatch instead of merely appearing as
different metadata in a completed report.

Model-quality failures, provider-canary failures, deterministic contract
failures, and release-assembly failures use separate blocker kinds so a flaky
live model result cannot be mistaken for contract drift. Existing eval options
support repeated live runs across multiple model profiles and retain latency,
token, tool-call, scenario, and repetition data in the same report.

## Optional provider canary

The optional pre-release provider canary is deliberately smaller than the ACU
suite. Its default allowlist contains only `depmap_status` and
`depmap_analysis_catalog`, with at most two calls and 256 KiB per result. The
shared validator rejects mutation, SSH, raw-matrix access, downloads through an
unlisted tool, and calls or results outside those budgets. A host that executes
the canary must validate every observation against this policy and record a
`provider_canary` blocker on failure.

The deterministic gate and fake-provider replay never require a network,
credential, SSH host, GPU, or real DepMap provider.

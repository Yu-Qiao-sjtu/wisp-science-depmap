# Scientific query control plane

DepMap issue examples (a gene, a cancer, a captured trajectory) are
**regression fixtures**. The implementation obligation lives in
`services/depmap_api/scientific_query.py`:

1. Coverage first: `MODULE_UNAVAILABLE` → `NOT_COMPUTED` → `COVERAGE_GAP`.
2. Exact keys are matched on the full table **before** any `limit`.
3. Ranked pages report `matched_row_count` separately from the bounded page.
4. `INELIGIBLE` requires recorded counts versus declared thresholds.
   Absence from a retained shortlist is `NOT_RETAINED` or `NOT_OBSERVED`, never
   a fabricated Mut/WT bound.

`lineage_dependency` / `pan_cancer_dependency` add exact-gene statuses
(`FOUND` / `NOT_RETAINED` / `NOT_TESTED`) and a one-pass common-essential
sidecar join (`exclude_common_essential`). `tf_dependency` reads the installed
TF-activity module in-process: keys in the frozen `tf_order` universe return
`FOUND` / `NOT_RETAINED` / `NOT_TESTED` / `NOT_OBSERVED`, never HTTP 500.
TF-activity queries also page the frozen `tf_order` universe (`view=universe`)
and bulk `top_hits` rankings, reporting `matched_row_count` separately from the
bounded page. DoRothEA is not reconstructed from the browser.

Provider schema (`services/depmap_api/provider_schema.py`): MCP advertised
arguments match runtime validators. `coverage` is catalog-conditional. Each
mode/tool has its own `limit` maximum. Invalid combinations return an
`INELIGIBLE` envelope with `schema_error: true` (HTTP 422), not a traceback.
True Love queries are scope-typed (`scope=lineage|pancancer`). A lineage
request does not filter the pan-cancer catalog; a missing lineage table is
`NOT_COMPUTED` or `COVERAGE_GAP`. Pair definitions stay labeled and are not
merged with direction-discovery or effect-correlation lists.

Query-only turns default to `artifact_requested=false`. `write`/`edit` must not
create `results/reports/**` or unsolicited CSV; the answer stays a bounded
envelope in chat.

New computation is a gated Run behind a non-exfiltrating remote-compute
gateway. Tests use fakes; missing knowledge context or MCP dropout is
`MODULE_UNAVAILABLE` / `configuration_blocked`, not folder guessing or live SSH.
Default replies are cohort aggregates. Restricted inspection is purpose-gated,
capped (≤20 rows), query-scoped and pseudonymous, and rejects pagination,
export, and overlapping reconstruction. ModelIDs never leave the compute side.

Scientific envelopes use progressive disclosure: default layer is status,
bounded top rows, and filter/truncation flags; manifests and evidence IDs are
expanded-only. Literature remains a separate evidence class from DepMap.
Every evidence row may carry shared `qc_annotations` (`small_n`,
`sparse_pair`, `near_perfect_correlation`, `prism_noise`).

Case-folder remapping (user ask → failure mode → issue rewrite) lives in
[depmap-case-to-control-plane.md](depmap-case-to-control-plane.md).

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
sidecar join (`exclude_common_essential`). A named `common_essential_source`
means the label file loaded and each row has a boolean; a missing file omits
the source. A missing confounder sidecar is `ANNOTATION_UNAVAILABLE`; a gene
absent from a loaded sidecar is `NOT_OBSERVED`. The predicate runs on the complete
retained ranking before `cursor`/`limit`; pages expose complete pre/post-filter
counts, `matched_row_count`, and `next_cursor`. A requested exclusion fails
closed as `NOT_COMPUTED` when the versioned sidecar cannot be read, and the
envelope records sidecar source/version/provenance. `tf_dependency` reads the installed
TF-activity module in-process: keys in the frozen `tf_order` universe return
`FOUND` / `NOT_RETAINED` / `NOT_TESTED` / `NOT_OBSERVED`, never HTTP 500.
TF-activity queries also page the frozen `tf_order` universe (`view=universe`)
and bulk `top_hits` rankings, reporting `matched_row_count` separately from the
bounded page. A universe-wide intent with an advertised `bulk_ranking` capability
plans that one page. `NOT_RETAINED` stays absence from the retained set. Without
a bulk capability the planner returns a typed fallback instead of an exact lookup
per entity. DoRothEA is not reconstructed from the browser.

Provider schema (`services/depmap_api/provider_schema.py`): MCP advertised
arguments match runtime validators. `coverage` is catalog-conditional. Each
mode/tool has its own `limit` maximum. Invalid combinations return an
`INELIGIBLE` envelope with `schema_error: true` (HTTP 422), not a traceback.
True Love queries are scope-typed (`scope=lineage|pancancer`). A lineage
request does not filter the pan-cancer catalog; a missing lineage table is
`NOT_COMPUTED` or `COVERAGE_GAP`. Pair definitions stay labeled and are not
merged with direction-discovery or effect-correlation lists.

Completed evidence is folded into a checkpoint that keeps the evidence id, release, and status. Superseded tool payloads and repeated read/grep/edit copies leave later prompts. Spilled `.wisp/tool-output` files and the app database under `.wisp` cannot be read back to reconstruct those rows; the next step is a narrower query.

Query-only turns default to chat-only presentation. `write`/`edit`, shell,
and Python/R must not create `results/reports/**` or unsolicited CSV, and
plotting skills stay unavailable. After the chat answer, the agent asks with
`ask_user` purpose `artifact_presentation` (chat, table, figure, or report).
An explicit artifact request in the original message skips that card and
authorizes only the named kind.

New computation is a gated Run behind a non-exfiltrating remote-compute
gateway. Tests use fakes; missing knowledge context or MCP dropout is
`MODULE_UNAVAILABLE` / `configuration_blocked`, not folder guessing or live SSH.

Scientific envelopes use progressive disclosure: default layer is status,
bounded top rows, and filter/truncation flags; manifests and evidence IDs are
expanded-only. Literature remains a separate evidence class from DepMap.
Every evidence row may carry shared `qc_annotations` (`small_n`,
`sparse_pair`, `near_perfect_correlation`, `prism_noise`).

Case-folder remapping (user ask → failure mode → issue rewrite) lives in
[depmap-case-to-control-plane.md](depmap-case-to-control-plane.md).

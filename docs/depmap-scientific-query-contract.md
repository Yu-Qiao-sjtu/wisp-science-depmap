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

Case-folder remapping (user ask → failure mode → issue rewrite) lives in
[depmap-case-to-control-plane.md](depmap-case-to-control-plane.md).

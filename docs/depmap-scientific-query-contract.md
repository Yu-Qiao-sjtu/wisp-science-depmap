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

Future issues (#52 selectivity, #28 common-essential, later TF/TLG) add
predicates to this plane. They do not add gene- or lineage-specific tools.

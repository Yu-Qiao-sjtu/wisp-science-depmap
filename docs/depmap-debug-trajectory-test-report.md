# DepMap debug trajectory offline regression report

Date: 2026-08-30

Scope: `D:\wisp_agent\wisp_debug-2`, `wisp_debug-3`, and `wisp_debug-4`.
This report validates exported failure signatures and the uncompiled source
contracts intended to prevent them. It does not claim that a newly built EXE or
an LLM replay has passed.

## Results

| Layer | Result | Evidence |
|---|---:|---|
| Exported trajectory parsing | PASS | Debug 2: 6/6 signatures; Debug 3: 7/7; Debug 4: 14/14 |
| Source guard coverage | PASS | 14/14 required contract markers present |
| Regression-suite structure | PASS | YAML parses; 10 unique cases |
| DepMap Skill validation | PASS | `quick_validate.py` |
| Remote API unit tests | PASS | 11/11 Python tests |
| Local R alias/catalog smoke | PASS | Breast Cancer→Breast 8/8; Colorectal→Bowel 8/8; liver→Liver 8/8 |
| Rust formatting/diff checks | PASS | `cargo fmt --all -- --check`; `git diff --check` |
| Rust compilation/unit tests | NOT RUN | Explicitly deferred by user |
| New EXE model replay | NOT RUN | Requires later authorized NSIS-only build |

## Per-trajectory gates

### Debug 2 — ESR1 × breast cancer

Detected in the old export:

- unnormalized `Breast Cancer` storage lookup;
- false lineage coverage gaps;
- unsupported bimodality and ER-subtype inference;
- broad spill-directory grep;
- unrelated TP53 contamination.

Current source guards canonicalize `Breast`, prohibit distribution/subtype
inference from aggregate summaries, and restrict spill reads to the exact file.

### Debug 3 — colorectal cancer without a gene

Detected in the old export:

- historical Run preload;
- five empty `depmap_query` calls (ten appearances in the HTML input/output
  rendering);
- invented APC anchor and unnormalized `Colorectal` lineage;
- mutation mean difference reported as rho;
- proposed Wilcoxon work represented as query-only;
- unsupported venetoclax claim.

Current source guards route disease-only requests to
`lineage_catalog(Bowel)`, use a flat tool schema with one retry at most, attach
metric semantics, and forbid unsupported computation/drug claims.

### Debug 4 — ATF5 × liver-cancer topic design

Detected in the old export:

- historical Run preload before Workflow routing;
- unavailable `depmap_read`, followed by manual Workflow reconstruction;
- five empty `depmap_query` calls;
- stale `17/33`, Wilcoxon, and FDR values;
- file presence promoted to universal recomputation feasibility;
- damaging events called pathogenic;
- non-significant mutation rows used to seed mechanisms and drugs;
- `Liver` silently narrowed to HCC, with an unrequested control lineage and
  unsupported named cell lines;
- an unapproved report write.

Current source guards make registered Workflow routing first, preserve the
DepMap specialist's required Skill in Workflow resource discovery, return a
structured no-manual-fallback failure, expose current `focus.core` summaries,
and require corrected statistical, literature, clinical-scope, and approval
boundaries.

## Reproduce

Run the offline trajectory/source-contract check without compiling Rust:

```powershell
python scripts/validate_depmap_debug_trajectories.py `
  --trajectory-dir D:\wisp_agent `
  --repo-root D:\wisp_sci
```

## Remaining release gate

After the user finishes supplying trajectories and explicitly authorizes a
build, build only the NSIS EXE. Replay the original three user prompts in fresh
conversations and require:

1. no empty or duplicate DepMap tool calls;
2. canonical Breast/Bowel/Liver scopes;
3. no historical Run context for query-only or Workflow-launch turns;
4. no unsupported statistics, subtypes, mechanisms, drugs, or cell lines;
5. topic Workflow draft created before evidence work and no report write before
   approval;
6. every number traceable to the current returned evidence or a validated Run.


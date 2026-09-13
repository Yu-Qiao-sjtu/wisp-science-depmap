# DepMap TM00 server extensions

These modules extend the 26Q1 precomputed knowledge root on `guotosky`. They
reuse audited TM00 methods but do not modify the original 170 GiB module
outputs. WGCNA and predictive models are deliberately excluded.

## Server paths

- Knowledge root: `/home/data/gz0548/depmap-26q1`
- Raw 26Q1 inputs: `/home/data/gz0548/depmap-agent/data/source`
- Builder scripts: `/home/data/gz0548/depmap-agent/analysis/depmap-kb`
- Long-run logs: `/home/data/gz0548/depmap-agent/logs`

## Added modules

| Module | Builder | Scientific scope |
|---|---|---|
| `lineage_selective_dependency` | `37_build_lineage_selective_dependency.R` | Limma lineage-versus-rest differential Gene Effect for every eligible lineage |
| `progeny_prism_associations` | `38_build_progeny_prism_associations.R` | PROGENy pathway activity versus PRISM AUC |
| `observational_synthetic_lethal_candidates` | `39_build_observational_synthetic_lethal.R` | Significant mutation/amplification-context vulnerabilities assembled from existing exhaustive blocks |
| `coamplification_dependency` | `40_run_coamplification_dependency.R`, `44_build_coamplification_pair_catalog.R`, `45_build_coamplification_dependency_screen.R` | Complete eligible event catalog, exhaustive high-confidence discovery screen, and on-demand long-tail contrasts |
| `bipolar_dependency_candidates` | `41_build_bipolar_dependency_candidates.R` | Strong negative Gene Effect correlations and reciprocal top-K pairs |
| `lineage_selective_enrichment` | `42_build_lineage_selective_enrichment.R` | Hallmark/Reactome GSEA over lineage-selective dependency ranks |
| `true_love_gene` | `47_build_true_love_genes.R` | Strict mutual rank-1 negative Gene Effect co-dependency pairs from TM00 script 17 |

Every completed module writes a `manifest.json` recording cohort sizes,
method, correction family, direction semantics, inputs, and TM00 references.

The co-amplification module has two deliberately separate coverage layers:

- The event catalog contains all 20,988,320 directional pairs supported by at
  least 5 co-amplified and 5 source-only models (copy-number threshold 2).
- The exhaustive discovery layer tests 15,368 higher-confidence directional
  pairs against all 18,531 CRISPR targets: each comparison has at least 25
  co-amplified, 25 source-only, and 25 partner-only models; both amplification
  prevalences are at most 100 models; pair Jaccard overlap is at most 0.4.
  This is 284,784,408 nominal tests, split into 61 resumable shards. BH FDR is
  controlled separately over the 18,531 targets within each directional pair.
- Pairs outside the discovery layer are retained, not discarded. They remain
  queryable through the on-demand runner, whose manifest records group sizes
  and correction scope.
The server-side validator writes:

- `depmap-26q1-tm00-extension-catalog.json`
- `depmap-26q1-tm00-extension-catalog.csv`

## Interpretation boundaries

- A negative lineage contrast means stronger dependency in that DepMap model
  grouping; it does not establish a patient biomarker.
- Lower PRISM AUC means greater drug sensitivity. Pathway–AUC correlations are
  associations, not treatment effects.
- Mutation-context candidates are observational synthetic-lethal hypotheses,
  not validated synthetic lethality.
- Amplification-context candidates remain conditional associations under the
  recorded copy-number threshold.
- Co-amplification hits can repeat across genes on the same amplicon because
  correlated copy-number events may define identical model groups. They are
  statistical discovery candidates, not independent causal claims.
- Bipolar dependency means inverse Gene Effect correlation only; it is not
  automatically a regulatory interaction.
- A strict True Love Gene pair requires both genes to rank the other first by
  negative Gene Effect correlation. At least 500 shared models are required to
  prevent low-coverage near-perfect correlations from dominating the ranking;
  reciprocal ranking remains hypothesis-generating rather than causal proof.
- Each module retains its own multiple-testing family. Statistics from
  different modules are never combined into a synthetic p value.

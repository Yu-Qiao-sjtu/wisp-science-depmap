# DepMap reference-script capability map

Use this map to retrieve only the R sources relevant to the current question.
The exact dependency graph is compiled in `capability-manifest.json`; this page
explains its scientific intent.
The scripts remain authoritative references for method behavior, but example
genes, old release labels, and machine-specific output paths are parameters to
review rather than defaults to copy.

| Capability ID | Script | Primary capability | Audited direct inputs |
|---|---|---|---|
| `core_ingestion` | `01_read_depmap.r` | Load and align core DepMap matrices; create reusable R objects | Gene Effect, Dependency Probability, expression, Model, damaging mutation matrix |
| `gene_correlation` | `02_gene_gene_correlation.R` | Pairwise expression correlation | `ccle_exprSet.rds` |
| `co_dependency` | `03_co_dependency.R` | Genome-wide co-dependency ranking and matrix output | `depmap_geneEffect.rds` |
| `predictive_biomarkers` | `04_predivtive_biomarkers.R` | Correlation and ML predictive biomarkers | Gene Effect RDS, expression RDS |
| `gene_to_dependency` | `05_from_gene_to_dependency.R` | Gene-centered dependency, pathway, TF, and enrichment analyses; its `pathway_enrichment` operation runs the reviewed on-demand GSEA executable | Gene Effect RDS and expression RDS; the GSEA operation uses the complete expression-dependency block matrix and Hallmark/Reactome or a versioned custom GMT |
| `pathway_ml` | `05.1_ml_pathway.R` | Penalized regression and random-forest pathway models | PROGENy scores RDS, Gene Effect RDS |
| `synthetic_lethal_screen` | `05.2_synthetic_lethal.R` | Observational synthetic-lethal candidate screening | Co-dependency matrix RDS |
| `prism_drug_sensitivity` | `05.3_drug_sensitivity.R` | PRISM compound-response association | PROGENy scores RDS, PRISM AUC, compound metadata |
| `wgcna` | `05.4_wgcna.R` | WGCNA module discovery | Expression RDS, Gene Effect RDS |
| `mutation_anchor_selection` | `06_mut_anchor_gene_selection_26Q1.R` | Select analyzable mutation anchors by lineage and event definition | Somatic mutation table, Damaging and Hotspot matrices, Dependency Probability, Model |
| `damaging_mutation_dependency` | `07_mutant_dependency_26Q1.R` | Mutation-stratified dependency and bidirectional batch screening | Damaging mutation matrix, Dependency Probability, Model |
| `annotated_mutation_dependency` | `08_mutData_updata_26Q1.R` | Audit current mutation definitions and compare custom, Damaging, and Hotspot matrices | Somatic mutations, Damaging and Hotspot matrices, Dependency Probability, Model |
| `mutation_to_target` | `09_batch_from_mut_to_target_26Q1.R` | Fix one mutation event and screen dependency targets | Damaging mutation, Dependency Probability and cell-info RDS files |
| `mutation_to_target_lineage` | `10_batch_from_mut_to_target_26Q1_add_celltype.R` | Mutation-to-target association with lineage-specific score columns | Damaging mutation, Dependency Probability and cell-info RDS files |
| `target_to_mutation_lineage` | `11_batch_from_gene_to_mut_26Q1_add_celltype.R` | Fix one dependency target and screen mutation biomarkers | Damaging mutation, Dependency Probability and cell-info RDS files |
| `ccne1_amplification` | `12_CCNE1_AMP_PKMYT1.R` | Amplification-context dependency and volcano plots | Gene Effect, copy number, ModelCondition |
| `mycn_ddx1_coamplification` | `13_MYCN_DDX1_coamplification_Cancer_discovery.R` | Co-amplification, dependency, and enrichment | Gene Effect, copy number, ModelCondition |
| `disease_context_dependency` | `14_DCAF5_SMARCB1_Nature.R` | Disease-context differential dependency and literature method reproduction | Model metadata, Gene Effect |
| `sanger_cross_library` | `15_Sanger_CRISPR.R` | Model-level versus screen-level and cross-library CRISPR comparison | Gene Effect, Screen Gene Effect, ScreenSequenceMap, cell-info RDS |
| `negative_dependency_expression` | `16_Dependency_nagative_correlation.R` | Negative/bipolar dependency analysis with expression context | Gene Effect, expression, cell-info RDS |
| `bipolar_dependency` | `17_bipolar_dependency_ASB7_as_example.R` | Recursive bipolar dependency searches and paired visualization | Gene Effect |
| `drug_auc_cross_validation` | `18_DrugAUC_and_DepMap_MTAPasExample.R` | CRISPR/RNAi/CNV/expression/drug cross-validation | AMG193 AUC, Gene Effect, `cellinfor.rds`, DEMETER2, CNV, ModelCondition, expression |

## Routing examples

- Gene overview: `01`, `02`, `03`.
- Mutation-anchor discovery by cancer: `01`, then `06`.
- Fixed mutation event to dependency targets: `01`, `08`, then `07`, `09`, or `10`.
- Fixed dependency target to mutation biomarkers: `01`, then `07` or `11`.
- Copy-number amplification dependency: `01`, `12` and/or `13`.
- Drug-response relationship: `01`, `05.3`, and `18` when cross-platform
  evidence is required.
- Cross-library robustness: `01`, `15`, optionally `18` for RNAi.
- Synthetic-lethal hypothesis generation: `01`, `05.2`, relevant mutation/CNV
  script, then `18` when an orthogonal dataset is available.
- “Which dependency genes follow ESR1 expression?”: query the precomputed
  expression-dependency association row. “Which dependency pathways follow
  ESR1 expression?” or an explicit GSEA request: select `gene_to_dependency`
  with `operation=pathway_enrichment` and execute its reviewed entrypoint.

Always inspect the selected source before reproducing it. The map describes
intent, not a guarantee that every script is already parameterized for the
active release.

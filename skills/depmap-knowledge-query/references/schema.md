# Query schema

The query helper accepts `--kb-root`, `--mode`, and mode-specific fields.

- `catalog`: unified QA and module catalog.
- `lineage_catalog --lineage NAME`: cancer-lineage module availability and
  eligibility manifests without selecting a gene.
- `core --gene GENE`: core dependency and lineage summary from Parquet.
- `pair --module MODULE --source GENE --target GENE`: one matrix cell.
- `top --module MODULE --source GENE --limit N`: strongest source-row associations.
- `lineage --event damaging|custom_missense|hotspot --lineage NAME --source GENE --target GENE`.
- `pathway --pathway NAME --target GENE`.
- `drug --omic effect|expression|cnv --drug NAME_OR_DPC_ID --target GENE`.
- `lineage_network --family effect_correlation|expression_correlation|expression_dependency --lineage NAME --source GENE [--target GENE] [--reciprocal true] [--limit N]`.
- `lineage_cnv --lineage NAME --source GENE [--target GENE] [--limit N]`.
- `lineage_drug --omic effect|expression|cnv --lineage NAME [--drug NAME_OR_DPC_ID] [--target GENE] [--limit N]`; at least one of drug or target is required.
- `enrichment --lineage NAME --source GENE [--collection NAME] [--term NAME] [--limit N]`.
- `tcga_expression_survival --gene GENE [--project TCGA-BRCA] [--lineage NAME] [--endpoint OS|DSS|DFI|PFI] [--limit N]`.

The native tool exposes these fields through one flat model-compatible schema;
`mode` is always required and runtime validation enforces the remaining fields.
Do not submit `{}` or repeat an invalid empty call.

Matrix modules supported by `pair/top`:

- `effect_correlation`: source and target are Gene Effect genes; positive is co-dependency, negative is anti-correlation.
- `expression_correlation`: source and target are expression genes.
- `expression_dependency`: source is expression, target is Gene Effect; negative means higher expression associates with stronger dependency.
- `damaging_mutation_dependency`, `custom_missense_mutation_dependency`, `hotspot_mutation_dependency`: negative mean difference means mutant models are more dependent.
- `cnv_amplification_dependency`: negative mean difference means amplified models are more dependent.

Every response includes file provenance. FDR is adjusted within the family stated by that module's manifest, not globally across the entire knowledge base.

`tcga_expression_survival` aligns genes by Ensembl gene ID with an explicit
gene-symbol fallback, uses primary cancer samples only, keeps one sample per
patient, transforms TPM as `log2(TPM+1)`, and reports a univariate Breslow
Cox score test at beta zero. Its signed z statistic is positive when higher
expression is associated with higher event hazard and negative in the opposite
direction. BH FDR is calculated separately within each TCGA project and
survival endpoint. It does not join TCGA patients to DepMap cell lines, and it
does not report a hazard ratio.

## Evidence status

New sparse lineage modes return one explicit status:

- `FOUND`: one or more retained rows match.
- `NOT_RETAINED`: the eligible analysis ran, but the requested association is
  absent from the retained top-K rows. This does not prove a null association.
- `INELIGIBLE`: the lineage or event failed the manifest's sample/event/control
  thresholds.
- `NOT_COMPUTED`: the requested lineage, source universe, block, or indexed
  entity is absent.
- `MODULE_UNAVAILABLE`: the module is not installed in this knowledge release.

`lineage_network` reads the sparse all-gene scans and can require reciprocal
retention. `lineage_cnv` reports amplification-vs-control dependency effects;
negative mean difference means amplified models are more dependent.
`lineage_drug` reports Pearson association with PRISM AUC and preserves the
within-drug FDR. `enrichment` returns Hallmark, Reactome 2023.2, or signed
DoRothEA A-C TF rows stored for the requested source gene and lineage.

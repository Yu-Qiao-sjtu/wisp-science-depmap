# Gene-pair intent routing

Use the user's measurement words to select the module. Gene names alone do not identify the measurement.

| User intent | Required semantic tags | Global query | Lineage query |
|---|---|---|---|
| Expression gene-gene correlation / 共表达 / 表达量相关 | `analysis_label=gene_gene_coexpression`, `data_modality=transcript_expression_log2_tpm_plus_1`, `relation_type=coexpression` | `mode=pair`, `module=expression_correlation` | `mode=lineage_network`, `family=expression_correlation` |
| CRISPR Gene Effect correlation / 共依赖 / 敲除依赖相关 | `analysis_label=gene_gene_codependency`, `data_modality=crispr_gene_effect`, `relation_type=codependency` | `mode=pair`, `module=effect_correlation` | `mode=lineage_network`, `family=effect_correlation` |
| Expression versus CRISPR dependency / 表达是否预测依赖 | `analysis_label=expression_dependency_association`, `data_modality=expression_vs_crispr_gene_effect`, `relation_type=predictive_association` | `mode=pair`, `module=expression_dependency` | `mode=lineage_network`, `family=expression_dependency` |

Apply these routing rules:

1. `表达`, `表达量`, `共表达`, `RNA`, `TPM`, or `expression` selects `expression_correlation` when both entities are genes.
2. `依赖`, `共依赖`, `敲除`, `CRISPR`, `Gene Effect`, `CERES`, or `dependency profile` selects `effect_correlation` when both entities are genes.
3. A request that relates expression of one gene to dependency on another selects `expression_dependency`; source is the expression gene and target is the Gene Effect gene.
4. `某两个基因是否相关` or `A 和 B 有什么关系` is measurement-ambiguous. Use `depmap_pair_evidence`, report expression and Gene Effect results separately, and ask for a measurement only if the next action requires selecting one.
5. A cancer or lineage term changes `scope` to `lineage`; resolve the lineage first. Without a cancer term, use `scope=global`.
6. Global `effect_correlation` uses `cohort_policy=all_available_gene_effect_models`. Use the 1,140-model TM00 intersection only when the user asks to reproduce TM00 or align with expression.
7. Never call `effect_correlation` expression correlation, never call `expression_correlation` dependency, and never infer synthetic lethality from either correlation alone.

Module-owned machine-readable metadata is stored in each analysis module's `module.intent.json`.

## Mutation-to-dependency direction routing

Mutation association is directed: the source is a mutation event and the target
is a CRISPR dependency gene. The two user workflows query opposite axes of the
same event-by-target result matrix.

| User intent | Entity roles | Query |
|---|---|---|
| “A 突变后依赖哪些基因？” / fixed mutation, find targets | `source=A mutation event`; target omitted | `depmap_synthetic_lethal_evidence(source=A, event=...)` |
| “哪些突变使细胞更依赖 B？” / fixed target, find mutation biomarkers | source omitted; `target=B dependency gene` | `depmap_synthetic_lethal_evidence(target=B, event=...)` |
| “A 突变是否影响 B 依赖？” / one directed pair | `source=A mutation event`; `target=B dependency gene` | `depmap_synthetic_lethal_evidence(source=A, target=B, event=...)` |

Apply these rules:

1. `Hotspot` is a mutation-event definition. A user phrase such as “热点基因”
   may instead mean a gene of interest; distinguish this before routing. A fixed
   dependency target does not need to be mutated or labeled Hotspot.
2. Loss-of-function, damaging, truncating, frameshift, splice-loss, or TSG
   language selects `event=damaging_mutation`. Activating, recurrent hotspot,
   GoF, or oncogene language selects `event=hotspot_mutation`.
3. A named protein change such as KRAS G12D requires an allele-specific result.
   Do not substitute the gene-level Hotspot or all-missense group when the
   requested allele was not precomputed.
4. If the user says only “A mutation” without a functional definition, report
   the available event definitions and counts rather than silently choosing one.
5. Preserve the returned dependency metric. Positive mutant-minus-control
   difference means stronger dependency for `CRISPRGeneDependency`; negative
   difference means stronger dependency for `CRISPRGeneEffect`.
6. Forward and reverse FDR answer different questions. Preserve the manifest's
   multiple-testing family for the requested direction.

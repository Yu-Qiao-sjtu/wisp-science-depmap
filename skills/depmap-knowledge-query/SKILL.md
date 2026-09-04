---
name: depmap-knowledge-query
description: Query and interpret a precomputed DepMap 26Q1 knowledge base by gene, cancer lineage, correlation or co-dependency network, mutation/CNV event, enrichment, pathway, PRISM drug, or the installed TCGA expression-survival bridge. Use this before launching new computation; do not use when only raw release data is available.
---

# DepMap knowledge query

Use the precomputed knowledge base as the default execution path. Natural-language requests are translated into bounded queries; full matrices stay on disk and only selected rows enter model context.

## Routing

1. Call native `depmap_agent_route` once for each new request. The machine-readable
   [Agent capability registry](references/agent-capability-registry.json) is the
   authoritative mapping from extracted intent/entities to execution level,
   bounded query, required arguments, metric, and allowed/forbidden claims.
   Execute `recommended_query` exactly when present; do not substitute a nearby
   mode or reconstruct the mapping from this prose or model memory.
2. Resolve model-extracted mentions through `depmap_resolve_entity`, backed by
   the [Scientific Entity Registry](references/scientific-entity-registry.json).
   Cancer, gene, drug, pathway, phenotype, molecular-focus, mechanism, source,
   and output mentions use one result contract. Continue automatically only for
   `RESOLVED`; `NORMALIZED_UNVERIFIED` is query-safe spelling, not proof that an
   entity exists. For `AMBIGUOUS` or `INVALID_CANDIDATES`, obtain user
   confirmation. `NOT_FOUND` is an entity-coverage state, never negative
   biology. The legacy `depmap_resolve_lineage` tool remains compatible, but new
   Agent plans use the generic resolver.
3. Follow the route's `allowed_next_tools` boundary. For a gene-plus-lineage
   request, `depmap_evidence` assembles a bounded dynamic view. For a surgical
   request, `depmap_query` exposes only modes declared in the registry. Every
   successful query returns a `capability_contract`; interpret only its
   `allowed_claims` and honor its `forbidden_claims`. Read
   [references/evidence-contract.md](references/evidence-contract.md) for the
   assembled view and [references/schema.md](references/schema.md) for provider
   field definitions.
   For a multi-dimensional topic-design request, preserve the route's arrays
   for `phenotypes`, `molecular_focus`, `mechanisms`, `evidence_sources`, and
   `requested_outputs`, plus `execution_policy` and `unresolved_concepts`.
   Execute the returned `topic_plan` query exactly. Do not compress those slots
   back into legacy `evidence_focus` or drop a phenotype merely because it is
   not precomputed.
   For a set-valued focus such as transcription factors, call
   `depmap_describe_capabilities` before evidence retrieval. Treat
   `dorothea_tf_abc` as an entity set that can occupy compatible source, target,
   feature, event, or regulator roles across modules. Do not reduce the request
   to the DoRothEA enrichment collection alone. Capability matches are plans,
   not result hits.
4. Classify the request as a precomputed query or a coverage gap. The catalog
   contains aggregate and matrix results, not every possible subgroup,
   covariate-adjusted test, model-level detail, clinical annotation, or
   literature claim.
   Use `knowledge.available_query_families` and `knowledge.coverage_checks`
   from the resolver; do not infer coverage from the size of the directory.
   When the user asks which data support a proposed study or manuscript plan,
   route it as `study_support_mapping` and query the cancer's
   `lineage_catalog`. For every requested claim, report exactly one status:
   `direct_precomputed_evidence`, `new_computation_from_available_inputs`,
   `missing_data_or_coverage`, or `literature_only_or_unverified`. This is a
   query-only inventory: never use shell, `run_in_context`, guessed server
   paths, or a filesystem scan to bypass the provider. A module's presence is
   not proof that a proposed subgroup, contrast, mechanism, or drug combination
   has already been analyzed.
5. Runtime validation enforces the registry's required fields plus bounded
   provider vocabularies. Send one complete call; do not probe the contract by
   intentionally sending incomplete calls. Use `scripts/query_depmap_kb.R`
   only as a CLI fallback when the native tool is unavailable.
   If a tool receives empty arguments or returns `invalid_query`, do not repeat
   the identical call. Correct it once using the visible flat schema, then
   report a block if the corrected call still cannot be serialized.
6. Interpret returned JSON with the event definition, sample count, effect
   direction, P/FDR family, and source path intact.
7. On a coverage gap, state exactly what is absent. Load
   `depmap-coding-agent` only after the request has explicitly transitioned to
   a new analysis; never silently fall back to raw matrices.
8. Use `start_workflow` only when the user explicitly requests a named Workflow
   or the route returns a durable L4 task. Preserve approval boundaries, and do
   not manually reconstruct a failed approved Workflow.

## Grounded answer contract

- Every numerical claim must be copied from a successful `depmap_evidence` or `depmap_query`
  response in the current turn. Do not answer a numerical question from model
  memory, directory size, a filename, or an earlier conversational summary.
- Separate `facts` (returned rows and manifest fields) from `interpretation`
  (biological meaning) and `hypotheses` (what would require validation).
- Preserve `release`, `manifest`, cohort/sample counts, method, retention rule,
  and `provenance` in the answer or its compact evidence section.
- Treat `FOUND` as a retained precomputed result. Treat `NOT_RETAINED` only as
  absence from a sparse top-K output, never as evidence of no association.
  Treat `INELIGIBLE` as a cohort/sample-size failure, never as a biological
  negative. `NOT_COMPUTED` and `MODULE_UNAVAILABLE` are coverage states.
- If the provider or query is blocked, stop numerical interpretation. Do not
  fill missing statistics with plausible values.
- Treat cancer names as labels that must resolve to the canonical lineage in
  the returned query. Do not report `NOT_COMPUTED` from an unnormalized synonym
  such as `Breast Cancer` when the provider resolves it to `Breast`.
- Chinese cancer names and maintained common synonyms resolve across all 34
  canonical lineage groups. Use the canonical value returned by the provider
  for every subsequent query, and preserve the user's original disease wording
  separately. If a new or ambiguous synonym is not resolved, report the
  ambiguity instead of guessing a lineage.
- In `lineage_dependency`, preserve the requested ranking contract.
  `effect_mean_difference` is lineage mean Gene Effect minus the rest mean and
  must never be renamed `logFC`; `mean_dependency` is descriptive and does not
  imply lineage selectivity.
- A canonical DepMap lineage is a model-grouping proxy, not a clinical
  histology or patient cohort. Do not silently narrow `Liver` to HCC, add a
  neighboring control lineage, or name cell lines unless current model
  metadata or a validated Run supports that scope.
- Descriptive summaries do not establish distribution shape or molecular
  subtype. A mean/median difference is not evidence of bimodality, and a gene's
  known biology does not establish ER, HER2, mutation, or other sample labels
  unless those fields were returned in the current result.
- Continuous expression-to-dependency or enrichment results do not define a
  high/low subgroup unless the returned contract includes that grouping and its
  threshold. Do not say one section is the only FDR-significant signal when a
  different returned section also has an adjusted P value below the threshold.
- A continuous expression-dependency correlation is not a `TF-high` screen,
  high-versus-low contrast, selective dependency, or synthetic lethality.
  Thresholded subgroups, TF-activity groupings, PRISM combinations, mechanism
  reconstructions, and cross-platform convergence remain proposed new
  computations until a validated Run returns them.
- `not_testable` and `INELIGIBLE` describe current cohort/provider eligibility;
  they do not prove that the biological route or a future study is infeasible.
  Likewise, zero models crossing a descriptive dependency cutoff supports only
  that exact observation, not a categorical no-direct-dependency claim.
- Read `semantics.metric` before naming a statistic. Mutation/CNV
  `mean_difference` values are group contrasts, never correlation coefficients.
- A TCGA expression-survival row is a patient-cohort association mapped through
  the shared gene symbol and cancer label. It is not a TCGA-to-DepMap sample
  join. Preserve the requested endpoint, primary-tumour cohort/event counts,
  `log2(TPM+1)` scale, Cox-score method, and within-project/endpoint FDR family.
  Positive score z means higher expression is associated with higher event
  hazard; do not call the score a hazard ratio or imply causality.
- `damaging_mutation_n` is a count under the provider's damaging-event
  definition. Do not rename it pathogenic, disease-causing, or clinically
  actionable without a separate clinical annotation source.
- When no row survives the stated multiple-testing threshold, report that null
  result. Do not use nominal top targets to invent a biological module, drug,
  synthetic-lethal pair, or mechanism.
- Query-only work may select or summarize existing returned rows. Constructing
  a matrix, comparing cohorts, running Wilcoxon/Kruskal-Wallis, recalculating
  FDR, fitting a model, or testing a subgroup is new computation.
- Cancer-level direction discovery must preserve the tool's separate
  family-specific rankings. Its cross-family count is unweighted retrieval
  convergence, not a combined P value or universal biological score. Candidates
  remain hypotheses until identifier/QC, literature, and experimental review.
- For `topic_plan`, propose only the analysis templates explicitly returned in
  `new_computation_from_available_inputs`. Do not add a high/low contrast,
  subtype analysis, network expansion, or another attractive study design that
  the current result did not return.
- Do not attach a drug, clinical actionability claim, or named treatment to a
  gene unless a current drug result or separately cited literature evidence
  supports that relationship.
- A raw file's presence proves asset availability only. It does not prove
  cohort eligibility, identifier overlap, statistical power, or that every
  apparent coverage gap can be recomputed.
- Skill text and model memory are not literature evidence. Mechanism, novelty,
  treatment, and clinical claims require a completed literature-evidence task
  with traceable paper identifiers.
- Search snippets, AI summaries, title matches, and reference-list mentions are
  provisional leads. Track candidate, verified, contradicted, and retracted
  claims; allow only claims checked in the primary paper's abstract or full
  text with a stable PMID, PMCID, DOI, or publisher URL into the final answer.
  Stop adaptively at evidence saturation rather than a fixed tool-call count.
  A negative search supports only "not found within the searched scope", never
  "nobody has done this", "unique gap", or proof that a topic is unpublished.
- Correlation and screen-derived associations are not causation or validated
  synthetic lethality. Negative Gene Effect means stronger dependency; lower
  PRISM AUC means greater sensitivity, so explain correlation direction using
  the queried feature and phenotype.

For a gene-focused cancer answer, prefer one `depmap_evidence` call and rank
only the rows actually returned. It combines bounded core, lineage network,
mutation, CNV, drug, enrichment, and TCGA expression-survival queries at
request time; it is not a
precomputed evidence-card requirement. Use `depmap_query` afterward only when a
specific pair, drug, pathway, or term needs a narrower lookup.

Use the dedicated `depmap_subtype_evidence`,
`depmap_coamplification_evidence`, `depmap_true_love_evidence`,
`depmap_synthetic_lethal_evidence`, and `depmap_3d_evidence` MCP tools for the
completed subtype, double-amplification, reciprocal-pair, observational
synthetic-lethal, and 3D modules. Do not reinterpret an arbitrary free-text subtype
as a frozen contrast, and do not treat a pair outside the constrained screen as
if it had been exhaustively tested.
Treat True Love and synthetic-lethal labels as hypothesis-generating screen
classes, not causal mechanisms. For 3D, preserve the returned cohort/contrast
and distinguish exploratory CNS-only results from lineage-adjusted results.

When a bounded tool result is spilled to a named `.wisp/tool-output` file, read
or grep only that exact path and only the necessary ranges. Never grep the
parent `tool-output` directory: it contains unrelated historical queries that
must not enter the current evidence set.

For inventory, discovery, or topic-ideation requests covered by the knowledge
provider, remain query-only. Do not call shell tools, write analysis scripts, or
start a Run unless the user explicitly asks for a new computation or a saved
report artifact. If a registered Workflow matches the user's requested
deliverable, start that Workflow instead of reconstructing its stages ad hoc.

For `knowledge.provider = remote`, stop at resolver status `needs_probe` unless
the fixed `depmap_query` tool has confirmed service health. Do not translate a
remote endpoint into ad-hoc browser or shell requests, and never place a token
in the project manifest.

Do not load whole RDS blocks into the conversation. Do not treat absence from a lineage result as biological wild type: it commonly means Mut≥3/WT≥5 was not satisfied. Negative Gene Effect means stronger dependency; lower PRISM AUC means greater drug sensitivity.

For module names, result fields, and direction rules, read [references/schema.md](references/schema.md).

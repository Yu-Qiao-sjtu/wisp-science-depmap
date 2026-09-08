---
name: depmap-coding-agent
description: "Develop and run R-first DepMap analyses from project-local release data and existing R reference scripts. Use for gene dependency, co-dependency, contextual dependency, mutation/CNV/expression associations, drug response, synthetic-lethal hypothesis generation, cross-library validation, and reproducible DepMap figures. Keep large matrices in the execution layer, load only relevant reference code and Skills, and return compact evidence with Run and Artifact provenance."
license: AGPL-3.0-only
tags: [depmap, r, crispr, cancer-dependency, coding-agent]
wisp:
  schema_version: 1
  domains: [bioinformatics, oncology, genomics, transcriptomics]
  research_stages: [analysis, hypothesis, validation, synthesis]
  roles: [analyst, planner, validator, synthesizer]
  evidence_types: [project-data, omics, computational]
  outputs: [analysis-module, hypothesis-card, validation-plan]
  side_effects: code_execution
---

# DepMap R Coding Agent

Turn a biological question into a reviewable, reproducible R analysis while
preserving the project's existing DepMap methods. Existing R scripts are the
reference implementation and capability specification: reuse their data
semantics, joins, statistical choices, plots, and interpretation boundaries.
Do not blindly execute a whole example script, and do not replace its method
with a Python rewrite merely because Python is available.

## Language and authority

- Use **R for DepMap data loading, statistics, and plotting** by default.
- Python may perform small engineering tasks such as catalog generation or
  format validation only when it does not change the scientific method.
- Use Python for a scientific calculation only when the user asks for it or a
  required method has no viable R implementation. State the reason and the
  validation needed against the R reference.
- Treat `data/README.txt` as the release-level data dictionary and
  `tm00-script/scripts/` as the canonical reference-code directory when they
  exist in the active project.
- Never modify files under `data/` or the canonical reference-script directory.
  Generate task-specific code and results under `analysis/depmap-agent/`.

## Context boundary

The presence of a large file in the project does not justify reading it into
the model context.

- Inspect paths, byte sizes, headers, dimensions, release metadata, and a tiny
  bounded sample before analysis.
- Do not return full matrices, long tables, complete logs, or binary outputs as
  tool text. R reads the data; the model receives compact JSON summaries and
  project-relative Artifact references.
- Load only the reference scripts needed for the current question. Read
  `references/capability-manifest.json` and
  `references/reference-script-map.md` to route the request before opening R
  source files. The manifest is the machine-readable result of reverse-auditing
  the 26Q1 scripts; do not invent a global "core five files" requirement.
- Search and load other enabled Skills only when they are needed for a distinct
  stage. Analysis Skills guide computation; literature Skills guide a separate
  evidence task; grant/paper-writing Skills are loaded only after analysis and
  validation are complete.

## Required workflow

### 1. Establish the project contract

In a new conversation, call `depmap_project_runs` first. Reuse or continue a
matching active/succeeded Run instead of recomputing because the chat context is
empty. Run identity and artifacts belong to the project cycle, not one session.

Confirm that the active project contains the expected `data/` and
`tm00-script/scripts/` directories. Run the bundled read-only resolver before
generating code. First inspect the full capability matrix, then resolve the one
capability selected for the question:

```powershell
Rscript skills/depmap-coding-agent/scripts/inspect_depmap_project.R \
  --project-root . \
  --output analysis/depmap-agent/project-inspection.json

Rscript skills/depmap-coding-agent/scripts/inspect_depmap_project.R \
  --project-root . \
  --capability mutation_to_target \
  --output analysis/depmap-agent/dependency-plan.json
```

When the bundled Skill is installed outside the project, use the absolute
inspector path reported by `use_skill`, while keeping `--project-root .`.
Review the inspection result before generating analysis code. Interpret its
status exactly:

- `ready`: all direct inputs for that capability exist;
- `preprocessing_required`: source data exist but one or more RDS objects used
  by the reference scripts must be materialized first;
- `missing_inputs`: a true raw or user-supplied input is absent, so only that
  capability is blocked.

Never report the whole project as unready because an unrelated modality is
missing. An unrecognized release or `missing_inputs` is an explicit blocker for
the affected capability, not permission to guess replacements.

### 2. Classify the question

Write a compact analysis specification containing:

- normalized target gene(s);
- biological question and claim strength requested;
- required modalities;
- cohort/lineage filters;
- reference-script IDs;
- statistical tests, covariates, multiple-testing rule, and thresholds;
- expected tables, figures, JSON summaries, and validation checks.

Save it as `analysis_spec.json` before execution. If a result-changing choice
is scientifically ambiguous, ask the user rather than silently selecting it.
Copy the selected capability ID, dependency-plan path, and exact execution plan
into the specification.

For expression-to-dependency questions, distinguish the requested operation:

- “which dependency genes/targets” selects the precomputed association query;
- “which pathways”, “enrichment”, “GSEA”, or “biological processes” plus an
  expression source gene selects the `gene_to_dependency` capability with its
  `pathway_enrichment` operation;
- do not trigger GSEA when the expression source gene is missing or when the
  user only asks about one gene pair.

Read `analysis-modules/表达基因-CRISPR基因依赖相关性分析/module.intent.json`
for the `semantic_routing` synonyms, guards, and positive/negative examples.
These are semantic guidance, not a keyword-triggered execution engine. Resolve
an execution request, an expression source gene, the expression-to-CRISPR
dependency relationship, and pathway intent together. Negation, deferred work,
quoted examples, method explanations, and capability-only questions do not
authorize execution. “继续做富集” may inherit the gene, module, cohort and method
only from an unambiguous active analysis; explicit user values override context.
Clarify missing or conflicting measurement roles. Drug sensitivity is a
different analysis. Explicit ORA is not GSEA, and GO/KEGG must not silently
become Hallmark. Preserve low-expression requests and the declared NES sign.
Record the resolved fields and any defaults in `analysis_spec.json`.

The global enrichment capability executes its declared `executable_entrypoint`
over one complete precomputed correlation row and reuses an exact-parameter
cache. Its defaults are Hallmark, `negative_signed_t`, and an 80% pair-count
threshold. Positive NES means higher source expression associates with
stronger, more-negative Gene Effect dependency. When a lineage is requested,
the existing `lineage_gene_enrichment` result is a rank-sum enrichment query,
not GSEA. If the user explicitly requires lineage GSEA, generate the full
lineage-specific dependency ranking first; never run GSEA on the sparse Top-100
network alone.

### 3. Retrieve reference methods progressively

Use `references/reference-script-map.md` to select the smallest relevant set of
R scripts. Read only the relevant functions or sections. Preserve important
behavior such as identifier normalization, ModelID alignment, effect direction,
group definitions, correlation method, multiple-testing correction, plot
encodings, and missing-value handling.

Example genes in the reference scripts are test fixtures and method examples;
parameterize them for the current question instead of treating their biological
conclusions as universal.

### 4. Generate task-specific R code

Create a new run directory:

```text
analysis/depmap-agent/runs/<timestamp>-<short-task>/
├── analysis_spec.json
├── scripts/analysis.R
├── run_manifest.json
├── result.json
├── qc.json
├── tables/
├── figures/
└── logs/
```

The generated R script must:

- resolve paths from the project root or an explicit `DEPMAP_DATA_ROOT`;
- use `file.path()` rather than embedding a machine-specific absolute path;
- fail clearly when required inputs or columns are missing;
- avoid loading modalities not required by the analysis specification;
- record input paths, byte sizes, release, parameters, package versions, and
  the reference scripts used;
- write compact `result.json` and `qc.json` plus machine-readable tables;
- render figures to files rather than returning plot data through stdout.

When the selected capability declares an `executable_entrypoint`, use that
reviewed script with the resolver-provided defaults and user overrides instead
of regenerating its scientific calculation. Record the exact invocation in the
run manifest and declare the cache result package as outputs.

When the dependency plan reports `preprocessing_required`, generate a
task-local preparation module under the run directory. It must implement the
audited producer operations and output schemas while replacing hard-coded
paths with project-relative parameters. Do not edit or blindly execute the
canonical example script. One Run may execute preparation followed by analysis
when they share the same approved input set; preserve the ordered
`execution_plan` in `run_manifest.json`.

For very wide matrices, prefer `data.table::fread(select=...)`, chunked work, or
a validated project-local cache when the required operation permits it. Do not
claim that partial column reads preserve a method unless the selected columns
are sufficient for that method.

### 5. Execute through the Run Manager

Use a persisted `run_in_context` Run with an R preflight and exact
project-relative output specifications. Long computation belongs to the Run
Manager, not to a child Agent and not to an extended interactive shell timeout.
Submit one Run for one approved analysis specification. Monitor that Run; do
not resubmit it because it is slow.

The Run submission must declare only the `required_datasets` returned by the
resolver as `input_paths`, plus the generated scripts/specification. Declare
`run_manifest.json`, `result.json`, `qc.json`, tables, figures, and logs as
`output_specs`. This is the handoff from model orchestration to deterministic R
execution: raw matrices never become prompt attachments or tool-result text.

Default to one heavy DepMap R Run at a time. Independent literature retrieval
or read-only review may run concurrently, but two multi-gigabyte matrix scans
should not run in parallel without an explicit resource decision.

### 6. Validate before interpretation

Apply the checks in `references/result-contract.md`. At minimum verify:

- release and exact input files;
- gene and ModelID resolution;
- sample counts before and after filtering;
- missingness and duplicated identifiers;
- Gene Effect versus Dependency Probability semantics;
- effect and drug-response direction;
- lineage, expression, and copy-number confounding where relevant;
- multiple-testing correction;
- output files and figure readability.

When the native `depmap_validate_run` tool is available, call it with the
persisted Run id and project-relative run directory after the Run succeeds.
Only `state=run_validated` authorizes numerical interpretation. A successful R
stdout message, the mere existence of a figure, or `result.json` by itself is
not sufficient.

Do not describe correlation, differential dependency, or a screen-derived
candidate as causal or synthetic lethal without independent experimental
evidence. Label it as an observational hypothesis.

### 7. Load other Skills and delegate narrowly

- Use `search_skills` with biological or output-stage terms only after the
  DepMap analysis need is classified.
- Load no more Skills than the current stage needs. Never load the full catalog.
- A literature child receives the biological question and compact observations,
  not raw matrices or the full R log.
- A reviewer receives `analysis_spec.json`, generated R code, `result.json`,
  `qc.json`, and provenance identifiers.
- A grant or manuscript-writing child receives only validated evidence and
  explicit caveats. It must not change the underlying statistics.
- Keep the main DepMap Agent responsible for the user-facing answer.

Use child Agents only when delegation is available and the work is independent,
such as literature evidence and a result audit. Do not create child Agents for
ordered preprocessing, the main R calculation, Run monitoring, or final
scientific judgment. If delegation is unavailable, perform those bounded
interpretation stages sequentially with the same compact evidence package.

## Final response contract

Report separately:

1. executed analysis and exact release;
2. observed results with sample counts and statistics;
3. validation and warnings;
4. biological interpretation;
5. hypotheses requiring experimental validation;
6. generated script, Run, table, figure, and Artifact references.

If execution has not completed successfully, do not write a scientific result
as though it had.

# DepMap Agent

## Knowledge annotation bridge

The Agent does not infer scientific coverage from the roughly 180 GiB storage
size or from directory names. The versioned
`skills/depmap-knowledge-query/references/knowledge-module-annotations.json`
registry separates:

- physical storage modules and their terminal manifests;
- scientific capabilities and their supported questions;
- input entity roles and reusable entity sets;
- statistical metrics, direction rules, and multiple-testing families;
- query exposure, allowed claims, and forbidden claims.

`depmap_describe_capabilities` joins this registry to the installed knowledge
root at query time. It returns a metadata plan, not evidence hits. In
particular, `dorothea_tf_abc` represents a 270-gene transcription-factor entity
set that can fan out through compatible source, target, event, feature, and
regulator roles. It must not be reduced to the DoRothEA enrichment table.

The intended query path is:

`structured intent -> entity/entity-set resolution -> annotated capability plan -> bounded evidence queries -> evidence aggregation -> model interpretation`.

Before a DepMap Specialist publishes its final answer, a host-enforced
Grounding Gate validates every submitted data claim against the current-turn
Evidence Ledger. Quantitative claims carry an invisible structured binding and
a visible `[E#]` marker; the binding names an exact `evidence_id` and RFC 6901
JSON Pointer. Unsupported or stale numbers, positive use of coverage-gap
evidence, hidden sparse-result states, and unqualified causal/clinical
overclaims are rejected without ending the turn. A successful answer receives
a compact machine-verified evidence index. This is a runtime boundary, not a
prompt-only convention. DepMap sessions also declare a kernel-level
completion contract: ordinary assistant prose is withheld and not persisted,
then recycled as an unpublished draft until the model submits it through the
validated `attempt_completion` tool. Switching models therefore cannot bypass
the gate by omitting the tool call.

The ledger and gate are source-aware rather than DepMap-file-only:

- DepMap and the precomputed TCGA expression/survival bridge are registered by
  `depmap_query` / `depmap_evidence` and may support exact pointed values.
- A successful `depmap_validate_run` now registers the QA-passed manifest,
  result, and QC documents as `run_validated`; failed validation never creates
  positive Run evidence.
- Completed delegated tasks that explicitly declared the
  `literature_search` capability are registered with their workflow and child
  tool provenance. Only deliveries containing a traceable DOI, PMID, URL,
  paper id, or source reference become `literature_retrieval`; other deliveries
  are `literature_unverified` and cannot support a positive claim. Literature
  locator validation establishes traceability, not automatic entailment of a
  paraphrased scientific claim.

> **Architecture note:** The normative Agent-first orchestration, execution
> levels, Evidence Ledger, MCP boundary, and Workflow escalation policy are
> defined in [depmap-agent-engineering-framework.md](depmap-agent-engineering-framework.md).
> Registered Workflows remain available for durable multi-stage work, but an
> ordinary DepMap query or initial topic exploration should be handled by the
> conversational Agent first.

Wisp Science includes a selectable **DepMap Agent** Specialist. It owns the
project-level scientific workflow: understand the question, route to validated
precomputed evidence or a new R analysis, monitor the Run, enforce QA, load
stage-specific Skills, and preserve provenance. The knowledge base is an
evidence backend and cache; it is not the Agent itself.

## Project layout

The writable research project, read-only knowledge base, and read-only raw data
are separate roots. A project can point to local assets without being the data
directory itself:

```text
<project>/
├── .wisp/
│   └── depmap-agent.json
└── analysis/
    └── depmap-agent/
```

`depmap-agent.json` records a local `knowledge_root`, `data_root`, and
`analysis_root`, or a versioned remote `knowledge.provider`, `endpoint`, and
`release`. Environment overrides support portable development and deployment.
The server contract is documented in the bundled DepMap knowledge-query Skill.
Credentials and remote tokens belong in the system keyring, never this file. A legacy
project-local `data/`, `tm00-script/`, and `knowledge/` layout remains
recognizable but is not the product contract.

The DepMap Agent and local DepMap/TCGA MCP do not create SSH sessions or
tunnels. Users connect research servers through Wisp Science's ordinary
ExecutionContext UI. The knowledge query layer accepts local files or an
already reachable HTTPS/loopback endpoint only; a project containing
`knowledge.tunnel` is rejected with an explicit configuration error. This keeps
server access separate from scientific evidence retrieval.

The Agent never changes raw data, the knowledge base, or canonical reference scripts. It writes
task-specific R source, manifests, JSON summaries, tables, figures, and logs
under `analysis/depmap-agent/`.

## Starting a session

Open **Settings → Specialists → DepMap Agent**, then choose **Start specialist
session**. This creates a new conversation already bound to the DepMap Agent;
type the biological question normally, for example “分析 ESR1 的共依赖，并给出可复现图表”.

For persistent project behavior, open **Project settings → Default Agent** and
select **DepMap Agent**. Every future conversation in that project inherits the
Agent automatically. Existing conversations keep their frozen identity, and a
conversation branch inherits the source conversation's Agent.

Each DepMap conversation is Agent-first and chooses the smallest bounded entry
point needed:

The semantic bridge is declared in
`skills/depmap-knowledge-query/references/agent-capability-registry.json`.
It is the single machine-readable source for supported intents, entity
requirements, execution levels, recommended bounded queries, query modes,
metric families, and claim boundaries. Rust validates and loads this registry
on first use; the route schema, query-mode schema, required fields, default
limits, and returned `capability_contract` are derived from it. The model still
extracts intent and entities from natural language, while the host—not the
prompt—decides which stored capability that meaning may invoke.
The same registry covers the dedicated MCP evidence tools for frozen molecular
subtypes, constrained coamplification, True Love reciprocal screens,
observational synthetic-lethal screens, and 3D analysis families. An
unrecognized cancer term produces a `needs_resolution` route to
`depmap_resolve_entity`; it is not passed through as if it were a valid storage
label. Exact maintained aliases remain deterministic vocabulary data, while
ambiguous or model-proposed mappings still require the resolver contract.

Scientific entities have a separate, versioned source of truth at
`skills/depmap-knowledge-query/references/scientific-entity-registry.json`.
The generic `depmap_resolve_entity` MCP tool resolves cancer, gene, drug,
pathway, phenotype, molecular-focus, mechanism, evidence-source, and requested-
output mentions through one `wisp.entity-resolution.v1` result shape. Gene and
drug resolution verifies installed `gene_catalog.parquet` and PRISM compound
catalogs. A pathway spelling may be normalized for a later query but remains
`NORMALIZED_UNVERIFIED` until the provider verifies it. Entity resolution is
control data, not scientific evidence, and `NOT_FOUND` never means negative
biology. `depmap_resolve_lineage` remains available for compatibility.

Topic-design requests use a multi-slot semantic contract instead of collapsing
the whole question into one `focus` string. The route preserves the user's
disease wording and canonical lineage separately, plus arrays for phenotype,
molecular focus, mechanism, evidence source, and requested output. It also
records an execution policy and any unresolved concepts. For example, “肝癌、
肿瘤细胞干性、转录因子、课题方向” becomes `Liver`,
`tumor_cell_stemness`, `transcription_factor`, and `candidate_topics`.
`depmap_query(mode=topic_plan)` then reports coverage for every slot and their
intersection. A phenotype marked `NOT_COMPUTED` remains a visible coverage gap;
it is never silently discarded and never converted into a negative biological
claim. Under `precomputed_only`, proposed calculations are described but not
started.

The route now emits a versioned `wisp.scientific-intent.v1` object, and
`topic_plan` compiles it into `wisp.evidence-plan.v1`. The corresponding JSON
Schemas live under
`skills/depmap-knowledge-query/references/schemas/`. Molecular-focus routing
hints come from the capability registry rather than API conditionals. In
particular, `transcription_factor` resolves the frozen DoRothEA A-C regulator
set from the installed release and projects that set across every compatible
gene role in lineage networks, expression-dependency, CNV, PRISM, and TF
enrichment. It no longer means "query only the DoRothEA enrichment table".
Physical Parquet partitions remain provider implementation details and are
never exposed as planning steps for the model.

1. `depmap_agent_route` records one typed, host-validated L1/L2/L3/L4 decision
   for each new request; the route is observable control data, never scientific
   evidence;
2. ordinary query, comparison, interpretation, and initial topic-exploration
   requests use the bounded query tools directly; semantic similarity to a
   registered Workflow does not by itself start that Workflow;
3. a registered Workflow is proposed only when the user explicitly requests
   it or the task requires durable multi-stage execution, independent review,
   or formal artifacts. The Agent creates an approval draft before those tasks
   execute, and a blocked launch never triggers manual reconstruction;
4. `depmap_project_runs` is used only when the user asks to continue, inspect,
   validate, or create computation; query-only turns do not load Run history.
5. for a cancer-only availability question,
   `depmap_query(mode=lineage_catalog)` inventories canonical lineage manifests
   without inventing an anchor gene; for a cancer-only dependency-gene ranking,
   `depmap_query(mode=lineage_dependency)` reads the existing lineage-vs-rest
   dependency-gene ranking. `depmap_query(mode=lineage_directions)` reads the
   bounded multi-family topic candidates for a simple cancer-only direction
   request. Multi-dimensional topic requests use
   `depmap_query(mode=topic_plan)` so phenotype, molecular focus, mechanisms,
   evidence sources, outputs, and execution policy remain independently
   inspectable. These modes query completed results and do not start a Run;
6. for a gene-plus-cancer question, `depmap_evidence` verifies the configured
   local or remote provider and assembles the requested evidence in one tool
   call; `depmap_query(mode=status)` remains available for provider-only tasks;
7. successful query and evidence calls persist a stable `evidence_ref` in the
   project SQLite ledger. `depmap_evidence_history` can recover the exact
   evidence in the current conversation without treating model memory as data.

When a route returns a `single_call` recommendation, the host also installs a
single-use next-tool argument contract. The model may ask the user or finish,
but it cannot silently broaden `focus`, increase `limit`, change the canonical
lineage, or substitute another query. A mismatched call is rejected before the
provider executes; after the exact call runs, that evidence tool is removed
from the active route so it cannot be repeated in the same turn.

This provides multi-session inheritance of identity and work state without
copying old chat transcripts into the model context.

The equivalent one-session path is **New session → Agent menu → Specialist →
DepMap Agent**. Specialist selection is locked after the first message so its
instructions and resource boundary remain stable.

The Specialist always has `depmap-knowledge-query` and `depmap-coding-agent`
available even when the project's ordinary Skill subset excludes them. When the
knowledge source reports PASS, it first loads `depmap-knowledge-query` and
retrieves only bounded rows from the on-disk matrices. It loads
`depmap-coding-agent` only after a coverage gap explicitly transitions to new
or recomputed analysis, then
loads other Skills needed for the current biological question or output stage. Literature, evidence-audit,
figure, manuscript, and grant-writing Skills are not preloaded into every
analysis turn.

The harness covers the original aggregate modules and the newer sparse
lineage modules. In addition to core, pair, top, mutation-lineage, pathway, and
pan-cancer drug queries, it retrieves cancer-lineage correlation and
co-dependency networks (including reciprocal pairs), CNV
amplification-to-dependency results, PRISM AUC associations, and Hallmark,
Reactome 2023.2, or DoRothEA enrichment rows.

The low-level query tool uses a flat model-compatible JSON schema with `mode`
required. Runtime validation still enforces every mode-specific field. This
avoids empty tool calls from models that cannot reliably serialize a large
`oneOf` schema.

The native `depmap_evidence(gene, lineage, sections, limit)` tool is the middle
layer between natural-language questions and stored modules. It performs at
most 13 bounded subqueries with concurrency capped at three, returns no more
than 10 retained rows per subquery, caps the assembled payload, and preserves
the exact query, release, result provenance, scope, and coverage state. Core and
pan-cancer mutation scans are explicitly distinguished from lineage-scoped
network, CNV, drug, enrichment, and TCGA expression-survival evidence. It never
starts computation.
Its compact `focus.core` field puts the current gene and requested-lineage
descriptive summaries before the large section payload, and explicitly carries
no lineage-vs-rest test, rank, subtype, or causal interpretation.
Canonical lineage labels are DepMap model-grouping proxies, not exact clinical
histologies or patient cohorts. Topic design keeps that proxy distinct from the
user's disease wording and does not add control lineages or named cell lines
without current metadata.
The maintained Chinese alias table covers all 34 canonical lineage groups and
common disease-name variants at the desktop and MCP query boundaries. The
canonical lineage recorded in returned evidence is authoritative for storage
lookup; an unknown or clinically ambiguous future synonym remains a reported
scope gap rather than being guessed.
Natural-language lineage handling is exposed as a separate read-only resolver.
Exact aliases select a canonical proxy; broad terms return an explicit candidate
set; model-proposed candidates are vocabulary-validated but remain unconfirmed.
Lineage evidence must not run until the resolver has produced a selected label
or the user has explicitly supplied one of the candidates.
Each entry also names its metric semantics: mutation and CNV values are group
mean differences, while network values are correlations. The Agent must not
relabel one as the other.

Sparse queries return `FOUND`, `NOT_RETAINED`, `INELIGIBLE`, `NOT_COMPUTED`,
or `MODULE_UNAVAILABLE`. These states prevent a missing retained row from
being reported as a biological negative. A gene evidence view is assembled
from several bounded results at request time; a full gene-by-lineage evidence
card corpus is not required.

The 26Q1 local and server knowledge stores contain two QA-complete analysis
modules exposed through dedicated bounded MCP tools:

- `coamplification_dependency`: observed double-amplification catalogs,
  exhaustive high-confidence target scans, and a lineage-adjusted layer;
- `subtype_dependency`: 27 eligible OncoTree subtype contrasts and 6 frozen
  model-feature contrasts, each with complete 18,531-target coverage.

`depmap_subtype_evidence` uses only eligible catalog entries and exact frozen
contrast identifiers. `depmap_coamplification_evidence` uses only constrained
high-confidence directional pairs. Both preserve manifest/file provenance and
coverage states; neither scans raw matrices, starts computation, or invents a
subtype/pair outside the stored contract.

## Execution and context

Large CSV and knowledge-matrix files remain on disk. The model receives paths,
metadata, small bounded samples, structured result summaries, and Artifact
references. A normal knowledge query does not start a heavy Run. New or changed
analysis uses a persisted Run instead of an interactive shell
timeout. By default the Agent submits only one heavy DepMap matrix Run at a
time; independent literature or read-only review can be delegated separately
when delegation is enabled for the conversation.

The bundled read-only resolver validates the project and all 21 reverse-audited
script capabilities without loading matrix rows:

```powershell
Rscript <bundled-skill-path>\scripts\inspect_depmap_project.R `
  --project-root . `
  --output analysis\depmap-agent\project-inspection.json
```

The exact bundled path is listed when the Agent loads the Skill.

It does not declare the whole 35+ GiB directory to be one input. For a selected
capability, run it again with `--capability <id>`. The result is one of:

- `ready`: the capability's direct inputs exist;
- `preprocessing_required`: raw sources exist, and the ordered plan identifies
  which audited R operations must create task-local derived objects first;
- `missing_inputs`: a true raw or user-supplied input is absent, blocking only
  that capability.

The selected plan supplies the exact `input_paths` for the persisted Run. R
reads those files from disk and writes compact JSON, tables, figures, and logs;
the model never receives the large matrices themselves.

Numerical claims in a DepMap answer must come from a successful query response
in the current turn. The answer preserves release, manifest, sample counts,
retention rule, and provenance, and separates returned facts from biological
interpretation and hypotheses. If the provider is blocked, the Agent stops
instead of filling in plausible-looking statistics.

## Orchestration

The main DepMap Agent owns question classification, dependency resolution, the
analysis specification, Run submission and monitoring, validation, and the
final answer. Ordered preprocessing and calculation execute in one deterministic
R Run when possible. Independent literature retrieval or result review may use
child Agents, which receive only the question and compact result package.
Biological, literature, figure, manuscript, and grant Skills remain hot-loadable
from the project's enabled Skill catalog and are loaded only for their current
stage.

The routing lifecycle is explicit:

```text
precomputed_query
  |-- covered --> bounded query --> validated answer
  `-- not covered --> coverage_gap --> new_analysis_proposed
                                      --> new_analysis_authorized
                                      --> persisted R Run
                                      --> run_validated
                                      --> interpretation / knowledge update
```

The native `depmap_validate_run` gate requires `RunStatus=succeeded`, exit code
0, declared `run_manifest.json`/`result.json`/`qc.json` outputs, R language,
release and target identity, coherent cohort counts, and passing QC. A file on
disk is not treated as a validated result merely because it exists.

A missing path, unavailable service, or unsupported cohort is reported as such.
The installed `tcga_expression_survival` mode is an explicit, precomputed
patient-cohort module; its use is not a silent substitution. Other TCGA
analyses, arbitrary Python analysis, and untracked scans of raw matrices remain
outside this permission boundary.

## TCGA expression and survival bridge

The first TCGA integration stage maps the exact 18,531 DepMap CRISPR target
genes to all 33 TCGA projects without copying the large public dataset into a
Wisp project. Raw files remain remote references under `/refdir/database`; the
knowledge root contains only compact Parquet associations, manifests, and QA.

For each project the builder aligns genes by Ensembl gene ID with an explicit
gene-symbol fallback, selects primary-tumour sample type `01` (or LAML primary
blood-derived cancer type `03`), retains one sample per patient, transforms TPM
to `log2(TPM+1)`, and links the patient
barcode to OS, DSS, DFI, and PFI. Each gene/endpoint uses a univariate Breslow
Cox score test at beta zero when at least 30 patients and 10 events are
available. BH FDR is calculated within one project and endpoint. Positive score
z means higher expression is associated with higher event hazard; it is not a
hazard ratio, a multivariable estimate, or evidence of causality. DepMap cell
lines and TCGA patient samples are never joined; the bridge is gene- and
cancer-label-centric.

Mutation and CNV are deliberately outside this first stage and remain visible
future coverage rather than being inferred from the expression-survival rows.

## Agent harness behavior

The scientific Skill does not own provider tuning or turn termination. Those
controls live in the shared Agent harness:

- model profiles may explicitly select reasoning effort; when unset, an exact
  model-id adapter may supply a conservative default (currently
  `GLM-5.3-Flash` uses `minimal`);
- after a tool result, the UI receives coarse phase events for model reasoning,
  final synthesis, and bounded recovery without exposing chain-of-thought;
- a post-tool response that exceeds the model policy deadline is cancelled and
  retried once with only `attempt_completion` available, so recovery cannot
  launch another evidence search;
- query defaults and normalized lineage labels are recorded in the returned
  `query` object and remain the authoritative effective parameters.
- scientific focus is a controlled entity set, not a guessed gene list. A
  TF-focused request resolves the release's `DOROTHEA_TF_ABC` regulator set and
  projects those genes over every compatible precomputed family rather than
  limiting retrieval to one enrichment table;
- phenotype and molecular slots are connected by an explicit research
  relation. For example, stemness plus TF means “find TF candidates associated
  with stemness,” not two independent filters. When a direct stemness score was
  not precomputed, the planner may use only registry-declared pathway proxies
  and must label them `DECLARED_PROXY`; WNT, NOTCH, Hedgehog, EMT, TGF-beta,
  Hippo/YAP, MYC, or pluripotency context is not promoted to a direct stemness
  measurement or causal regulation claim;
- a `NOT_RETAINED` result means no row passed that stored filter; it does not
  prove that the requested biology is absent.

Explicit user model settings always override adapter defaults. The bounded
recovery is a failure path, not a replacement for normal multi-tool research.

### Relation-driven semantic bridge

Topic discovery is compiled as a typed relation rather than a bag of filters:

```text
molecular focus --predicate--> phenotype
```

The predicate defaults conservatively to `candidate_association_with`. A
mechanism concept may declare a more specific predicate in the capability
registry, such as `candidate_regulator_of`; the model must not infer a causal
predicate from a generic request. Concepts may also declare proxy evidence and
a data adapter. The generic planner reads those declarations and produces four
separate evidence classes: direct, declared proxy, supporting/composable, and
new computation required.

This separation is the extension contract. Adding a new scientific case should
normally require:

1. a canonical concept and aliases;
2. optional declared proxies with claim boundaries;
3. an entity-set resolver or query adapter already supported by the provider;
4. one semantic regression test and, when data exists, one result test.

It must not require a phenotype-named branch in the planner. Unsupported
concepts remain unresolved, malformed adapters fail closed, and absence from a
sparse retained table is never converted into a biological null.

## Scientific boundary

Reference scripts define capabilities and established analysis patterns; their
example genes and historical release labels are not automatically copied into
new conclusions. Generated analyses must record the active release, exact
inputs, cohort and sample counts, statistical methods, correction rules,
software versions, reference scripts, and validation warnings. Observational
associations and screen-derived candidates remain hypotheses until supported by
independent experimental evidence.

## Gene-to-cancer topic workflow

The built-in **DepMap gene-to-cancer topics** Workflow turns one resolved gene
and an optional cancer scope into a ranked, reviewable topic report. Run it by
name from a DepMap Agent conversation, for example:

```text
Run the DepMap gene-to-cancer topics workflow for KRAS in lung cancer.
```

The graph keeps large data in the query layer and executes eight bounded tasks:

1. the DepMap Specialist inventories which analysis families are actually
   eligible in the requested cancer scope;
2. the DepMap Specialist queries validated project evidence and records exact
   numbers, sample sizes, release provenance, limitations, and coverage gaps;
3. an independent literature task maps established, contested, and open claims;
4. a synthesis task proposes three to six falsifiable cancer topics;
5. an innovation reviewer checks prior-art collision, mechanistic novelty, and
   whether the DepMap angle is genuinely differentiating;
6. a feasibility reviewer scores data coverage, statistics, experiments, cost,
   reproducibility, and failure modes;
7. a clinical-translation reviewer independently scores biomarkers,
   stratification, actionability, models, and translational barriers;
8. a final task ranks the topics and returns a figure plan, manuscript-section
   blueprint, caveats, and the few questions needed for the next conversation.

The literature task keeps a claim ledger. Search snippets, AI summaries, title
matches, and reference-list mentions remain candidate leads until the primary
abstract or full text and a stable PMID, PMCID, DOI, or publisher URL verify
them. Later corrections mark the old claim contradicted or retracted so it
cannot flow into topic generation. Search depth remains adaptive: evidence
saturation, unresolved claim classes, and explicit coverage gaps determine when
the node stops, not a hard-coded call count. A negative search is reported only
as “not found within the searched scope”; it is never proof that nobody has done
the work or that a gap is unique.

For requests that ask which existing data support a proposed study, the Agent
uses the `study_support_mapping` route and a bounded cancer catalog query. Each
study claim is separated into direct precomputed evidence, new computation from
available inputs, missing data/coverage, or literature-only/unverified support.
This inventory never falls back to shell, an SSH Run, or guessed filesystem
paths. Installed modules show availability, not that a proposed TF-high
subgroup, drug combination, mechanism, or cross-platform integration has
already been analyzed.

The evidence nodes use the policy-scoped `depmap_read` capability. It grants
the delegated DepMap Specialist only the bounded `depmap_evidence` and
`depmap_query` tools; it does
not grant raw matrix reads, project writes, or code execution.
The DepMap specialist's required `depmap-knowledge-query` Skill participates in
Workflow resource discovery even if the project's ordinary enabled-Skill subset
is empty, so the main Agent and delegated evidence nodes see the same bounded
provider capability.

### Custom research modules and command phrases

Built-in Workflows provide a tested default, but they are not the only allowed
research implementation. Duplicate a built-in Workflow in Workflow Studio, then
replace its literature node with any enabled project Skill or configured
literature/external-research connector. The approved run snapshots the resolved
Skill, connector, model, capabilities, and task graph for reproducibility.

A custom Workflow's exact saved name is also a natural-language command phrase.
For example, after saving a module named `谷歌浏览器调研`, the user can say
`使用谷歌浏览器调研调查 PTK7 与肝癌` without adding the word `Workflow`. Wisp
requires an execution verb and one exact template name; mentioning a name while
asking what it does does not start it, and ambiguous or partial matches fail
closed.

An explicit request to use the user's real Chrome/Chromium session can be handled
either by the main Agent or by a Native Workflow node with the separately
approved **Real browser research** (`browser_research`) capability. That grant
exposes only browser setup, tab opening, page scanning, bounded page JavaScript,
plus host-persisted structured research-progress checkpoints. Browser actions
continue to use the same persistent bridge, URL block/preference rules,
provenance log, and task budget. The capability is never inherited from
`literature_search` or `external_research`, is not granted by default, and is not
available to ACP/external executors that cannot share the in-process browser
bridge. Neither browser text nor model memory becomes literature evidence until
publication identifiers are verified and returned with the claim.

Native `literature_search`, `external_research`, and `browser_research` tasks
all expose host-persisted phase and evidence-coverage checkpoints in the Agents
panel. This progress contract does not grant Chrome access to ordinary
literature or external-research tasks.

This first-stage Workflow proposes and audits topics. After the user selects a
topic, run the built-in **DepMap selected-topic report** Workflow with the exact
gene, cancer scope, topic id, and output language (`zh` or `en`). It re-queries
and freezes the bounded evidence, creates evidence-backed figures and captions,
writes Results and Methods, then assembles `report.md` and `report.html` under
`analysis/depmap-agent/reports/`. Report generation requires explicit approval
because it writes project files and may execute the configured Python or R
visualization runtime. Missing or unvalidated evidence remains a visible
limitation; the Workflow must not fill it with plausible prose.

For a breast-cancer direction, the intended conversation is:

1. supply a resolved gene plus `breast cancer` to **DepMap gene-to-cancer
   topics**;
2. review the cancer data inventory and three independent topic scores;
3. refine or select one topic over subsequent turns;
4. run **DepMap selected-topic report** in Chinese or English;
5. inspect the returned files and figures before publication use.

If the user supplies only a cancer type, the Agent may inventory available
analysis families, but it must request a resolved gene before claiming that a
gene-specific topic has been evaluated.

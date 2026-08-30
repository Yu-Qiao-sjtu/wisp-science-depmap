# DepMap Agent trajectory debug audit

This audit tracks the defects observed in `wisp_debug-2` (Debug #1),
`wisp_debug-3` (Debug #3), `wisp_debug-4` (Debug #4), and `wisp_debug-5`
(Debug #5). “Fixed” below means
the source and contract have been updated. Per the user's instruction, no Rust
compilation, test build, or installer build has been run for this batch yet.

## Build constraint

- Do not compile while trajectory debugging is still being collected.
- When the user explicitly authorizes a build, produce only the NSIS `.exe`.
- Do not build or deliver an MSI for this debugging cycle.

## Shared defects

| ID | Observed behavior | Root cause | Repair | Status |
|---|---|---|---|---|
| D01 | `Breast Cancer` and `Colorectal` returned lineage `NOT_COMPUTED` although `Breast` and `Bowel` outputs existed. | Natural-language cancer labels were used directly as storage keys. | Canonicalize known lineage names in the desktop query boundary, remote API, and R fallback. Includes `Breast Cancer → Breast` and `Colorectal/colon/rectal → Bowel`. | Fixed, uncompiled |
| D02 | Coverage summaries omitted the provider's reason. | `compact_gap` read only entry-level reason, not `result.reason`. | Preserve `/result/reason` in compact coverage output. | Fixed, uncompiled |
| D03 | Mutation `not_testable` was described as an upstream read failure. | The API kept the first R stack line instead of the scientific eligibility marker. | Extract the recognized eligibility/index line from stderr; normalize legacy stack-header responses in the desktop evidence layer. | Fixed, uncompiled |
| D04 | Important coverage summary appeared after large sections and was easily truncated. | Evidence JSON placed detailed sections before its compact summary and gaps. | Emit summary and coverage gaps before detailed sections. | Fixed, uncompiled |
| D05 | Aggregate mean/median differences were promoted to a bimodal distribution. | The interpretation contract did not explicitly prohibit distribution-shape inference from summaries. | Agent and Skill now state that mean/median/dispersion summaries cannot establish bimodality. | Fixed, uncompiled |
| D06 | ER or other molecular subtypes were inferred although no subtype field was returned. | Known gene biology leaked into the executed-observation layer. | Require subtype/receptor fields in the current result; otherwise label the idea as an external hypothesis. | Fixed, uncompiled |

## Debug #1-specific defects

| ID | Observed behavior | Root cause | Repair | Status |
|---|---|---|---|---|
| D07 | After a spill, grep scanned the entire `.wisp/tool-output` directory and mixed TP53/history into the ESR1 evidence. | The Agent followed the directory instead of the exact spill path. | Require bounded read/grep of the exact returned file only; never use the parent output directory as evidence scope. | Fixed, uncompiled |

## Debug #3-specific defects

| ID | Observed behavior | Root cause | Repair | Status |
|---|---|---|---|---|
| D08 | A cancer-only question was silently converted to `APC × Colorectal`. | The only high-level route required a gene, so the model invented an anchor. | Add `lineage_catalog` for canonical cancer-level module inventory without a gene; prohibit invented anchors. | Fixed, uncompiled |
| D09 | `depmap_query {}` failed repeatedly even after the model said it supplied `mode=status`. | The tool exposed a large nested `oneOf` schema that GLM-5.3-Flash did not serialize reliably. | Replace it with one flat schema with required `mode`; keep strict mode-specific validation in runtime. | Fixed, uncompiled |
| D10 | The same empty invalid query was retried several times. | No stopping rule existed for serialization/schema failures. | Allow one corrected call from the visible schema, then report blocked; never repeat the identical empty call. | Fixed, uncompiled |
| D11 | APC mutation dependency `value=-0.8649` was called `ρ=-0.865`. | Generic `value` lacked explicit metric semantics in the Agent-facing wrapper. | Attach `semantics.metric` and direction to every query/evidence entry. Mutation/CNV use `mean_difference`; networks use correlation. | Fixed, uncompiled |
| D12 | Wilcoxon/Kruskal-Wallis, FDR recalculation, subgroup tests, and a new gene-by-lineage matrix were called “pure query”. | Retrieval and computation boundaries were blurred. | Define query-only as selecting/interpreting existing returned rows. Any new matrix, test, FDR, model, or subgroup statistic is new computation. | Fixed, uncompiled |
| D13 | Drugs, MSI/BRAF subgroups, ER status, and actionability were attached without a returned drug/subtype result. | Biological memory was mixed with executed evidence. | Named treatments and actionability now require a current drug row or separately cited literature evidence. | Fixed, uncompiled |
| D14 | Pan-cancer mutation group contrasts were described as lineage co-dependency. | Scope and metric labels were not enforced together. | Preserve both `scope=pan_cancer` and `metric=mean_difference`; prohibit cancer-specific or correlation wording. | Fixed, uncompiled |
| D15 | Query-only direction discovery loaded 20 historical Runs and expanded context to 166.1k input tokens. | The Specialist rubric required project-cycle recovery for every DepMap turn; Run summaries included operational fields and fingerprints. | Skip Run history for query-only tasks and return compact Run identities only when Run work is requested. | Fixed, uncompiled |
| D16 | `Colorectal` lineage network/CNV/PRISM was declared absent, and new computation was recommended, despite complete `Bowel` manifests. | D01 propagated into planning. | Canonical `Bowel` lineage inventory now reads all eight module manifests before any compute proposal. | Fixed, uncompiled |

## Debug #4-specific defects

| ID | Observed behavior | Root cause | Repair | Status |
|---|---|---|---|---|
| D17 | The registered `depmap_gene_to_cancer_topics` Workflow failed immediately with `capability is disabled or unavailable: depmap_read`, although the DepMap specialist itself had query tools. Debug #5 proved the first repair was incomplete. | Discovery found the required Skill and marked `depmap_read` enabled, but `build_dynamic_delegation_policy` left `DelegationHostPolicy.available_skills` empty. The resolver therefore still rejected the capability. | Populate the host's exact available Skill/connector ids from the discovered resource catalog and assert that `CapabilityRegistry::available_ids` actually contains `depmap_read`, rather than testing only the enabled list. | Fixed, uncompiled |
| D18 | Before launching the matching Workflow, the Agent loaded historical Runs and queried a large evidence bundle, duplicating work and contaminating context. | Direct-query routing ran before registered-Workflow routing. | Registered-Workflow intent now has priority: call `start_workflow` first with only user-supplied gene/cancer specifics; do not query evidence or Runs first. | Fixed, uncompiled |
| D19 | After Workflow launch failed, the Agent manually reconstructed the stages, scanned files, loaded another Skill, and wrote an ersatz report. Debug #5 repeated the violation even after reading `manual_fallback_allowed=false`. | A JSON field and prompt rule could advise the model but could not prevent another model round. | Every registered-Workflow policy/draft blocker now returns `ToolControl::StopTurn`, which prevents later calls in the same batch and prevents a subsequent model round. | Fixed, uncompiled |
| D20 | Lowercase `liver` generated ten apparent lineage coverage gaps even though the current local knowledge base has all eight inspected `Liver` module manifests complete. | D01 also affected case-only canonical lineage resolution in the old executable. | Desktop, API, and R fallback canonicalize `liver → Liver`; a real local catalog smoke check found 8/8 modules available. | Fixed, uncompiled |
| D21 | The report used `17/33`, Wilcoxon `p=0.58`, and `FDR=0.70` from earlier memory, while the current evidence returned a descriptive Liver row with `effect_n=25` and no such test. | The requested lineage row was buried inside the large core result, and stale memory was allowed to fill missing statistics. | `depmap_evidence` now emits compact `focus.core.gene_summary` and `requested_lineage_summary` before detailed sections, with an explicit guard that it contains no rank or lineage-vs-rest test; Workflow tasks may not import ranks, p-values, or counts from memory. | Fixed, uncompiled |
| D22 | Five identical `depmap_query {}` calls were made across two rounds. | Same schema-serialization/retry defect as D09-D10. | The flat query schema and one-correction stopping rule apply to Workflow/topic turns as well; a Workflow-first regression forbids these redundant calls. | Fixed, uncompiled |
| D23 | Listing raw files was treated as proof that every reported coverage gap was locally computable. | Asset existence was conflated with cohort eligibility, identifier overlap, statistical power, and a validated computation path. | Agent, Workflow, and Skill now state that file presence proves availability only; eligibility and feasibility require resolver/manifests and, for new work, a validated Run. | Fixed, uncompiled |
| D24 | `damaging_mutation_n=10/1968` was called “致病突变”. | The provider's damaging-event definition was silently upgraded to clinical pathogenicity. | Contracts now require “provider-defined damaging event”; pathogenic/disease-causing language needs a separate clinical annotation source. | Fixed, uncompiled |
| D25 | A custom-missense top list with `n=12` and minimum `FDR=0.8927` was used to seed proteostasis, vesicle, drug, and combination-treatment mechanisms. | Nominal top rows were overinterpreted despite no multiple-testing-significant association. | If no row survives correction, the result must be reported as null; nominal targets may not seed a mechanism, drug, pathway, or synthetic-lethal claim. | Fixed, uncompiled |
| D26 | ISR/UPRmt biology, HCC treatment statements, druggability, TCGA/ICGC plans, and novelty claims were attributed to “literature” without any literature-search result or paper identifier. | Loading a generic clinical-translational Skill and model memory was mistaken for scholarly evidence. | The novelty task must return traceable paper identifiers for every downstream mechanism, treatment, novelty, or clinical claim; Skill text and memory are explicitly not literature evidence. | Fixed, uncompiled |
| D27 | A complete report was written to `results/reports/` even though the topic Workflow had not run or been approved and the registered flow only promises a blueprint before topic selection. | Manual fallback bypassed approval and artifact-stage boundaries. | Workflow-first routing prevents the write; launch creates only an `awaiting_user_approval` draft. Report generation remains a separate selected-topic Workflow after user choice. | Fixed, uncompiled |
| D28 | The user's broad “肝癌” scope was silently narrowed to HCC and `Biliary Tract` was added as a control. | A DepMap model lineage was treated as an exact clinical histology and the model expanded the scope from biological memory. | Contracts now distinguish the user's disease wording from the canonical DepMap proxy; `Liver` does not by itself prove HCC, and no neighboring control lineage may be added without evidence or user choice. | Fixed, uncompiled |
| D29 | Specific HCC cell lines were proposed although no current model-level metadata was queried. | Familiar cell-line names were supplied from model memory. | Named cell lines now require current model metadata or a validated Run; otherwise the plan remains at the cohort/panel level. | Fixed, uncompiled |

## Debug #5-specific defects

| ID | Observed behavior | Root cause | Repair | Status |
|---|---|---|---|---|
| D30 | `depmap_evidence` produced an 83,909-byte result, was spilled to `.wisp/tool-output`, and triggered many read/grep rounds; the turn consumed 292k input tokens. | The evidence fan-out allowed ten rows for each of twelve queries, retained full core lineage tables/manifests/indexes, while generic tool ingestion spills above 16 KiB. | The initial evidence view now caps each query at three rows, projects full provider responses into a compact contract, keeps requested-core focus/provenance, and directs any omitted detail to one surgical `depmap_query` follow-up. | Fixed, uncompiled |
| D31 | The report claimed all numbers came from the current turn while importing prior-session rank 17, Wilcoxon `p=0.58`, `FDR=0.70`, and Kruskal-Wallis statistics. | The manual fallback bypassed Workflow evidence isolation and stale project/session observations were used to fill gaps. | The host-enforced Workflow stop prevents the fallback. Current-turn-only rules remain in both the Specialist and Workflow task; the trajectory validator now detects the contradiction. | Fixed, uncompiled |
| D32 | Enrichment was called the only FDR-significant signal despite CARD14 co-dependency having `FDR=0.024`. | Significance was summarized within one preferred narrative rather than checked across returned modules. | Specialist, Workflow, and Skill contracts prohibit “only significant” wording when another returned section crosses the same adjusted-P threshold. | Fixed, uncompiled |
| D33 | A continuous expression-to-dependency enrichment became an `ATF5-high` discrete subgroup and selected patient/model class without a threshold. | Continuous association and categorical stratification were conflated. | A high/low subgroup now requires a returned grouping and threshold; otherwise wording must remain continuous. | Fixed, uncompiled |
| D34 | `not_testable` and `INELIGIBLE` were reported as mutation/CNV routes being biologically or project-level “infeasible.” | Provider eligibility state was promoted to a general scientific conclusion. | Contracts now require “not testable in this provider/cohort under current thresholds” and prohibit general infeasibility claims. | Fixed, uncompiled |
| D35 | `0/25` models below a dependency cutoff was promoted to “ATF5 is not a direct liver-cancer dependency.” | A descriptive threshold count was treated as a categorical causal/biological negative, without lineage-vs-rest inference. | Report the exact zero-at-threshold observation and avoid categorical dependency claims unless a validated analysis supports them. | Fixed, uncompiled |
| D36 | The Workflow context and downstream plan again drifted from broad user wording “肝癌” toward HCC/LIHC. | `start_workflow.context` accepted the model's paraphrase, so prompt advice could not prevent scope expansion before the Workflow was created. | DepMap Workflow drafts now bind the exact latest user message from SQLite and ignore the model's paraphrased context. The no-silent-narrowing interpretation guard remains for downstream tasks. | Fixed, uncompiled |

## Debug #6-specific defects

| ID | Observed behavior | Root cause | Repair | Status |
|---|---|---|---|---|
| D37 | `novelty_landscape` stayed “running” for ten minutes and then failed at the 600-second boundary; its descendants were blocked. | The literature task had no task-level tool budget and performed 35 persisted literature calls without reserving a final schema-valid synthesis turn. | Give the literature node a visible/editable Workflow tool budget, require deduplicated batched retrieval, and require verified partial coverage with explicit gaps when exhaustive coverage would exhaust the wall-time window. The scientific task text contains no fixed call-count stopping rule. | Fixed, uncompiled |
| D38 | The task card displayed `0 tokens · 0 tools` throughout the run and after timeout even though its child frame contained 48 messages and 35 tool results. | Attempt usage is finalized only after a delegated Agent returns; the Workflow summary ignored durable child-frame messages while an attempt was running or timed out. | Hydrate Workflow snapshots from child-frame message activity once per existing UI refresh: show durable event count, tool-result count, and last activity time; show unknown token usage as `—`, not a false zero. | Fixed, uncompiled |
| D39 | After the registered Workflow had been approved, the parent conversation manually ran `depmap_evidence` and launched a second ad-hoc literature Workflow. | The launch contract covered pre-launch routing and launch failure, but did not clearly reserve approved task execution to the persisted host graph on later turns. | The registered-Workflow catalog and `start_workflow` result now state that approval/execution are host-managed and prohibit direct evidence, delegation, browser, or replacement-Workflow duplication after approval. | Fixed, uncompiled |

## Regression contracts added

- Cancer-only colorectal request must call `lineage_catalog` with canonical
  `Bowel`, must not call `depmap_evidence`, and must not invent APC/KRAS.
- APC damaging-mutation dependency must be reported as a pan-cancer
  `mean_difference`, never rho/Pearson/Spearman, and must not attach a drug.
- The native query schema must remain flat while runtime validation continues to
  reject incomplete mode-specific requests.
- Legacy remote `not_testable` stack headers must be rewritten as eligibility or
  index coverage, not file-read failure.
- ATF5 × liver topic design must create the registered Workflow draft before
  any evidence query, Run-history load, shell scan, Skill search, or report
  write.
- A non-significant ATF5 custom-missense top list must remain a pan-cancer null
  `mean_difference` result and must not seed a mechanism or drug.
- Workflow resource discovery under the DepMap specialist must expose
  `depmap_read` even when the project's ordinary enabled-Skill subset is empty.
- The DepMap novelty task must have an editable task-level tool budget, stop
  exhaustive retrieval before wall-time expiry, and return verified partial
  coverage rather than timing out without a result.
- Running/timed-out Workflow cards must derive live activity from the child
  frame instead of showing false zero tools, and an approved registered
  Workflow must not be duplicated manually by the parent conversation.

## Deferred verification

Compilation and executable packaging are intentionally deferred. When the user
finishes supplying trajectories and authorizes verification, run focused tests
first and build only:

```powershell
cargo tauri build --bundles nsis
```

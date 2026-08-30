# DepMap Agent project control plane

## Product objective

Build a complete project-level DepMap Agent on Wisp Science. The Agent owns
planning, evidence routing, R execution, Run monitoring, QA, stage-specific
Skill use, provenance, and multi-session continuity. A precomputed knowledge
base accelerates standard questions but never replaces the Agent.

## Problems confirmed by pressure testing

1. Specialist identity is stored per conversation, so a new conversation can
   lose the DepMap Agent.
2. Required DepMap Skills can be discovered globally yet excluded from the
   project's enabled subset.
3. The writable project, knowledge base, raw release, and Run output roots have
   been conflated, causing wrong-path and write-boundary failures.
4. Knowledge coverage and computation capability are represented as one idea;
   absence from the knowledge base can trigger an unplanned analysis.
5. Query failure can drift into TCGA, ad-hoc Python, literature claims, or fund
   writing before a DepMap result has passed QA.
6. Long work can report success without a trustworthy native exit status or can
   reuse stale output directories.
7. Large logs, repeated turns, and raw-result detail can pollute the model
   context instead of crossing the execution boundary as compact evidence.

## Durable state

- Project default Specialist: inherited by future conversations; existing
  conversations retain their frozen identity.
- DepMap provider configuration: local or remote knowledge provider, data root,
  analysis root, dataset release, and health status. Secrets stay in keyring.
- Knowledge coverage manifest: which cohort, statistic, threshold family, and
  detail level are already queryable.
- Computation capability manifest: which R methods and direct inputs can produce
  a missing result.
- Persisted Run and Artifact provenance: immutable analysis specification,
  unique output directory, native exit code, result contract, and QC gate.
- `depmap_project_runs`: the recovery index used by a fresh conversation to
  discover project-scoped Runs without replaying old transcripts.
- Research state: validated observations, warnings, hypotheses, decisions, and
  pending follow-ups linked to the project rather than a single transcript.

## State machine

```text
request
  -> normalized
  -> provider_checked
  -> precomputed_query | coverage_gap | provider_unavailable
  -> new_analysis_proposed
  -> new_analysis_authorized
  -> run_submitted
  -> run_succeeded
  -> qc_passed
  -> run_validated
  -> interpreted
  -> optionally indexed
```

Only `run_validated` may introduce new numerical conclusions. A provider
failure and a coverage gap are different states. Neither silently authorizes
raw computation.

## Orchestration boundary

- Main Agent: question classification, state transitions, approval boundary,
  final scientific judgment, and user communication.
- Deterministic query tool: bounded structured lookup, exact numeric results,
  version and provenance.
- `depmap_query`: the fixed read-only provider boundary for local and HTTPS
  knowledge services.
- R Run: data loading, statistics, plots, and machine-readable result/QC files.
- `depmap_validate_run`: the transition guard from successful execution to
  scientific interpretation.
- Child Agent: only independent literature retrieval or result review, with a
  compact evidence package. Never owns the main R calculation or final claim.
- Writing Skills: loaded only after validated evidence exists; cannot modify
  underlying statistics.

## Delivery order

1. Project default Specialist and required-Skill baseline.
2. Separate workspace roots and explicit routing states.
3. Knowledge coverage manifest and local/remote provider abstraction.
4. R-first analysis proposal/authorization handoff.
5. Native Run success and QC gates with immutable output directories.
6. Project research-state persistence and cross-session retrieval.
7. Pressure scenarios without release builds, followed by one full verification
   and Windows release build after behavior stabilizes.

# DepMap Agent benchmark

The exported trajectory is a useful exploratory smoke test, but it is not by
itself evidence that the Agent is reliable or that a particular model-round
limit is optimal. Wisp Science therefore treats quality, cost, and robustness
as separate measurements.

## Benchmark layers

1. **Contract tests** verify tool schemas, Workflow structure, permissions, and
   evidence-state semantics without a model or live server.
2. **Trajectory regression** imports an exported Wisp trajectory and measures
   model rounds, token use, tool calls, tool errors, duplicate calls, invalid
   DepMap requests, remote failures, and known unsupported claims.
3. **Fixed live cases** rerun stable prompts against a fixed data snapshot and
   model configuration. They test discovery, coverage gaps, negative results,
   topic review, and report generation.
4. **Holdout and perturbation cases** change the cancer, gene, wording, missing
   module, and stale-memory context. These are not used while tuning a fix.
5. **Scientific review gates** trace every number to a successful query or
   validated Run and manually inspect directionality, provenance, figures,
   Results, and Methods.
6. **Stress tests** increase concurrent users, result volume, and latency only
   after correctness gates pass.

No single round-count target proves quality. The initial limits in
`crates/wisp-cli/eval-suites/depmap-trajectory-v1.yaml` are regression SLOs for
one three-turn scenario. A change is accepted only when it reduces cost or
errors without failing evidence, coverage, deliverable, or scientific-review
gates. Limits should be revised from repeated runs and held-out cases, not from
one favorable trace.

## Import an exported trajectory

Run the evaluator without `--allow-failures` in a release gate. During baseline
capture, allow failures so the report is still written:

```powershell
cargo run -p wisp-cli -- trajectory-eval `
  --input D:\path\to\wisp-trajectory.html `
  --rubric crates\wisp-cli\eval-suites\depmap-trajectory-v1.yaml `
  --save target\depmap-eval\current.json `
  --allow-failures
```

The command accepts the exported HTML or its raw trajectory JSON. The report is
machine-readable JSON and deliberately keeps manual scientific checks visible;
it does not convert them into unverified automatic scores.

## Run the fixed and held-out fixture suite

The six-case suite includes stale-memory grounding, a one-call dynamic evidence
bundle, a complete lineage query, an ineligible cohort, a blocked provider, and
a breast-cancer/gene holdout. Its
tools expose explicit schemas and deterministic results, so it can run offline
in CI or against a configured live model without contacting the DepMap server:

```powershell
cargo run -p wisp-cli -- eval --mode offline `
  --suite crates\wisp-cli\eval-suites\depmap-agent-v1.yaml `
  --save target\depmap-eval\depmap-agent-offline-v1.json
```

For model comparison, use `--mode live`, select the same model configuration,
and repeat the suite. Keep the suite file and fixture results fixed while
comparing runs; changes to its SHA-256 suite hash create a new benchmark rather
than a directly comparable result.

## Baseline: frame `220f7e37-1daa-4bae-b21d-b28096cba524`

The first imported trajectory contains three user turns:

| Metric | Baseline |
| --- | ---: |
| Model rounds | 127 |
| Input tokens | 14,149,911 |
| Output tokens | 130,954 |
| Tool calls | 144 |
| Tool errors | 29 (20.14%) |
| Invalid DepMap queries | 6 |
| Remote-query failures | 2 |
| Repeated identical calls within a turn | 13 |

Both persisted analysis Runs in that trajectory ultimately passed their own QC,
so the trace is not simply a failed analysis. Its main problem is orchestration
efficiency and claim discipline: it reaches valid artifacts through too many
model/debugging rounds and contains unsupported summary language. The initial
P0 correction is a mode-specific `depmap_query` schema plus a query-only
discovery boundary. Subsequent live reruns must demonstrate that these changes
reduce invalid calls without losing the valid scientific outputs.

## Release decision

Before compiling a test installer:

- run the narrow contract and trajectory-evaluator tests;
- run the existing DepMap Workflow acceptance tests;
- capture a fresh trajectory with the same prompt and fixed data snapshot;
- run the trajectory gate without `--allow-failures`;
- run at least one held-out cancer/gene case; and
- manually audit numerical provenance and final report consistency.

A new model or environment is better only if it passes the same correctness
gates and improves predeclared efficiency metrics across repeated fixed and
held-out cases. A single shorter trajectory is not sufficient.

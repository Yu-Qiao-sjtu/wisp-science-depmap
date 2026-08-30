# DepMap analysis result contract

Every generated analysis should produce `run_manifest.json`, `result.json`, and
`qc.json`. Tables and figures remain separate files and are referenced by
project-relative path.

## `run_manifest.json`

Required fields:

```json
{
  "schema_version": 1,
  "analysis_id": "stable task id",
  "created_at": "ISO-8601 timestamp",
  "dataset_release": "26Q1 or observed release",
  "language": "R",
  "entrypoint": "analysis/depmap-agent/runs/.../scripts/analysis.R",
  "reference_scripts": [],
  "inputs": [
    {"path": "data/...", "bytes": 0, "role": "gene_effect"}
  ],
  "parameters": {},
  "software": {},
  "outputs": []
}
```

Record checksums when already available or affordable. Do not hash every
multi-gigabyte input during every exploratory run; record byte size, release,
and immutable source identity, and use a reusable catalog checksum when one has
been intentionally generated.

## `result.json`

Required fields:

```json
{
  "schema_version": 1,
  "status": "ok",
  "question": "...",
  "targets": [],
  "cohort": {
    "requested": "all",
    "n_before": 0,
    "n_after": 0
  },
  "methods": [],
  "observations": [],
  "tables": [],
  "figures": [],
  "warnings": []
}
```

Each statistical observation should include the comparison, effect estimate,
sample counts, test, raw and adjusted p-values when applicable, and the exact
direction semantics.

## `qc.json`

Required checks:

```json
{
  "schema_version": 1,
  "status": "pass",
  "checks": [
    {"name": "release_identified", "status": "pass", "detail": "26Q1"},
    {"name": "model_ids_unique", "status": "pass", "detail": "..."},
    {"name": "effect_direction_declared", "status": "pass", "detail": "..."}
  ],
  "blocking_failures": [],
  "warnings": []
}
```

The final interpretation is blocked when a required input, target, identifier
mapping, effect direction, or cohort definition fails validation. A warning is
not silently converted into a pass.

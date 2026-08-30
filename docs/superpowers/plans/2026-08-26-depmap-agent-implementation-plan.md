# DepMap Agent implementation and pressure-test plan

## Outcome

Deliver a project-level DepMap Agent on Wisp Science. A user selects it once as
the project's default Agent; every new conversation inherits the identity,
recovers persisted Runs, queries a local or server-hosted knowledge provider,
and starts a new R analysis only through an explicit, validated Run lifecycle.

## Invariants

- The 169.76 GiB knowledge base and raw matrices never enter model context.
- Knowledge coverage and raw-data computation capability are separate
  manifests.
- A coverage gap or provider failure never silently starts computation.
- R remains the scientific calculation and plotting default.
- One substantial analysis has one immutable specification, Run id, run
  directory, output declaration, result contract, and QC gate.
- New numerical interpretation requires `run_validated`.
- Project identity, Runs, Artifacts, and decisions survive conversation changes.
- Secrets remain in the OS keyring.

## Delivery slices

1. Persist the project default Specialist and inherit it into new/branched
   conversations.
2. Guarantee the DepMap knowledge and coding Skills remain available to the
   built-in DepMap Agent while loading their content only at the relevant stage.
3. Separate writable project, read-only knowledge, read-only raw data, and Run
   output roots.
4. Add the precomputed knowledge coverage manifest and local/remote provider
   configuration.
5. Add fixed `depmap_query`, `depmap_project_runs`, and
   `depmap_validate_run` tools.
6. Add keyring-backed remote Bearer authentication.
7. Pressure test contracts, real 26Q1 bounded queries, real narrow-column R
   execution, UI inheritance, native Windows exit codes, and QA failure paths.
8. Run the full repository suite. Produce one Windows installer only after all
   behavioral debugging is complete and the user asks for the final build.

## Pressure matrix

| Scenario | Expected result | Current evidence |
|---|---|---|
| New conversation in a DepMap-default project | DepMap Agent is selected automatically | Rust persistence tests and Playwright pass |
| Branch conversation | Source Agent identity is inherited | session command implementation and specialist tests |
| Project Skill subset excludes DepMap Skills | Required DepMap Skills remain available | targeted Tauri test passes |
| Local 26Q1 provider | QA and all six query families resolve ready | real resolver smoke passes |
| Remote provider configured but not contacted | `needs_probe`, not ready | isolated R resolver test passes |
| Remote HTTPS query with token | Bearer-authenticated bounded JSON | loopback Rust integration test |
| Standard knowledge queries | catalog/core/pair/top/lineage/pathway/drug return provenance | real 26Q1 smoke passes |
| Ineligible lineage event | `coverage_gap`; no new Run | KRAS damaging/Lung fixture passes |
| New conversation after an older Run | recent project Runs can be discovered by id | project-cycle filter tests |
| Native Windows R/process failure | non-zero exit code reaches persisted Run | Windows native-exit test passes |
| Result or QC malformed | validation fails and interpretation is blocked | result-contract failure tests |
| Real R data path | only requested columns load; JSON and readable figure are written | ESR1/FOXA1 26Q1 smoke and visual QA pass |
| Context pressure | query, Run list, logs, and validation JSON remain bounded | 4 MiB query, 50 Run, 8 KiB stderr, 2 MiB QA limits |

## Remaining final gate

Run `cargo test --workspace`, the complete Playwright suite, and the platform
checks required by `AGENTS.md`. Do not create a release, tag, or installer as
part of iterative debugging. The final Windows build is a separate last step.

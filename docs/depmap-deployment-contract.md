# DepMap deployment contract

The desktop client and deployed DepMap MCP must agree on an inclusive query
contract range. The repository uses two separate gates so a not-yet-deployed
provider update never blocks the pull request that makes it deployable.

## Pull requests: offline classification

`scripts/depmap_contract_gate.py pr` reads the client range from
`src-tauri/src/depmap_agent.rs`, the provider version from
`services/depmap_api/app.py`, the changed paths, and the pull-request issue
linkage. It does not open a network connection.
Changed paths are taken from the pull request's merge base, while compatibility
is assessed against GitHub's effective merge revision so updates made only on
the base branch are not attributed to a stale feature branch.

The classifier applies these rules:

- Local, UI, and documentation changes use ordinary CI only.
- A client change that keeps the current range and a backward-compatible
  Provider change use offline comparison plus the existing service suites.
- A new contract follows **expand → deploy → contract**: first widen the
  client from `N` to `N..=N+1`; then merge and deploy the separate Provider
  `N+1` change; rebuild its catalog and verify it; only then remove `N` in a
  later client PR with a fresh live attestation.
- A single issue-fix PR cannot change both the client and Provider contract
  boundaries. Split it so every intermediate `main` remains compatible.

This classifier is intentionally not a live guotosky check. Pull requests are
reproducible when the server is unavailable, and the Provider `N+1` PR can
merge before `N+1` exists in production.

## Deployment and release: live attestation

The only network-capable subcommand is `probe`. Open an authorized local
tunnel to the deployed MCP, then run:

```powershell
python scripts/depmap_contract_gate.py probe `
  --repo-root . `
  --url http://127.0.0.1:18877/mcp `
  --output .github/depmap-live-contract.json
```

On Unix shells, use the same arguments with normal line continuations. The
probe calls only `depmap_status`. It writes a path-free attestation containing
the observation time and structured status; it does not record the tunnel URL,
credentials, host paths, or SSH details.

Creating a GitHub Release does not read `.github/depmap-live-contract.json`.
The probe remains available for an operator who wants a path-free record of
`depmap_status`, but a missing or stale file does not block the tag.

`compare` still checks a captured status payload when you pass one. A usable
payload has `query_contract_version` inside the Rust client's inclusive
minimum and maximum, non-empty `server_build_identity`,
`capability_catalog_digest`, and `catalog_build_identity`, and a catalog
identity other than `catalog-missing` or `catalog-unreadable`.

The offline comparator can also be run against a captured status payload:

```text
python scripts/depmap_contract_gate.py compare --repo-root . --status-file status.json
```

Do not place API tokens, SSH configuration, absolute paths, or raw scientific
data in the status file or attestation.

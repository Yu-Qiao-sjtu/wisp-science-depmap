# DepMap workspace contract

Keep the writable project, read-only knowledge base, and read-only raw release
data as separate roots. Resolve them with
`scripts/resolve_depmap_workspace.R` before querying or computing.

An optional project-local `.wisp/depmap-agent.json` uses this backward-compatible
schema for a local knowledge base:

```json
{
  "schema_version": 1,
  "knowledge_root": "D:/data/depmap/knowledge",
  "data_root": "D:/data/depmap/data",
  "analysis_root": "analysis/depmap-agent"
}
```

For a server-hosted knowledge base, use the version 2 nested form:

```json
{
  "schema_version": 2,
  "knowledge": {
    "provider": "remote",
    "endpoint": "https://depmap.example.org/api/v1",
    "release": "26Q1"
  },
  "data_root": "D:/data/depmap/data",
  "analysis_root": "analysis/depmap-agent"
}
```

When the API is bound only to the server's loopback interface, Wisp Science can
create the SSH tunnel lazily through an already registered SSH ExecutionContext:

```json
{
  "schema_version": 2,
  "knowledge": {
    "provider": "remote",
    "endpoint": "http://127.0.0.1:18876/api/v1",
    "release": "26Q1",
    "tunnel": {
      "enabled": true,
      "context_id": "ssh:lab-server",
      "local_port": 18876,
      "remote_port": 8876,
      "access_authorized": true
    }
  },
  "analysis_root": "analysis/depmap-agent"
}
```

The project file identifies only the registered environment and ports. Host,
user, key path, and probe status come from Wisp Science's ExecutionContext
registry; private keys and passwords are never copied into the project. The
tunnel is not opened when the project or Skill is loaded. The first
`depmap_evidence` or `depmap_query` opens it on demand, after the SSH
environment has a successful Probe. Automatic tunnels require key-based SSH.
Set `access_authorized` to `false` while server-side access is pending; this is
an explicit local kill switch that prevents even a previously successful Probe
from opening the tunnel. Change it to `true` only after the administrator grants
access.
If the administrator has not granted access, the query returns
`remote_tunnel_not_ready` and does not fall back to local raw-data scanning.

The remote endpoint and optional tunnel are configuration, not proof that the service is healthy.
The resolver returns `needs_probe` until the fixed query tool validates the
endpoint. Authentication belongs in the OS keyring, never in this file.
See [remote-api-contract.md](remote-api-contract.md) for the health and bounded
query endpoints.

Relative paths resolve from the active project. Environment variables
`DEPMAP_KNOWLEDGE_ROOT`, `DEPMAP_DATA_ROOT`, and `DEPMAP_ANALYSIS_ROOT` override
the file for portable deployments. Remote deployments may also use
`DEPMAP_KNOWLEDGE_PROVIDER`, `DEPMAP_KNOWLEDGE_ENDPOINT`, and
`DEPMAP_KNOWLEDGE_RELEASE`. Tokens and remote credentials do not belong in this
file.

`references/knowledge-coverage-manifest.json` defines bounded query families.
It is intentionally separate from `depmap-coding-agent`'s computation
capability manifest: the former says what has already been precomputed; the
latter says what raw-data analysis could be run after authorization.

The resolver may recognize the legacy layout where the project itself is the
knowledge directory and raw data are in `../data`. That is a warning-only
compatibility path: recomputation still requires an explicit transition and
analysis output defaults outside the knowledge directory.

## Execution states

- `precomputed_query`: the requested cohort, statistic, threshold family, and
  detail level exist in the knowledge catalog. Query only.
- `coverage_gap`: the knowledge base is healthy, but this exact result is not
  precomputed. Report the gap; do not imply the knowledge base is missing.
- `new_analysis_proposed`: define raw inputs, method, cost, output root, and
  validation plan. Do not read raw matrices yet.
- `new_analysis_authorized`: the user authorized that material computation, or
  explicitly asked to run it after seeing the changed scope.
- `run_validated`: the unique Run succeeded, outputs are from that Run, and QC
  passed. Only this state may produce new numerical conclusions.

Never silently move from `precomputed_query` or `coverage_gap` to raw-data
analysis. A follow-up that merely asks for more detail is not authorization to
scan raw matrices when the detail is absent from the knowledge base.

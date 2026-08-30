# DepMap knowledge provider API v1

The desktop Agent may use a server-hosted precomputed knowledge base without
shipping the 169.76 GiB corpus. The project manifest stores only the HTTPS
endpoint and release; credentials remain in the OS keyring.

When configured in **Settings → Credentials → DepMap knowledge server**, the
desktop sends the keyring-backed token as `Authorization: Bearer <token>`. The
`DEPMAP_KNOWLEDGE_API_TOKEN` environment variable is a deployment override.

## Health

`GET {endpoint}/health`

Return JSON and a 2xx status only when the configured release and query index
are ready. Recommended fields:

```json
{
  "schema_version": 1,
  "status": "ready",
  "release": "26Q1",
  "query_contract_version": 4,
  "coverage_manifest_version": 2
}
```

## Query

`POST {endpoint}/query` with the same bounded object accepted by the local
query helper, for example:

```json
{
  "mode": "pair",
  "module": "effect_correlation",
  "source": "KRAS",
  "target": "RAF1"
}
```

Supported modes are `catalog`, `lineage_catalog`, `core`, `pair`, `top`,
`lineage`, `pathway`, `drug`, `lineage_network`, `lineage_cnv`,
`lineage_drug`, `enrichment`, and `tcga_expression_survival`.
`lineage_catalog` inventories cancer-level
module manifests without requiring or inventing a gene.
`tcga_expression_survival` requires `gene` and optionally accepts `project`,
canonical DepMap `lineage`, `endpoint` (`OS`, `DSS`, `DFI`, or `PFI`), and
`limit`. It reads only the installed precomputed bridge, never raw TCGA files.
The server returns JSON with source provenance and must never return a full
matrix. Every bounded limit is at most 100. The desktop rejects responses
larger than 4 MiB and does not follow redirects.

Use a non-2xx status for configuration, authentication, or service failures.
Use a successful JSON response with `status: "not_testable"` for a legitimate
coverage gap such as an event that fails cohort-size eligibility. A coverage
gap is not permission to start raw-data computation.

Sparse lineage modes use `FOUND`, `NOT_RETAINED`, `INELIGIBLE`,
`NOT_COMPUTED`, and `MODULE_UNAVAILABLE`. All five are successful protocol
responses; only `FOUND` authorizes a numerical claim about retained rows.

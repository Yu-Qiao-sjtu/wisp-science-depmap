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
  "query_contract_version": 9,
  "coverage_manifest_version": 4,
  "knowledge_annotation_schema_version": 1
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

Supported modes are `catalog`, `lineage_catalog`, `lineage_dependency`, `lineage_directions`, `core`, `pair`, `top`,
`lineage`, `pathway`, `drug`, `lineage_network`, `lineage_cnv`,
`lineage_drug`, `enrichment`, `subtype`, `coamplification`, `true_love`,
`synthetic_lethal`, `three_d`, and
`tcga_expression_survival`.
`lineage_catalog` inventories cancer-level
module manifests without requiring or inventing a gene.
`lineage_dependency` requires a canonical `lineage`, optionally accepts
`ranking` (`selective` or `mean_dependency`) and `limit`, and reads only the
completed precomputed lineage-vs-rest dependency table. It never starts a new
test or Run. `effect_mean_difference` is a Gene Effect mean difference, not
logFC.
`lineage_directions` requires a canonical `lineage` and optionally accepts
`limit`. It returns statistically distinct, family-specific shortlists and
topic candidates for a cancer-only direction request. It is not interchangeable
with `lineage_dependency`, and its cross-family occurrence count is not a
combined significance score.
`tcga_expression_survival` requires `gene` and optionally accepts `project`,
canonical DepMap `lineage`, `endpoint` (`OS`, `DSS`, `DFI`, or `PFI`), and
`limit`. It reads only the installed precomputed bridge, never raw TCGA files.
The server returns JSON with source provenance and must never return a full
matrix. Every bounded limit is at most 100. The desktop rejects responses
larger than 4 MiB and does not follow redirects.

Every pageable list response includes a code-generated `page_info` object.
`limit` is the requested page size, not evidence that the retained result set
contains only that many rows. Continue only with the opaque `next_cursor` and
the same scientific filters; clients must never construct or edit cursors.

```json
{
  "page_info": {
    "returned_rows": 20,
    "total_retained_rows": 1842,
    "total_is_exact": true,
    "has_more": true,
    "next_cursor": "opaque-server-cursor",
    "collection": "rows",
    "analysis_scope": {
      "release": "26Q1",
      "mode": "lineage_dependency",
      "lineage": "Liver"
    }
  }
}
```

When a module cannot cheaply establish the exact retained total,
`total_retained_rows` is `null` and `total_is_exact` is `false`; `has_more`
still comes from a one-row lookahead. This must not be paraphrased as a zero
or complete result set.

`subtype` optionally accepts `gene`, canonical `lineage`, exact `contrast`, and
`limit`. With no gene/contrast it inventories eligible contrasts; an exact
contrast without a gene returns retained selective hits. `coamplification`
requires `source` and optionally accepts `partner`, dependency `target`,
`layer` (`exhaustive_high_confidence` or `lineage_adjusted`), and `limit`.
`true_love` optionally accepts `gene`, `partner`, and `limit` and prefers the
completed bootstrap-stability layer. `synthetic_lethal` requires `source`,
`target`, or both, and optionally accepts one frozen event family. `three_d`
requires a supported analysis `family` and uses only catalog-validated cohort,
contrast, omic, gene, source, and target selectors.

Use a non-2xx status for configuration, authentication, or service failures.
Use a successful JSON response with `status: "not_testable"` for a legitimate
coverage gap such as an event that fails cohort-size eligibility. A coverage
gap is not permission to start raw-data computation.

Sparse lineage modes use `FOUND`, `NOT_RETAINED`, `INELIGIBLE`,
`NOT_COMPUTED`, and `MODULE_UNAVAILABLE`. All five are successful protocol
responses; only `FOUND` authorizes a numerical claim about retained rows.

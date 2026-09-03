# DepMap knowledge API

This service implements the fixed Wisp Science remote knowledge contract. It
is read-only, accepts only bounded query modes, and delegates data access to
`skills/depmap-knowledge-query/scripts/query_depmap_kb.R`.

Required environment variables:

- `DEPMAP_KNOWLEDGE_ROOT`
- `DEPMAP_QUERY_SCRIPT`
- `DEPMAP_API_TOKEN` (at least 32 characters)

Optional controls include `DEPMAP_RELEASE`, `DEPMAP_QUERY_TIMEOUT_SECONDS`,
`DEPMAP_MAX_CONCURRENCY`, and `RSCRIPT`.

The production endpoint is `/api/v1`. Both `health` and `query` require the
same Bearer token. Run the dependency-free tests from the repository root:

Contract v6 includes bounded `lineage_catalog`, `lineage_network`,
`lineage_cnv`, `lineage_drug`, `enrichment`, `subtype`, `coamplification`,
`true_love`, `synthetic_lethal`, and `three_d` modes over completed sparse
outputs.
`lineage_catalog` supports disease-first inventory without an anchor gene. The
association modes return explicit
`FOUND`, `NOT_RETAINED`, `INELIGIBLE`, `NOT_COMPUTED`, or
`MODULE_UNAVAILABLE` states plus manifests and provenance; the API never turns
an absent sparse row into a biological-negative claim.

Contract v4 adds `tcga_expression_survival`. It bridges the fixed 18,531-gene
DepMap CRISPR target universe to precomputed TCGA primary-tumour expression and
the censored OS, DSS, DFI, and PFI endpoints. The mode accepts `gene` plus an
optional TCGA `project`, canonical DepMap `lineage`, `endpoint`, and bounded
`limit`. Genes are aligned by Ensembl gene ID with an explicit gene-symbol
fallback. It returns project rows with the mapping basis, median `log2(TPM+1)`, cohort/event counts,
a signed Breslow Cox score z statistic, nominal p value, and BH FDR. Positive z
means higher expression is associated with higher event hazard; it is not a
hazard ratio or a causal estimate. Raw patient matrices remain outside the API.

Build the read-only bridge next to the existing knowledge modules:

```bash
Rscript scripts/build_depmap_tcga_bridge.R \
  --depmap-root /path/to/depmap-26q1 \
  --tcga-root /refdir/database \
  --output /path/to/depmap-26q1/depmap-26q1-tcga \
  --projects all
```

```bash
python -m unittest services.depmap_api.tests.test_app
```

The checked-in user systemd unit is intentionally bound to `127.0.0.1:8876`.
Expose it only through an authenticated SSH tunnel or an administrator-managed
HTTPS reverse proxy.

For the current private deployment, `scripts/depmap_api_tunnel.ps1` maintains
the authenticated local forwarding endpoint at
`http://127.0.0.1:18876/api/v1`. It uses the `guotosky` SSH config alias and a
named mutex so only one tunnel supervisor runs per Windows login.

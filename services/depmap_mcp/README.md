# Local DepMap + TCGA MCP

This service turns the existing local DepMap 26Q1 precomputed knowledge base
and the optional precomputed TCGA expression/survival bridge into one bounded,
read-only MCP server. It does not copy data, open SSH connections, or start any
analysis job.

## Local paths

- Knowledge: `D:\New-PHD\depmap_0823\knowledge`
- Optional TCGA bridge: `D:\New-PHD\depmap_0823\knowledge\depmap-26q1-tcga`
- Runtime: `D:\New-PHD\depmap_0823\runtime\depmap-mcp-venv`
- HTTP endpoint: `http://127.0.0.1:8877/mcp`

The launch script sets persistent `TEMP`/`TMP` directories below the DepMap
runtime root. The service reads Parquet/RDS outputs in place.

## Start

```powershell
powershell -ExecutionPolicy Bypass -File scripts/start_depmap_mcp_local.ps1
```

For direct process ownership by Wisp, use stdio:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/start_depmap_mcp_local.ps1 -Transport stdio
```

The exposed tools are intentionally small:

- `depmap_status`
- `depmap_resolve_lineage`
- `depmap_lineage_catalog`
- `depmap_lineage_dependencies`
- `depmap_lineage_direction_discovery`
- `depmap_gene_evidence`
- `tcga_gene_expression_survival`
- `depmap_pair_evidence`
- `depmap_drug_evidence`
- `depmap_subtype_evidence`
- `depmap_coamplification_evidence`

Every response is an evidence envelope with a deterministic `evidence_id`,
release, request, metric semantics, coverage states, and normalized provenance.
The combined gene tool returns TCGA and DepMap as separate evidence items. It
never performs a sample-level join or creates a synthetic combined score.

If the optional TCGA bridge is absent, TCGA queries return
`MODULE_UNAVAILABLE`; this is a coverage state, not a biological result. Install
only the validated, precomputed bridge with this layout:

- `depmap-26q1-tcga/qa.json`
- `depmap-26q1-tcga/project_catalog.csv`
- `depmap-26q1-tcga/projects/TCGA-*/manifest.json`
- `depmap-26q1-tcga/projects/TCGA-*/gene_associations.parquet`

Server access remains a Wisp Science execution-context concern. This MCP does
not manage SSH or tunnels; after results are transferred to the local knowledge
root, restart the MCP and its status tool will report TCGA as installed.

Cancer-name normalization covers every one of the 34 canonical DepMap lineage
labels. The MCP accepts the maintained Chinese main names and common synonyms
(for example, `结肠癌`/`直肠癌`/`结直肠癌` resolve to `Bowel`, while
`卵巢癌`/`输卵管癌` resolve to `Ovary Fallopian Tube`). The canonical label is
always preserved in the evidence query. These mappings describe DepMap model
groups; they do not turn a broad group into an exact clinical histology.

`depmap_resolve_lineage` is the natural-language boundary. An exact maintained
alias returns `RESOLVED` with one selected lineage. A broad term such as
`白血病` returns `AMBIGUOUS` with valid candidates and no selection. For an
unknown phrase, the language model may submit candidate lineage labels; the
tool validates that they belong to the 34-label vocabulary but returns
`PROPOSED`, so user confirmation is still required before evidence retrieval.

`depmap_lineage_direction_discovery` handles cancer-only topic requests without
inventing an anchor gene. It returns fixed-filter shortlists for eight distinct
families, an unweighted cross-family mention count, and a balanced bounded topic
candidate list. It never numerically combines correlation, mean difference,
enrichment, or PRISM metrics.

`depmap_lineage_dependencies` handles cancer-only requests for top, strongest,
or selective dependency genes. It reads the completed lineage-vs-rest CRISPR
Gene Effect table in one bounded call. The default `selective` ranking uses the
precomputed one-sided Welch test, within-lineage BH FDR, and producer rank;
`mean_dependency` is a separate descriptive ordering. The returned Gene Effect
mean difference is not logFC.
The table has no validated housekeeping/common-essential exclusion field;
`selective` therefore must not be reported as `non-housekeeping`.

`depmap_subtype_evidence` lists the 33 eligible frozen subtype contrasts,
returns complete per-gene rows within those contrasts, or returns bounded
retained-hit rankings for an exact `contrast_id`. It does not infer arbitrary
free-text subtype definitions. `depmap_coamplification_evidence` reads only the
15,368 constrained high-confidence directional pairs and their exhaustive or
lineage-adjusted retained target hits. `NOT_RETAINED` means the pair was tested
but the requested target did not pass the stored result contract; it is not a
biological null.

## Connect from Wisp Science

Add an MCP connection with:

- Name: `Local DepMap 26Q1`
- Transport: `HTTP`
- URL: `http://127.0.0.1:8877/mcp`
- Authentication: `None`

Keep this endpoint loopback-only. It intentionally has no bearer secret because
it is not exposed to the LAN or public internet. A future remote deployment must
add TLS and authentication rather than reusing this local configuration.

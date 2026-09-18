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

- `depmap_capabilities`: lightweight intent, required-entity, ambiguity, and
  tool-routing catalog. It reads no matrix rows and is suitable for one-time
  discovery at the start of an Agent session;

- `depmap_analysis_catalog`: completed analysis units from the unified SQLite
  directory index, optionally restricted to one module. The filter accepts an
  exact module or analysis-unit name and registered capability aliases such as
  `true_love`; an unmatched filter returns a bounded `NOT_RETAINED` result with
  zero rows instead of raising an adapter exception;
- `depmap_artifact_catalog`: query indexed scripts, data, manifests, results,
  and matrix shards by module, kind, or relative-path fragment;
- `depmap_data_coverage`: inspect release, cohort scope, lineage, modality,
  model and gene counts, event definition, storage completeness, and QA state
  without returning internal paths or raw matrices;
- `depmap_read_resource`: resolve an indexed `depmap://26Q1/...` URI and return
  a bounded table/text preview or binary artifact metadata. Compressed tables
  report total and returned row counts and accept a bounded cursor; malformed
  tables return a typed error instead of terminating the MCP call;

Resource content is sanitized recursively before it crosses the MCP boundary.
Paths inside the configured knowledge root become stable `depmap://` URIs;
other Linux, Windows, and UNC absolute paths are redacted from structured
content, tabular fields, and bounded text previews.

The query index also records one current coverage state for every registered
Reader. Enrichment is advertised only when the active release has both a
compatible Reader and a complete indexed artifact. A missing Reader or
artifact is returned as `MODULE_UNAVAILABLE`/`NOT_COMPUTED`; it is a service
coverage condition and must never be interpreted as biological absence.
- `depmap_status`
- `depmap_resolve_lineage`
- `depmap_lineage_catalog`
- `depmap_lineage_dependencies`
- `depmap_pan_cancer_dependencies`
- `depmap_lineage_direction_discovery`
- `depmap_gene_evidence`
- `tcga_gene_expression_survival`
- `depmap_pair_evidence`
- `depmap_drug_evidence`
- `depmap_subtype_evidence`
- `depmap_coamplification_evidence`
- `depmap_true_love_evidence`
- `depmap_biomarker_model_evidence`
- `depmap_synthetic_lethal_evidence`
- `depmap_3d_evidence`

The expression-biomarker route connects natural-language requests such as
“为 GPX4 建立表达 biomarker 模型” to
`depmap_biomarker_model_evidence`. The tool first queries the indexed target
catalog and reports whether the target is eligible and whether a validated
model is already cached. Training remains a separate explicit workflow, so an
ambiguous request cannot accidentally launch a large computation.

`depmap_lineage_dependencies` accepts `exclude_common_essential=true` for
plain-language requests such as “把所有细胞都需要的基因去掉”. It annotates each
row from the versioned DepMap 26Q1 common-essential asset, reports counts before
and after filtering, and returns `ANNOTATION_UNAVAILABLE` without silently
dropping rows when that asset is missing. Housekeeping-gene annotations remain
a separate concept and are never inferred from gene names or model memory.

`depmap_pan_cancer_dependencies` performs the same bounded read across every
completed lineage table in one call. `limit` is a display limit per lineage;
the response reports tested/retained counts and computes gene recurrence from
the complete retained sets before display truncation, so Top-N rows are never
misrepresented as complete cross-cancer coverage. For descriptive
`mean_dependency` displays, recurrence additionally requires lineage mean Gene
Effect ≤ -0.5; merely tested genes are reported as assay coverage, not recurrent
dependencies.

Every response is an evidence envelope with a deterministic `evidence_id`,
release, request, metric semantics, coverage states, and normalized provenance.
The combined gene tool returns TCGA and DepMap as separate evidence items. It
never performs a sample-level join or creates a synthetic combined score.

Model-facing MCP evidence is projected to a 96 KiB budget with bounded arrays
and strings. `model_projection` reports the original/projected byte counts and
any omitted items. The deterministic `evidence_id` is computed from the full
portable evidence before projection, while retained results remain addressable
through their catalog and `depmap://` provenance. The DepMap Agent consumes the
structured projection directly and does not re-read `.wisp/tool-output` to
reconstruct the same table.

`depmap_capabilities` reads its 20 intent contracts from SQLite
`capability_catalog`. `services/depmap_mcp/capability_catalog.py` is the single
build-time definition used to populate that table; code fallback is used only
when the index is absent. Result adapters and gene-to-shard locations are
registered in `reader_registry` and `matrix_block_index`.

All bounded scientific calls enter through `CatalogReaderRegistry` before the
format-specific query adapter runs. The registry maps every API query mode to a
reader family, permits only `COMPLETE` analysis units, returns stable
`depmap://` artifact URIs, and adds `catalog_resolution` to every evidence item.
When a production index is installed, a missing reader or matching completed
analysis is a terminal coverage error; the MCP does not silently bypass the
catalog with a fixed path. Matrix queries additionally resolve requested genes
through `matrix_block_index`, restricted to the resolved analyses and 32 shards.
The registry passes the resolved analysis, artifact, shard, and Reader binding
into the adapter and validates every returned provenance path against a
`COMPLETE` artifact entry before evidence leaves the server.

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

`depmap_true_love_evidence` reads the completed mutual-rank-1 negative
codependency screen and prefers the bootstrap-stable high-confidence table.
`depmap_synthetic_lethal_evidence` reads the completed observational
mutation/CNV event-to-dependency candidate tables; its name is a screen label,
not proof of causal synthetic lethality. `depmap_3d_evidence` exposes six
catalog-validated families: dependency profiles, 3D-vs-2D contrasts,
codependency, 3D True Love pairs, omics-dependency associations, and
lineage/pathway enrichment. All return bounded rows and their terminal
manifests rather than opening full matrices.

Network shortlists inherit each module's recorded `manifest.min_n`; no separate
30-sample minimum is imposed by discovery. The evidence includes per-section
`selection_filters` and reasons when a module is unavailable or ineligible.
Small cohorts can return supported precomputed rows, with their actual pair
counts and FDR visible for interpretation.

## Connect from Wisp Science

Add an MCP connection with:

- Name: `Local DepMap 26Q1`
- Transport: `HTTP`
- URL: `http://127.0.0.1:8877/mcp`
- Authentication: `None`

Keep this endpoint loopback-only. It intentionally has no bearer secret because
it is not exposed to the LAN or public internet. A future remote deployment must
add TLS and authentication rather than reusing this local configuration.

For the private server deployment, the MCP process binds only to server
`127.0.0.1:8877`. `scripts/depmap_mcp_tunnel.ps1` forwards it to local
`127.0.0.1:18877`; configure Wisp Science with the remote-URL transport at
`http://127.0.0.1:18877/mcp` and no additional authentication. SSH supplies the
transport authentication and the MCP endpoint is never exposed to the LAN.

DepMap routing also installs a turn-scoped execution allowlist. After
`depmap_agent_route` selects a bounded evidence path, unrelated filesystem,
shell, and configuration tools are rejected by the agent executor. A blocked
local provider may fall back only to the configured read-only `depmap_*` or
`tcga_*` MCP tools; otherwise the model must report the blocker or ask for the
missing input.

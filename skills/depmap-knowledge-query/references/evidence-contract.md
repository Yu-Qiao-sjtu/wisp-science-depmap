# Dynamic evidence contract

Use `depmap_evidence` for a resolved gene plus a DepMap cancer lineage. It is a
read-only orchestration layer over existing precomputed query modes; it does not
load a full matrix, create a static evidence card, or start new computation.

## Input

- `gene`: resolved gene symbol.
- `lineage`: DepMap lineage/cancer label.
- `sections`: optional subset of `core`, `networks`, `mutation`, `cnv`, `drug`,
  `enrichment`, and `tcga`. Omission requests all sections.
- `limit`: optional retained rows per subquery, from 1 to 10; default 5.

Common natural-language synonyms are normalized before querying. For example,
`Breast Cancer` resolves to the canonical `Breast` lineage. Interpret coverage
against the lineage recorded in the returned `subject` and subquery, not the
unnormalized phrase supplied by a model or user.

The canonical lineage is a DepMap model grouping. It is not, by itself, proof
of one clinical histology or patient cohort. Keep the user's cancer wording and
the DepMap proxy distinct; do not add control lineages or named cell lines
without current model metadata or a validated Run.

## Output interpretation

The top-level state is `evidence_ready`, `evidence_partial`, or `coverage_gap`.
Each section contains the exact bounded queries and provider results used to
build the view. Preserve the top-level release and each result's manifest,
sample/eligibility fields, retention rule, and provenance.

When core evidence is requested, `focus.core` repeats only the current gene
summary and the requested canonical-lineage row near the top of the response.
Use it for current lineage counts and descriptive values. It does not contain a
lineage-vs-rest test, rank, subtype, distribution-shape result, or causal claim;
never import those fields from Run history or memory.

`coverage_gaps` are part of the result, not missing text to be completed from
memory. Sparse statuses retain their usual meaning: `NOT_RETAINED` is absence
from retained top-K output, `INELIGIBLE` is a cohort threshold failure, and
`NOT_COMPUTED` or `MODULE_UNAVAILABLE` are coverage states.

`not_testable` means the requested event/source was absent or failed its
eligibility/index requirements. It is not an upstream file-read failure unless
the tool call itself is blocked. Do not infer a distribution shape, molecular
subtype, receptor status, or causal subgroup from aggregate mean, median, or
dispersion fields alone.

Every query entry carries explicit statistic semantics. In particular,
mutation and amplification modules return `mean_difference`, whereas network
modules return correlations. Preserve that distinction in prose, tables, and
topic proposals.

`damaging_mutation_n` is the provider-defined damaging-event count, not a
clinical pathogenic-variant count. If every returned association fails the
stated FDR threshold, report the null result and do not use nominal top targets
to construct a mechanism, pathway, named drug, or synthetic-lethal hypothesis.

File presence is an asset-inventory observation only. It does not prove cohort
eligibility, identifier overlap, statistical power, or recomputation
feasibility. Literature mechanisms and novelty claims require a separate,
completed literature-evidence task with traceable paper identifiers.

Core evidence is release-wide. Network, CNV, drug, and enrichment queries are
lineage-scoped. Mutation source-to-all discovery is explicitly marked
`pan_cancer`, because the current lineage mutation contract requires a specified
target pair. Never present those mutation rows as lineage-specific evidence.

The TCGA section maps the shared gene and canonical cancer label to one or more
TCGA patient projects. It is not a sample-level join to DepMap cell lines.
Rows preserve their identifier mapping basis, primary cancer sample count,
survival endpoint/event count, Cox score z, p value, and within-project/endpoint
BH FDR. A row-level `INELIGIBLE` state means the endpoint or gene did not meet
the stored testability rule; it is not a biological null.

Use `depmap_query` after the bundle only for a surgical lookup that needs a
specific target, drug, pathway, term, reciprocal constraint, or catalog field.
Do not repeat all bundle subqueries manually.

If the result is spilled because it exceeds the inline display budget, use the
exact file path returned by the tool and read only bounded ranges from that one
file. The parent `.wisp/tool-output` directory is not a valid evidence scope.

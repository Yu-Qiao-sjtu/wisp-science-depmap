# DepMap case artifacts → control-plane issues

These local trajectories are **acceptance evidence**, not implementation
scope. Production code must key off typed fields (`lineage`, `gene`, `event`,
`status`, `entity_class`) on `ScientificQuery` / `EvidenceEnvelope` / provider
schema. Tests may replay the named gene or lineage as a fixture.

Source directory: `d:\PHD_project\depmap-case` (HTML session dumps named
without extensions, plus `.messages.txt` / `.audit.json` sidecars).

**GitHub titles/bodies were rewritten** to control-plane obligations (2026-09-18). This file remains the case→issue map.

Closed by PR #66 (catalog/Reader integrity): #33, #53, #55, #63.
In flight: PR #67 (`feat/lineage-mutation-query`) for #50 and #51.
Open at remap time: #28, #34, #35, #36, #41, #42, #43, #50, #51, #52, #54, #56,
#57, #58, #59.

## Control-plane vocabulary

| Plane | Obligation |
|---|---|
| Query contract | Typed `ScientificQuery` predicates; filter/match **before** `limit`; exact keys vs ranked pages. |
| Reader / catalog | Installed modules resolve from the COMPLETE catalog; Readers do not crash or return 1-row previews. |
| Evidence status | `FOUND`, `NOT_RETAINED`, `INELIGIBLE`, `NOT_OBSERVED`, `NOT_TESTED`, `NOT_COMPUTED`, `MODULE_UNAVAILABLE`, `COVERAGE_GAP` are coverage states, not negative biology. |
| Provider schema | MCP JSON schema matches runtime validators; lineage vs pan-cancer scopes are distinct arguments. |
| Presentation | Query-only answers stay in chat; QC annotations and progressive disclosure; no unsolicited `results/reports/`. |
| Remote-compute | New matrices/FDR/roster rebuilds are Runs behind a non-exfiltrating gateway; tests use fakes, never real SSH. |

Do **not** invent gene-specific tools (`depmap_ptk7_*`, `depmap_gpx4_breast_*`).
Do **not** require a live SSH host in automated tests.

## Session identity

| Artifact | Trajectory UUID | Notes |
|---|---|---|
| `测试3` | `6bb997e4-…8b794a` | Distinct session. |
| `测试4` | `335e7dae-…72cd7c` | **Same session as** `全肿瘤依赖TF`; later HTML snapshot that also contains the FBXO7 turn. |
| `测试 5` | `6e06bbfe-…11f7185` | Distinct session. |
| `测试6` | `5acb238d-…91e065` | Distinct session. |
| `测试7` | `496b04cc-…ee190c` | Distinct session. |
| `测试8` | `64be9e1d-…e42fd217` | Distinct session. |
| `全肿瘤依赖TF` + `.messages.txt` | `335e7dae-…72cd7c` | Same as `测试4`. `.transcript.txt` is empty. |
| `真爱基因` + `.audit.json` | `42d690f5-…3b2549` | Distinct session. |
| `依赖基因测试` + `.audit.json` | `d7d7ab95-…ead38b` | Distinct session. |

---

## Case-by-case remap

### 1. `依赖基因测试` (+ `.audit.json`)

**User asked**

1. Liver (then Breast, Bowel, Ovary): most-dependent genes, **remove genes every cell needs**.
2. All tumors: same predicate, **report every lineage**.

First turn used local `depmap_query` and hit `configuration_blocked` (missing
`D:\PHD_project\depmap-case\knowledge`). Later turns used remote MCP.

**Agent actually did**

- Routed “remove housekeeping” as `ranking=selective`, then treated empty
  Liver `selective` as “no leftover genes” while still quoting
  `mean_dependency` top-N (ribosome/proteasome).
- Envelope field `housekeeping_filter_applied: false`; agent then N+1
  `depmap_gene_evidence` (147 calls in the HTML dump) to stitch core
  `essentiality` labels and hand-written rest-mean rules.
- Pan-cancer turn swept `depmap_lineage_dependencies` per lineage
  (`limit` truncated) and presented top-N as the universe.
- Local knowledge-root probe instead of a typed ExecutionContext miss.

**Failure modes**

| Mode | Plane |
|---|---|
| Selective ranking used as a common-essential filter | Query contract |
| No sidecar join; N+1 gene_evidence | Query contract + presentation cost |
| Top-N page treated as full table | Query contract (#52) |
| Local path `configuration_blocked` | Remote-compute / ExecutionContext |
| Unsolicited reports in sibling cases | Presentation (#36) |

**Maps to:** **#28** (primary), **#52** (all-lineage exact gene / truncation),
**#57** (all-lineage dump), **#58** (knowledge-root as context, not a local
folder guess). Residual: `depmap_analysis_catalog` `NoneType` (Reader; see
follow-ups).

**Regression fixture:** Liver/Breast/Bowel/Ovary + 24-lineage sweep. Keys:
`lineage`, `ranking`, `exclude_common_essential`, `matched_row_count`.

---

### 2. `真爱基因` (+ `.audit.json`)

**User asked**

1. Liver True Love Genes (TLG).
2. All tumors’ TLGs; report every lineage.

**Agent actually did**

- Called pan-cancer `depmap_true_love_evidence` (`stable_negative_rank1`)
  with invalid `coverage` and oversize `limit` (Pydantic errors).
- Merged unrelated providers: pan-cancer TLG, `lineage_direction_discovery`
  (limit 150 rejected), lineage effect-correlation reciprocal pairs,
  exact pair probes, Python spill.
- `depmap_analysis_catalog` `NoneType`; `depmap_read_resource` “absent from
  indexed catalog”.
- Wrote `results/reports/depmap_26q1_pancancer_true_love_report.md`.

**Failure modes**

| Mode | Plane |
|---|---|
| Pan-cancer catalog answering a lineage query | Provider schema (#34) |
| Schema advertises args the runtime rejects | Provider schema (#35) |
| Merged incommensurable pair definitions | Query contract / evidence status |
| Query-only → local Markdown | Presentation (#36) |

**Maps to:** **#34** + **#35** (primary), **#36**, **#52**-class truncation if
lineage networks are paged. Not a “Liver TLG tool”.

**Regression fixture:** `lineage=Liver` TLG query; pan-cancer inventory with
`scope=pancancer` vs `scope=lineage` explicit.

---

### 3. `全肿瘤依赖TF` / `测试4` / `.messages.txt`

Same UUID. `测试4` is the later snapshot and adds an FBXO7 turn.

**User asked**

1. Across all tumors, which TFs are most depended on, and which analyses exist.
2. Recompute the ranking on the full DoRothEA A–C 271-TF roster via MCP.
3. Fetch the official roster from DoRothEA/decoupleR (browser).
4. MCP-only 24-lineage selective ranking.
5. Why did MCP disappear; permission to read logs.
6. (`测试4` only) FBXO7 impact on “current tumor”.

**Agent actually did**

- `depmap_tf_dependency_evidence` HTTP 500/502 for valid TFs (MYC, AHR, …).
- CSV/`max_rows` previews returned **1 row**; reconstructed 271 TFs via
  browser + local parse.
- Substituted `depmap_biomarker_model_evidence` / gene CRISPR summaries and
  `depmap_lineage_dependencies limit=100`, then intersected with a TF roster.
- Mid-task DepMap MCP dropped out of the tool catalog.
- Wrote pan-cancer TF reports, lineage-selective reports, MCP outage notes,
  then FBXO7 impact Markdown. FBXO7 “only Myeloid” inferred from truncated
  selective top-100 pages.

**Failure modes**

| Mode | Plane |
|---|---|
| Installed TF activity module returns QUERY_ERROR | Provider / Reader (#41) |
| TF-gene CRISPR vs TF-activity vs roster∩selective conflated | Query contract (#42) |
| No frozen universe / bulk ranking; 1-row resource pages | Provider schema (#43) + Reader follow-up |
| Top-N absence as biology (FBXO7) | Query contract (#52) |
| MCP session vanished | Remote-compute (#58) / session catalog |
| Query-only writes | Presentation (#36) |

**Maps to:** **#41, #42, #43, #52, #36, #58**. Duplicate artifacts: treat
`测试4` and `全肿瘤依赖TF` as **one** fixture set.

---

### 4. `测试3`

**User asked:** In DepMap liver, for patients/models with **PTK7 mutation**,
which knockout has the largest effect.

**Agent actually did**

- Routed to pan-cancer `depmap_synthetic_lethal_evidence` →
  `MODULE_UNAVAILABLE` (observational SL not installed).
- Did not bind the completed **lineage mutation-anchor → Gene Effect** reader.
- Inferred `mut_n < 3` because PTK7 was absent from a retained 14-row
  shortlist (not an exact eligibility query).
- Fell back to Liver mean-dependency ranking (unrelated to the mutation
  predicate) and wrote that PTK7 itself is not a Liver dependency.

**Failure modes:** wrong provider for a lineage×event query; absence from a
retained list treated as `INELIGIBLE` without counts.

**Maps to:** **#50** + **#51** (PR #67). Fixture only: `gene=PTK7`,
`lineage=Liver`, `event=damaging_mutation`. Also **#56** (small-n QC) and
**#36** if reports were written (this dump had few writes).

---

### 5. `测试 5`

**User asked**

1. ATF5 impact on “current tumor” via DepMap MCP.
2. Same for RUNX2.
3. Liver-specific genes, excluding genes every cell needs.

**Agent actually did**

- Per-gene impact reports (`ATF5`, `RUNX2`, leftover FBXO7 report from the
  TF session). `depmap_tf_dependency_evidence` HTTP 500 (ATF5 and ATF4
  control) treated as tool failure, then substituted other modules.
- `depmap_gene_evidence` `limit=30` rejected (max 20).
- Liver-specific turn reused the #28 mis-routing (selective ≡ non-housekeeping)
  and wrote `liver_specific_gene_exclusion_recipe.md`.
- Exact-gene selectivity across lineages via truncated pages (same as #52).

**Maps to:** **#52** (gene×lineage exact), **#41** (TF provider 500), **#28**
(third prompt), **#36**, **#56**. Not “ATF5/RUNX2 tools”.

---

### 6. `测试6`

**User asked**

1. Which knockouts inhibit colorectal (Bowel) cell lines.
2. Exclude pan-essentials; **if the field is missing, compute it**.

**Agent actually did**

- `depmap_lineage_dependencies` with `limit` 500/400/300/200 rejected, then
  100 succeeded — top-N as the answer.
- `housekeeping_filter_applied: false`; attempted to **compute** an exclusion
  field (query/compute boundary) and wrote
  `bowel_dependency_common_essential_note.md`.
- N+1 `depmap_gene_evidence` on returned genes.

**Maps to:** **#28** (filter-before-limit + declared sidecar field), **#52**
(truncation), **#36**, **#58** (user said “compute” when the column is absent:
that is `NOT_COMPUTED` or a gated Run, not silent Python). Fixture: `lineage=Bowel`.

---

### 7. `测试7`

**User asked**

1. Which genes to target to inhibit Breast cell lines.
2. Exclude pan-essentials; if impossible, say so / run the analysis.
3. Literature on the 25 candidates in breast cancer.
4. What research topics could I pursue in breast cancer (DepMap MCP).

**Agent actually did**

- Same #28 N+1 pattern; `limit=500` rejected; 72× `depmap_gene_evidence`.
- Wrote pan-essential exclusion, 25-candidate literature, cell-line effect,
  and research-topic Markdown/CSV under `results/reports/`.
- Mixed DepMap envelopes with literature counts as if they were one evidence
  type; long dumps of manifests (#57).
- `depmap_synthetic_lethal_evidence` schema mismatches (invalid `event`,
  missing source/target).

**Maps to:** **#28**, **#36**, **#57**, **#56**, plus presentation mixing of
literature vs DepMap (fold into #57 / #56, do not add a PubMed-specific
DepMap tool). Fixture: `lineage=Breast`.

---

### 8. `测试8`

**User asked:** GPX4 research directions in breast cancer, using DepMap MCP.

**Agent actually did**

- Swept pair/co-dependency, biomarker, 3D, TF, SL, subtype tools; several
  `MODULE_UNAVAILABLE` / HTTP 500 / catalog `NoneType`.
- Wrote `cmp_gpx4_breast_research_directions.md` and a feasibility report.
- Near-perfect correlations and odd PRISM hits without uniform QC.

**Maps to:** **#57** (progressive disclosure / topic menu), **#56** (QC),
**#36**, **#41** (TF 500), Reader residuals. Fixture: `gene=GPX4`,
`lineage=Breast` — not a GPX4-breast product surface.

---

## Mapping table (case → failure → issue → rewrite)

| Case | User ask (fixture) | Observed failure | Plane | Issue | Proposed rewrite (title-level) |
|---|---|---|---|---|---|
| 依赖基因测试 | Lineage + pan-cancer deps, drop pan-essentials | Selective ranking + N+1 `gene_evidence`; `housekeeping_filter_applied=false` | Query contract | **#28** | Predicate `exclude_common_essential` joins sidecar **before** `limit`; envelope records filter provenance |
| 依赖基因测试 | Report **every** lineage / exact gene | Top-N page as universe | Query contract | **#52** | Exact `(gene, lineage)` match on full table; `matched_row_count` vs page |
| 依赖基因测试 | First turn, local knowledge | `configuration_blocked` missing local path | Remote-compute | **#58** (or new ExecutionContext ticket) | Knowledge root is a declared context; miss → typed blocked status, not folder guessing |
| 真爱基因 | Liver TLG; all-lineage TLG | Pan-cancer catalog + merged pair sources | Provider schema | **#34** | `scope=lineage\|pancancer` on TLG; incommensurable sources must not merge |
| 真爱基因 | Same | `coverage` / `limit` schema lie; Pydantic 500-ish errors | Provider schema | **#35** | Schema ≡ runtime; invalid args → structured 4xx envelope |
| 真爱基因 / 测试4–8 | Query-only chat | `results/reports/*.md` | Presentation | **#36** | `artifact_requested=false` forbids writes |
| 全肿瘤依赖TF / 测试4 | Pan-cancer most-dependent TFs | TF activity tool HTTP 500 | Provider / Reader | **#41** | Catalog-complete TF Reader returns typed status, never `QUERY_ERROR` for valid keys |
| 全肿瘤依赖TF / 测试4 | Same | CRISPR TF-gene vs activity vs roster∩selective | Query contract | **#42** | Distinct `entity_class` predicates; no silent provider substitution |
| 全肿瘤依赖TF / 测试4 | 271 DoRothEA recompute | No universe resource; 1-row CSV; browser roster | Provider schema | **#43** | Frozen TF universe + bounded bulk rank; `max_rows` honored |
| 全肿瘤依赖TF / 测试4 | MCP vanished | Tool catalog empty mid-task | Remote-compute | **#58** | Capability catalog is session-stable; dropout → `MODULE_UNAVAILABLE`, not improvisation |
| 测试4 / 测试 5 | FBXO7 / ATF5 / RUNX2 “impact” | Top-100 selective as “only Myeloid” | Query contract | **#52** | Same exact-key predicate; gene is fixture |
| 测试3 | Liver × PTK7 mutation → knockout | Pan-cancer SL module; inferred mut_n from shortlist | Query contract + status | **#50, #51** | Lineage×event query; `INELIGIBLE` with counts vs thresholds (PR #67) |
| 测试 5 prompt 3 / 测试6 / 测试7 | Exclude pan-essentials; compute field if missing | Same as #28; user asked to **compute** | Query + remote-compute | **#28** + **#58** | Missing column is `NOT_COMPUTED`; compute is a gated Run |
| 测试7 / 测试8 | Breast topics; GPX4 directions | Long manifests; literature mixed with envelopes; unsolicited reports | Presentation | **#57, #56, #36** | Progressive disclosure + shared QC; literature is a separate evidence class |
| 测试8 / many | Catalog `NoneType`; 1-row resources | Reader crash / pagination lie | Reader / catalog | Follow-up on closed #33/#53/#55 **or new ticket** | See “New tickets” |
| — | Epic | Recurring class defects | Architecture | **#54** | Keep as tracker; children are the rows above |
| — | Per-model inspection | Not exercised as a user ask in these dumps | Remote-compute | **#59** | Keep; no case folder maps 1:1 |

---

## Proposed GitHub title/body edits

Rewrite each **open** issue so the first heading is the control-plane
obligation. Keep one `## Regression fixture` section with the case path and
entity keys. Delete “build a Liver/PTK7/GPX4 feature” language.

### #28 — keep, retitle

**Title:** `Query contract: common-essential exclusion is a pre-limit predicate`

**Body (draft):**

```markdown
## Obligation
`ScientificQuery` with `exclude_common_essential=true` (or equivalent
predicate) MUST join a catalog sidecar / core essentiality table on the
**full** lineage ranking **before** applying `limit`. The envelope MUST set
`housekeeping_filter_applied=true` with sidecar provenance.

`ranking=selective` is a statistical contrast, not a pan-essential filter.
If the sidecar is absent: `NOT_COMPUTED` or `MODULE_UNAVAILABLE`, never N+1
`gene_evidence` and never a hand-written rest-mean rule.

Do not add a gene- or lineage-specific tool.

## Regression fixture
`d:\PHD_project\depmap-case\依赖基因测试` (Liver/Breast/Bowel/Ovary +
all-lineage). Also `测试6` (Bowel, user asked for the exclusion **field**),
`测试7` (Breast), `测试 5` prompt 3.

## Tests
Fake tables with mixed common-essential and selective rows. Assert filter
precedes limit; `matched_row_count` counts post-filter rows. No SSH.
```

### #34 — keep, retitle

**Title:** `Provider schema: True Love queries are scope-typed (lineage vs pan-cancer)`

Lineage-scoped TLG is a **different catalog/scope**, not “filter the pan-cancer
table in prose”. If no lineage TLG table exists, return `NOT_COMPUTED` /
`COVERAGE_GAP`. Do not merge `direction_discovery`, effect-correlation
reciprocals, and rank-1 TLG into one list.

Fixture: `真爱基因` Liver then all-lineage.

### #35 — keep, retitle

**Title:** `Provider schema: MCP arguments must match runtime validators`

`coverage` is catalog-specific; `limit` maxima are per-tool. Invalid
combinations return a structured envelope, not a Pydantic traceback.
Fixture: `真爱基因` (`coverage` on `stable_negative_rank1`; limits 60/120/150).

Do **not** close as duplicate of #34.

### #36 — keep, retitle

**Title:** `Presentation: query-only turns must not create project artifacts`

`artifact_requested=false` (default for “看一下 / 汇报”) forbids
`results/reports/**` and local CSV materialization. Answers are bounded
envelopes in chat (#57). Fixture: almost every case folder.

### #41 — keep, retitle

**Title:** `Provider: TF-activity Reader must not QUERY_ERROR on catalog-valid TFs`

Installed + QA-passing `tf_activity_dependency` returns `FOUND` or a typed
coverage status for keys in the frozen universe. HTTP 500 is a Reader defect.
Fixture: `全肿瘤依赖TF` (MYC/AHR/STAT3); `测试 5` (ATF5/ATF4 control).

### #42 — keep, retitle

**Title:** `Query contract: TF-gene CRISPR, TF-activity associations, and roster∩selective are distinct predicates`

Router must emit `entity_class` (or equivalent) and must not substitute
`biomarker_model_evidence` / gene CRISPR summaries for activity associations.
Fixture: `全肿瘤依赖TF` first user turn.

### #43 — keep, retitle

**Title:** `Provider schema: expose frozen TF universe and bounded bulk ranking`

Resource pages honor `max_rows`. Bulk rank reports `matched_row_count`.
Do not reconstruct DoRothEA via browser. Fixture: 271-TF recompute turn.
Related Reader bug (1-row CSV) may be a child of this or a new ticket.

### #50 / #51 — keep (PR #67); retitle if still story-shaped

**#50:** `Query contract: lineage × mutation-event → dependency is a first-class ScientificQuery`

**#51:** `Evidence status: INELIGIBLE for mutation anchors includes counts vs declared thresholds`

Fixture: `测试3` (`PTK7`, `Liver`, `damaging_mutation`). Absence from a
retained shortlist is `NOT_RETAINED`, not a fabricated `mut_n`.

### #52 — keep, retitle

**Title:** `Query contract: exact entity×lineage match runs before top-N`

Absence from `limit=100` is not `NOT_OBSERVED`. Fixture: FBXO7 in `测试4` /
`测试 5`; pan-cancer gene list in `依赖基因测试`.

### #54 — keep as epic

Children = this table. Do not implement the epic as one PR. Do not treat it
as a gene story.

### #56 — keep, retitle

**Title:** `Presentation: shared QC annotations on every EvidenceEnvelope row`

Small-n, sparse pairs, `|r|≈1`, PRISM noise. Fixture: `测试8` GPX4 pairs;
`测试3` mutation groups; `测试 5` TCGA associations.

### #57 — keep, retitle

**Title:** `Presentation: progressive disclosure for scientific envelopes`

Default layer: one-sentence status, bounded top rows, filter/truncation
flags. Manifests and evidence IDs behind expansion. Fixture: `测试7` topic
menu; `测试8` direction dump; all-lineage `依赖基因测试`.

Literature vs DepMap remain separate evidence classes (测试7 “25 candidates
in papers”).

### #58 / #59 — keep

Not gene tickets. Map case evidence:

- #58: DoRothEA “recompute” ask; Bowel “compute the exclusion field”; MCP
  dropout; missing knowledge-root.
- #59: no direct user ask in these dumps; keep for per-model inspection
  without matrix export.

Tests: fake gateway, never real SSH.

---

## New tickets vs close-as-duplicate

**Do not close** #34↔#35, #41↔#42↔#43, #50↔#51, #56↔#57. Different planes.

**Do not open** gene-specific issues (PTK7, GPX4, FBXO7, ATF5, RUNX2, Liver).

**Consider opening** only if still true after PR #66:

1. **Reader: `depmap_analysis_catalog` must not raise `NoneType.get`**
   Seen in 真爱基因, 依赖基因测试, 测试4/6/7/8. If #66 already covers it,
   add a regression test and comment on the closed catalog issue instead of a
   new story ticket.
2. **Reader: catalog resource pages honor `max_rows` (no silent 1-row CSV)**
   Blocking #43. Can be a child of #43 rather than a third TF ticket.
3. **ExecutionContext: knowledge-root miss is a typed blocked status**
   First turn of `依赖基因测试`. Fold into #58 if the epic already owns
   context resolution; otherwise one small ticket.

**Duplicates of sessions, not issues:** `测试4` ≡ `全肿瘤依赖TF` (same UUID).
`.messages.txt` / `.audit.json` are sidecars of the HTML dump.

---

## Implementation order (engineering, not cases)

1. Finish PR #67 (#50/#51) as query-status predicates.
2. #28 + #52 on `scientific_query.py` (filter/match before limit; sidecar join).
3. #35 then #34 (schema truth, then lineage TLG scope).
4. #41 then #43 then #42 (Reader works, universe/bulk exists, router
   disambiguates).
5. #36 + #57 + #56 (presentation; cheaper once envelopes are typed).
6. #58/#59 with fake compute gateway only.

Each PR adds predicates/tests on the shared plane. Named genes stay in fixtures.

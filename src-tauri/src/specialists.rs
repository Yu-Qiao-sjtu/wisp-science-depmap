//! Specialists (专家): user-definable agent personas — instructions plus a
//! skill/MCP subset and a directly-bound model, selectable per session.
//! Stored as a JSON array under the `specialists` settings key (same pattern
//! as `model_profiles`). Built-ins are materialized on first read so user edits
//! to their model bindings persist like any other row.

use serde::{Deserialize, Serialize};
use tauri::State;
use wisp_store::Store;

pub const SPECIALISTS_KEY: &str = "specialists";
pub const DEPMAP_SPECIALIST_ID: &str = "depmap_r_agent";
pub const DEPMAP_REQUIRED_SKILLS: &[&str] = &["depmap-knowledge-query", "depmap-coding-agent"];
pub const SCIENTIFIC_ILLUSTRATOR_RUBRIC: &str = "\
You are the Scientific Illustrator. Turn the user's request and relevant \
project/session context into a finished scientific figure asset, not merely \
drawing advice.\n\n\
Inspect referenced data and files before drawing. Never invent measurements, \
labels, sample sizes, or scientific conclusions. Load `figure-style` for \
data-backed plots and also `figure-composer` for multi-panel figures.\n\n\
Support exactly two output modes. An explicit user choice of format or method \
has the highest priority; tool availability decides only when the user did not \
choose:\n\
- Direct-SVG mode: use this when the user explicitly asks for SVG, vector, an \
editable figure, or direct SVG generation. Create the actual publication-ready \
figure as a descriptive `figures/*.svg` file using `write`, Python, or R. Do \
not call `generate_image`, do not replace the request with PNG, and do not claim \
SVG is unsupported merely because `generate_image` itself only returns PNG. \
After writing the SVG, rasterize that exact SVG to a PNG preview, inspect the \
preview with `view_image`, fix visible problems in the SVG source, then \
re-render and re-inspect. Repeat this SVG -> PNG preview -> SVG correction loop \
until the figure is legible and unclipped. The SVG is the primary deliverable; \
the PNG is only a QA preview.\n\
- PNG image-model mode: use this when the user explicitly asks for PNG, \
`gpt-image-2`, `grok-imagine-image-2.0`, `generate_image`, or image-model \
generation. Call \
`generate_image` with one complete, self-contained visual brief and save a \
descriptive `figures/*.png` file. If `generate_image` is unavailable, explain \
that an image-generation model must be configured; do not silently substitute \
SVG for an explicit PNG or image-model request.\n\
- When the user specifies neither format nor method, use PNG image-model mode \
if `generate_image` is available. Otherwise use Direct-SVG mode, including its \
SVG -> PNG preview -> SVG correction loop, and deliver the SVG.\n\n\
Keep text legible, use colour-blind-safe encodings, and distinguish observed \
data from conceptual illustration. End with a concise explanation and embed \
the saved figure using a project-relative Markdown image link.";

pub const DEPMAP_R_AGENT_RUBRIC: &str = "\
You are the DepMap Agent, a project-level scientific Agent built on Wisp \
Science. The precomputed knowledge base is one evidence backend, not your \
identity or your complete capability. Orchestrate bounded knowledge queries, \
reviewable R analysis, validation, and stage-specific interpretation.\n\n\
When the user's intent semantically matches a registered Workflow, call \
`start_workflow` before any evidence query, Run-history lookup, shell inspection, \
or report write. Bind only the user's supplied gene and cancer scope into the \
Workflow context; do not preload remembered numbers. If `start_workflow` fails, \
report the exact blocker and stop. Do not manually reconstruct the registered \
Workflow, scan raw-data directories, or write an ersatz report. \
Recover the project cycle with `depmap_project_runs` only when the user asks to \
continue, inspect, validate, or create a Run/report; do not load historical Runs \
for a query-only evidence or topic-inventory request. For a cancer-only request \
without a user-supplied gene, call `depmap_query` once with \
`mode=lineage_catalog` and the cancer lineage; never invent an anchor gene from \
model memory. For a gene-in-cancer inventory, topic-ideation, or \
research-direction request, call the fixed `depmap_evidence` tool first; it \
checks provider readiness and assembles a bounded dynamic view. Use \
`depmap_query` for status/catalog inspection or a surgical pair, drug, pathway, \
or term follow-up; load \
`depmap-knowledge-query` when its schemas or fallback scripts are needed. \
Every mode-specific query must include all fields required by the tool schema; \
never learn the contract by deliberately issuing incomplete calls. For \
an empty-argument or invalid-schema failure, do not repeat the identical call; \
correct it once from the visible flat schema or report the block. For \
inventory, discovery, or topic-ideation requests covered by the provider, stay \
query-only: do not use shell, write analysis code, or start a Run. New \
computation requires an explicit user request; saving a report artifact permits \
only the writes needed for that report. Copy storage size and coverage only \
from returned status/catalog fields and never estimate them from memory. \
Never assume the active project itself contains \
`knowledge/`, raw data, or the developer's machine path. Keep the writable \
project root, read-only knowledge root, and read-only data root distinct. When \
the configured knowledge source is healthy and covers the exact request, issue \
a bounded query and do not rerun an available analysis. Distinguish \
`precomputed_query`, `coverage_gap`, `new_analysis_proposed`, \
`new_analysis_authorized`, and `run_validated`; never silently turn a coverage \
gap or connection failure into a raw-data scan or a different data system. Load \
`depmap-coding-agent` before inspecting or generating analysis code only after \
the request explicitly transitions to new computation. \
Treat `data/README.txt` as the release data \
dictionary and `tm00-script/scripts/` as the canonical reference implementation \
when those paths exist. Existing R scripts define required capabilities, data \
semantics, statistical patterns, and figure conventions. Parameterize and \
compose those methods for the question; do not blindly execute a whole example \
script, discard the reference implementation, or rewrite a scientific method \
in Python merely because Python is available.\n\n\
R is the default language for DepMap data loading, statistics, and plots. \
Python may be used for bounded engineering support only when it does not change \
the scientific calculation, unless the user explicitly requests Python or no \
viable R implementation exists. Never modify raw data or canonical reference \
scripts. Write generated code and outputs under `analysis/depmap-agent/`.\n\n\
Keep large matrices in the execution layer. Inspect metadata, headers, \
dimensions, and tiny bounded samples only; never return a full matrix, long \
table, complete log, or binary output into chat. Run substantial R work through \
a persisted `run_in_context` Run with preflight and exact output specifications. \
Use one heavy DepMap matrix Run at a time unless the user approves a different \
resource plan. After a Run succeeds, call `depmap_validate_run` with its Run id \
and project-relative run directory. Do not interpret new numerical results \
unless that tool returns `run_validated`.\n\n\
Use the Skill's compiled capability manifest and read-only resolver before \
writing code. `ready` means the selected capability's direct inputs exist; \
`preprocessing_required` means the audited source data exist and task-local R \
preparation is required; `missing_inputs` blocks only the selected capability. \
Declare only the resolver's `required_datasets` as Run inputs. Never treat all \
DepMap files, or a guessed global core set, as required.\n\n\
The two DepMap Skills are a required baseline even when the project's ordinary \
Skill subset excludes them. The rest of the configured Skill catalog is \
available progressively, not preloaded. Search \
and load only Skills required for the current stage. Use biological analysis \
Skills during computation; use literature or evidence-audit Skills in separate \
bounded tasks; use manuscript or grant-writing Skills only after results pass \
validation. If delegation is enabled, delegate only independent literature or \
review work. A long R calculation is a background Run, not a child Agent. \
If the user's intent matches a registered Workflow, propose it with \
`start_workflow` so the user can approve it; do not manually reconstruct a \
registered Workflow or ask the user to type a trigger phrase.\n\n\
Before interpreting results, verify release provenance, identifier alignment, \
sample counts, missingness, effect direction, cohort filters, confounding, and \
multiple-testing correction. Separate executed observations, literature \
evidence, interpretation, and hypotheses. Never promote correlation or a \
screen-derived candidate to a causal or synthetic-lethal claim without \
independent experimental evidence. Numerical claims must come from a successful \
`depmap_evidence` or `depmap_query` response in the current turn. Preserve its release, manifest, \
sample counts, retention rule, and provenance. `NOT_RETAINED` means absent from \
the sparse top-K output, not no association; `INELIGIBLE` is a cohort threshold \
failure; `NOT_COMPUTED` and `MODULE_UNAVAILABLE` are coverage states. A blocked \
tool call forbids numerical interpretation. Use the canonical lineage returned \
by the query rather than an unnormalized cancer synonym. A DepMap lineage is a \
model-grouping proxy, not proof of a clinical histology or patient cohort; do \
not silently narrow `Liver` to HCC, add a neighboring control lineage, or name \
specific cell lines unless current model metadata or a validated Run supports \
that scope. A `tcga_expression_survival` result is a gene- and cancer-label \
bridge to a patient cohort, never a DepMap-cell-line/TCGA-patient sample join. \
Preserve its endpoint, tumour cohort/event counts, expression scale, Cox-score \
method, and within-project FDR family; do not rename score z as a hazard ratio. \
Mean/median divergence \
does not establish a bimodal distribution, and known gene biology does not \
establish receptor status or molecular subtype unless those fields are returned. \
Continuous expression-to-dependency or enrichment associations do not define an \
`ATF5-high`, high/low, or other discrete subgroup unless the current result \
returns that grouping and its threshold. Never call one section the only \
FDR-significant signal when another returned section also contains an adjusted \
p-value below the stated threshold. `not_testable` and `INELIGIBLE` mean the \
current provider cannot test that event under its thresholds; they do not prove \
that a biological route or future study is infeasible. Likewise, zero models \
crossing a descriptive dependency cutoff supports only that exact observation, \
not the categorical claim that the gene is not a direct dependency. \
Use each result's `semantics.metric`: mutation and CNV `mean_difference` values \
must never be reported as correlation coefficients. Query-only means selecting \
and interpreting existing returned rows; a new matrix, cohort comparison, \
Wilcoxon/Kruskal-Wallis test, FDR calculation, model, or subgroup statistic is \
new computation even if its inputs are precomputed. Do not name a therapeutic \
agent or assert drug actionability unless a current drug-query row or separately \
cited literature evidence supports it. \
Do not relabel `damaging_mutation_n` as pathogenic or clinically causal; it is \
only the count under the provider's damaging-event definition. If a top list \
has no multiple-testing-significant row, report the null result and do not use \
its nominal targets to invent a biological module, named drug, or mechanism. \
The presence of a raw-data file proves asset availability only, not cohort \
eligibility, identifier overlap, statistical power, or that every gap is \
computable. Literature mechanisms, novelty, treatments, and clinical claims \
require an executed literature-evidence task with traceable citations; a Skill \
description or model memory is not literature evidence. \
When tool output is spilled to a named file, read or grep only that exact file \
and never its parent `.wisp/tool-output` directory. End with links or identifiers for generated \
R code, manifests, tables, figures, Runs, and Artifacts.";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Specialist {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub instructions: String,
    /// "" = follow the active model; dangling ids fall back to active too.
    #[serde(default)]
    pub model_id: String,
    /// Reviewer-only backend selection. `None` preserves the legacy behavior:
    /// use `model_id`, falling back to the active HTTP model. Other specialist
    /// personas continue to run inside Wisp's native agent loop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_backend: Option<crate::review::ReviewBackendConfig>,
    /// None = inherit the project skill config; Some = whitelist of skill names.
    #[serde(default)]
    pub skills: Option<Vec<String>>,
    /// None = inherit; Some = whitelist of connector slugs / MCP connection ids.
    #[serde(default)]
    pub connectors: Option<Vec<String>>,
    #[serde(default)]
    pub builtin: bool,
}

pub fn builtin_reviewer() -> Specialist {
    Specialist {
        id: "reviewer".into(),
        name: "Reviewer".into(),
        icon: "review".into(),
        color: "clay".into(),
        description:
            "Traces a session transcript and reports fabrication, hallucination, or plan deviation."
                .into(),
        instructions: crate::review::REVIEWER_RUBRIC.into(),
        model_id: String::new(),
        review_backend: None,
        skills: Some(vec![]), // reviewer runs one-shot; skills are irrelevant
        connectors: Some(vec![]),
        builtin: true,
    }
}

pub fn builtin_reader() -> Specialist {
    Specialist {
        id: "reader".into(),
        name: "Reader".into(),
        icon: "search".into(),
        color: "clay".into(),
        description: "Searches project sessions in parallel and returns compact, cited evidence."
            .into(),
        instructions: crate::project_reader::READER_RUBRIC.into(),
        model_id: String::new(),
        review_backend: None,
        skills: Some(vec![]),
        connectors: Some(vec![]),
        builtin: true,
    }
}

pub fn builtin_scientific_illustrator() -> Specialist {
    Specialist {
        id: "scientific_illustrator".into(),
        name: "Scientific Illustrator".into(),
        icon: "image".into(),
        color: "clay".into(),
        description:
            "Creates publication-ready scientific figures from the request and project context."
                .into(),
        instructions: SCIENTIFIC_ILLUSTRATOR_RUBRIC.into(),
        model_id: String::new(),
        review_backend: None,
        skills: Some(vec!["figure-composer".into(), "figure-style".into()]),
        connectors: Some(vec![]),
        builtin: true,
    }
}

pub fn builtin_depmap_r_agent() -> Specialist {
    Specialist {
        id: DEPMAP_SPECIALIST_ID.into(),
        name: "DepMap Agent".into(),
        icon: "dna".into(),
        color: "clay".into(),
        description: "Queries validated DepMap evidence and orchestrates reproducible R-first analyses when new computation is required."
            .into(),
        instructions: DEPMAP_R_AGENT_RUBRIC.into(),
        model_id: String::new(),
        review_backend: None,
        // Inherit the project's enabled catalog so the Agent can progressively
        // load biology, literature, review, figure, or writing Skills as the
        // current stage requires. The rubric pins depmap-coding-agent as the
        // mandatory analysis contract.
        skills: None,
        connectors: None,
        builtin: true,
    }
}

/// Skills that must remain available for a Specialist even when the project's
/// ordinary enabled-Skill subset excludes them. The rest of the project
/// catalog remains progressively searchable; this is a required baseline, not
/// a complete allowlist.
pub fn required_skill_names(spec: &Specialist) -> &'static [&'static str] {
    match spec.id.as_str() {
        DEPMAP_SPECIALIST_ID => DEPMAP_REQUIRED_SKILLS,
        _ => &[],
    }
}

async fn load_raw(store: &Store) -> Vec<Specialist> {
    store
        .get_setting(SPECIALISTS_KEY)
        .await
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str::<Vec<Specialist>>(&s).ok())
        .unwrap_or_default()
}

async fn save_raw(store: &Store, list: &[Specialist]) -> Result<(), String> {
    let json = serde_json::to_string(list).map_err(|e| e.to_string())?;
    store
        .set_setting(SPECIALISTS_KEY, &json)
        .await
        .map_err(|e| e.to_string())
}

/// Load the list, materializing builtins if absent. Builtin instructions are
/// always re-pinned to their compiled rubrics so improvements ship without a
/// settings migration.
pub async fn ensure(store: &Store) -> Vec<Specialist> {
    let mut list = load_raw(store).await;
    match list.iter_mut().find(|s| s.id == "reviewer") {
        Some(r) => {
            r.builtin = true;
            r.instructions = crate::review::REVIEWER_RUBRIC.into();
        }
        None => list.insert(0, builtin_reviewer()),
    }
    match list.iter_mut().find(|s| s.id == "reader") {
        Some(reader) => {
            reader.builtin = true;
            reader.instructions = crate::project_reader::READER_RUBRIC.into();
            reader.review_backend = None;
            reader.skills = Some(vec![]);
            reader.connectors = Some(vec![]);
        }
        None => list.insert(1.min(list.len()), builtin_reader()),
    }
    match list.iter_mut().find(|s| s.id == "scientific_illustrator") {
        Some(illustrator) => {
            illustrator.builtin = true;
            illustrator.instructions = SCIENTIFIC_ILLUSTRATOR_RUBRIC.into();
            illustrator.review_backend = None;
            illustrator.skills = Some(vec!["figure-composer".into(), "figure-style".into()]);
            illustrator.connectors = Some(vec![]);
        }
        None => list.insert(2.min(list.len()), builtin_scientific_illustrator()),
    }
    match list.iter_mut().find(|s| s.id == "depmap_r_agent") {
        Some(depmap) => {
            depmap.builtin = true;
            depmap.instructions = DEPMAP_R_AGENT_RUBRIC.into();
            depmap.review_backend = None;
        }
        None => list.insert(3.min(list.len()), builtin_depmap_r_agent()),
    }
    list
}

pub async fn get(store: &Store, id: &str) -> Option<Specialist> {
    ensure(store).await.into_iter().find(|s| s.id == id)
}

fn fresh_id(existing: &[Specialist]) -> String {
    for n in 1..10_000 {
        let id = format!("sp{n}");
        if !existing.iter().any(|s| s.id == id) {
            return id;
        }
    }
    "sp".into()
}

/// Create (empty id) or update (existing id). Builtin rows keep their
/// compiled instructions and can never lose `builtin`.
pub async fn upsert(store: &Store, mut spec: Specialist) -> Result<Vec<Specialist>, String> {
    if spec.name.trim().is_empty() {
        return Err("Specialist name is required.".into());
    }
    let mut list = ensure(store).await;
    if spec.id.trim().is_empty() {
        spec.id = fresh_id(&list);
    }
    if let Some(existing) = list.iter_mut().find(|s| s.id == spec.id) {
        if existing.builtin {
            spec.builtin = true;
            spec.instructions = existing.instructions.clone();
        }
        if spec.id == "reviewer" {
            if let Some(crate::review::ReviewBackendConfig::HttpModel { profile_id }) =
                &spec.review_backend
            {
                // Keep the old field in sync so downgrades and older settings
                // surfaces retain the selected HTTP reviewer.
                spec.model_id = profile_id.clone();
            }
        } else if spec.id == "reader" {
            spec.review_backend = None;
            spec.skills = Some(vec![]);
            spec.connectors = Some(vec![]);
        } else if spec.id == "scientific_illustrator" {
            spec.review_backend = None;
            spec.skills = Some(vec!["figure-composer".into(), "figure-style".into()]);
            spec.connectors = Some(vec![]);
        } else if spec.id == "depmap_r_agent" {
            spec.review_backend = None;
        }
        *existing = spec;
    } else {
        spec.builtin = false;
        list.push(spec);
    }
    save_raw(store, &list).await?;
    Ok(ensure(store).await)
}

pub async fn remove(store: &Store, id: &str) -> Result<Vec<Specialist>, String> {
    let mut list = ensure(store).await;
    if list.iter().any(|s| s.id == id && s.builtin) {
        return Err("Built-in specialists cannot be removed.".into());
    }
    list.retain(|s| s.id != id);
    save_raw(store, &list).await?;
    Ok(ensure(store).await)
}

#[tauri::command]
pub async fn list_specialists(
    state: State<'_, crate::AppState>,
) -> Result<Vec<Specialist>, String> {
    Ok(ensure(&state.store).await)
}

#[tauri::command]
pub async fn save_specialist_cmd(
    state: State<'_, crate::AppState>,
    spec: Specialist,
) -> Result<Vec<Specialist>, String> {
    upsert(&state.store, spec).await
}

#[tauri::command]
pub async fn remove_specialist(
    state: State<'_, crate::AppState>,
    id: String,
) -> Result<Vec<Specialist>, String> {
    remove(&state.store, &id).await
}

/// LLM config for a specialist: its bound profile, or the active-model chain
/// when unbound/dangling (soft fallback — personas are not hard capabilities).
pub async fn specialist_llm(
    store: &Store,
    spec: &Specialist,
) -> (String, String, String, String, u64, String, String) {
    if !spec.model_id.trim().is_empty() {
        if let Some(cfg) = crate::models::profile_llm(store, &spec.model_id).await {
            return cfg;
        }
    }
    let (provider, api_url, model, api_key) = crate::load_settings(store).await;
    let (max_tokens, reasoning_effort, service_tier) =
        crate::models::active_llm_advanced(store).await;
    (
        provider,
        api_url,
        model,
        api_key,
        max_tokens,
        reasoning_effort,
        service_tier,
    )
}

pub async fn specialist_context_window(store: &Store, spec: &Specialist) -> u64 {
    if !spec.model_id.trim().is_empty() {
        if let Some(window) = crate::models::profile_context_window(store, &spec.model_id).await {
            return window;
        }
    }
    crate::models::active_context_window(store).await
}

fn frame_key(frame_id: &str) -> String {
    format!("frame_specialist:{frame_id}")
}

pub async fn set_frame_specialist(store: &Store, frame_id: &str, id: &str) -> Result<(), String> {
    store
        .set_setting(&frame_key(frame_id), id)
        .await
        .map_err(|e| e.to_string())
}

pub async fn frame_specialist_id(store: &Store, frame_id: &str) -> Option<String> {
    store
        .get_setting(&frame_key(frame_id))
        .await
        .ok()
        .flatten()
        .filter(|id| !id.trim().is_empty())
}

pub async fn project_default_specialist_id(store: &Store, project_id: &str) -> Option<String> {
    let id = store
        .project_default_specialist(project_id)
        .await
        .ok()
        .flatten()?;
    get(store, &id).await.map(|_| id)
}

pub async fn set_project_default_specialist(
    store: &Store,
    project_id: &str,
    id: &str,
) -> Result<(), String> {
    if !id.is_empty() && get(store, id).await.is_none() {
        return Err(format!("Unknown specialist '{id}'."));
    }
    store
        .set_project_default_specialist(project_id, id)
        .await
        .map_err(|error| error.to_string())
}

/// Seed one fresh frame from the project's durable default. Existing frames
/// are never rewritten: changing the project default applies only to future
/// conversations, preserving the prompt identity of conversations in flight.
pub async fn inherit_project_default_specialist(
    store: &Store,
    project_id: &str,
    frame_id: &str,
) -> Result<(), String> {
    if let Some(id) = project_default_specialist_id(store, project_id).await {
        set_frame_specialist(store, frame_id, &id).await?;
    }
    Ok(())
}

pub async fn session_specialist(store: &Store, frame_id: &str) -> Option<Specialist> {
    let id = frame_specialist_id(store, frame_id).await?;
    get(store, &id).await
}

/// The UI disables the picker once a session has messages; this backend guard
/// enforces the same rule for any other caller.
#[tauri::command]
pub async fn set_session_specialist(
    state: State<'_, crate::AppState>,
    frame_id: String,
    id: String,
) -> Result<(), String> {
    let msgs = state
        .store
        .load_messages(&frame_id)
        .await
        .map_err(|e| format!("{e}"))?;
    if msgs.iter().any(|m| m.role != wisp_llm::Role::System) {
        return Err("Specialist is locked once the session has messages.".into());
    }
    if !id.is_empty() && get(&state.store, &id).await.is_none() {
        return Err(format!("Unknown specialist '{id}'."));
    }
    set_frame_specialist(&state.store, &frame_id, &id).await
}

#[tauri::command]
pub async fn get_session_specialist(
    state: State<'_, crate::AppState>,
    frame_id: String,
) -> Result<Option<Specialist>, String> {
    Ok(session_specialist(&state.store, &frame_id).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn illustrator_rubric_gives_explicit_svg_requests_priority() {
        let rubric = SCIENTIFIC_ILLUSTRATOR_RUBRIC;
        let svg_rule = rubric
            .find("when the user explicitly asks for SVG")
            .expect("rubric must define explicit SVG routing");
        let fallback_rule = rubric
            .find("When the user specifies neither format nor method")
            .expect("rubric must define the tool-availability fallback");

        assert!(svg_rule < fallback_rule);
        assert!(rubric.contains("explicit user choice of format or method"));
        assert!(rubric.contains("Do not call `generate_image`"));
        assert!(rubric.contains("rasterize that exact SVG to a PNG preview"));
        assert!(rubric.contains("inspect the preview with `view_image`"));
        assert!(rubric.contains("SVG -> PNG preview -> SVG correction loop"));
        assert!(rubric.contains("The SVG is the primary deliverable"));
        assert!(rubric.contains("explicitly asks for PNG"));
        assert!(rubric.contains("do not silently substitute"));
    }

    #[test]
    fn depmap_rubric_prefers_bounded_knowledge_queries_before_compute() {
        let rubric = DEPMAP_R_AGENT_RUBRIC;
        assert!(rubric.contains("load `depmap-knowledge-query`"));
        assert!(rubric.contains("Load `depmap-coding-agent`"));
        assert!(rubric.contains("do not rerun an available analysis"));
        assert!(rubric.contains("never learn the contract"));
        assert!(rubric.contains("stay query-only"));
        assert!(rubric.contains("never estimate them from memory"));
        assert!(rubric.contains("do not manually reconstruct"));
        assert!(rubric.contains("R is the default language"));
        assert!(rubric.contains("canonical reference implementation"));
        assert!(rubric.contains("persisted `run_in_context` Run"));
        assert!(rubric.contains("compiled capability manifest"));
        assert!(rubric.contains("`preprocessing_required`"));
        assert!(rubric.contains("`required_datasets`"));
        assert!(rubric.contains("Search and load only Skills required"));
        assert!(rubric.contains("background Run, not a child Agent"));
        assert!(rubric.contains("screen-derived candidate"));
    }

    #[test]
    fn depmap_manifest_is_closed_safe_and_covers_the_audited_scripts() {
        use std::collections::HashSet;

        let manifest: serde_json::Value = serde_json::from_str(include_str!(
            "../../skills/depmap-coding-agent/references/capability-manifest.json"
        ))
        .expect("compiled DepMap capability manifest must be valid JSON");
        assert_eq!(manifest["schema_version"], 1);

        let datasets = manifest["datasets"].as_array().unwrap();
        let capabilities = manifest["capabilities"].as_array().unwrap();
        assert_eq!(capabilities.len(), 21);
        let dataset_ids: HashSet<&str> = datasets
            .iter()
            .map(|dataset| dataset["id"].as_str().unwrap())
            .collect();
        assert_eq!(
            dataset_ids.len(),
            datasets.len(),
            "dataset ids must be unique"
        );

        for dataset in datasets {
            let path = dataset["path"].as_str().unwrap();
            assert!(!path.starts_with('/') && !path.contains(':'));
            assert!(!path.split('/').any(|part| part == ".."));
            if let Some(source_inputs) = dataset["source_inputs"].as_array() {
                for source in source_inputs {
                    assert!(dataset_ids.contains(source.as_str().unwrap()));
                }
            }
        }

        let mut capability_ids = HashSet::new();
        let mut scripts = HashSet::new();
        for capability in capabilities {
            assert!(capability_ids.insert(capability["id"].as_str().unwrap()));
            for input in capability["inputs"].as_array().unwrap() {
                assert!(dataset_ids.contains(input.as_str().unwrap()));
            }
            for script in capability["scripts"].as_array().unwrap() {
                assert!(scripts.insert(script.as_str().unwrap()));
            }
        }
        let expected_scripts: HashSet<&str> = [
            "01_read_depmap.r",
            "02_gene_gene_correlation.R",
            "03_co_dependency.R",
            "04_predivtive_biomarkers.R",
            "05_from_gene_to_dependency.R",
            "05.1_ml_pathway.R",
            "05.2_synthetic_lethal.R",
            "05.3_drug_sensitivity.R",
            "05.4_wgcna.R",
            "07_mutant_dependency_22Q2.R",
            "08_mutData_updata_23Q2.R",
            "09_batch_from_mut_to_target_23Q2.R",
            "10_batch_from_mut_to_target_23Q2_add_celltype.R",
            "11_batch_from_gene_to_mut_23Q2_add_celltype.R",
            "12_CCNE1_AMP_PKMYT1.R",
            "13_MYCN_DDX1_coamplification_Cancer_discovery.R",
            "14_DCAF5_SMARCB1_Nature.R",
            "15_Sanger_CRISPR.R",
            "16_Dependency_nagative_correlation.R",
            "17_bipolar_dependency_ASB7_as_example.R",
            "18_DrugAUC_and_DepMap_MTAPasExample.R",
        ]
        .into_iter()
        .collect();
        assert_eq!(scripts, expected_scripts);

        for dataset in datasets
            .iter()
            .filter(|dataset| dataset["kind"] == "derived")
        {
            for producer in dataset["producer_scripts"].as_array().unwrap() {
                assert!(scripts.contains(producer.as_str().unwrap()));
            }
        }
    }

    async fn test_store() -> (wisp_store::Store, std::path::PathBuf) {
        let tmp = std::env::temp_dir().join(format!("wisp_spec_{}.sqlite", uuid::Uuid::new_v4()));
        (wisp_store::Store::open(&tmp).await.unwrap(), tmp)
    }

    #[tokio::test]
    async fn ensure_materializes_builtin_specialists_once() {
        let (store, tmp) = test_store().await;
        let list = ensure(&store).await;
        assert_eq!(list.len(), 4);
        let r = &list[0];
        assert_eq!(r.id, "reviewer");
        assert!(r.builtin);
        assert_eq!(r.instructions, crate::review::REVIEWER_RUBRIC);
        let reader = &list[1];
        assert_eq!(reader.id, "reader");
        assert!(reader.builtin);
        assert_eq!(reader.instructions, crate::project_reader::READER_RUBRIC);
        let illustrator = &list[2];
        assert_eq!(illustrator.id, "scientific_illustrator");
        assert!(illustrator.builtin);
        assert_eq!(illustrator.instructions, SCIENTIFIC_ILLUSTRATOR_RUBRIC);
        assert_eq!(
            illustrator.skills.as_deref(),
            Some(&["figure-composer".to_string(), "figure-style".to_string()][..])
        );
        let depmap = &list[3];
        assert_eq!(depmap.id, "depmap_r_agent");
        assert_eq!(depmap.name, "DepMap Agent");
        assert!(depmap.builtin);
        assert_eq!(depmap.instructions, DEPMAP_R_AGENT_RUBRIC);
        assert_eq!(depmap.skills, None, "DepMap must inherit hot-loaded Skills");
        assert_eq!(depmap.connectors, None);
        assert_eq!(
            required_skill_names(depmap),
            &["depmap-knowledge-query", "depmap-coding-agent"]
        );
        // Second read does not duplicate the built-ins.
        assert_eq!(ensure(&store).await.len(), 4);
        let _ = std::fs::remove_file(&tmp);
    }

    #[tokio::test]
    async fn upsert_roundtrip_and_fresh_id() {
        let (store, tmp) = test_store().await;
        let spec = Specialist {
            id: String::new(),
            name: "Paper hunter".into(),
            icon: "search".into(),
            color: "clay".into(),
            description: "finds papers".into(),
            instructions: "You hunt papers.".into(),
            model_id: "m1".into(),
            review_backend: None,
            skills: Some(vec!["bear-support".into()]),
            connectors: None,
            builtin: false,
        };
        let list = upsert(&store, spec).await.unwrap();
        let created = list.iter().find(|s| !s.builtin).unwrap();
        assert_eq!(created.id, "sp1");
        assert_eq!(
            created.skills.as_deref(),
            Some(&["bear-support".to_string()][..])
        );
        // Edit by id keeps the id.
        let mut edited = created.clone();
        edited.name = "Paper hunter 2".into();
        let list = upsert(&store, edited).await.unwrap();
        assert_eq!(list.iter().filter(|s| !s.builtin).count(), 1);
        assert_eq!(
            list.iter().find(|s| s.id == "sp1").unwrap().name,
            "Paper hunter 2"
        );
        let _ = std::fs::remove_file(&tmp);
    }

    #[tokio::test]
    async fn builtin_specialist_guards() {
        let (store, tmp) = test_store().await;
        ensure(&store).await;
        assert!(remove(&store, "reviewer").await.is_err());
        assert!(remove(&store, "reader").await.is_err());
        assert!(remove(&store, "scientific_illustrator").await.is_err());
        assert!(remove(&store, "depmap_r_agent").await.is_err());
        // Editing the builtin keeps instructions but accepts a model change.
        let mut r = get(&store, "reviewer").await.unwrap();
        r.instructions = "haha".into();
        r.model_id = "m2".into();
        let list = upsert(&store, r).await.unwrap();
        let r = list.iter().find(|s| s.id == "reviewer").unwrap();
        assert_eq!(r.instructions, crate::review::REVIEWER_RUBRIC);
        assert_eq!(r.model_id, "m2");

        let mut reader = get(&store, "reader").await.unwrap();
        reader.instructions = "replace rubric".into();
        reader.model_id = "cheap".into();
        reader.skills = None;
        let list = upsert(&store, reader).await.unwrap();
        let reader = list
            .iter()
            .find(|specialist| specialist.id == "reader")
            .unwrap();
        assert_eq!(reader.instructions, crate::project_reader::READER_RUBRIC);
        assert_eq!(reader.model_id, "cheap");
        assert_eq!(reader.skills, Some(vec![]));

        let mut illustrator = get(&store, "scientific_illustrator").await.unwrap();
        illustrator.instructions = "replace rubric".into();
        illustrator.skills = None;
        let list = upsert(&store, illustrator).await.unwrap();
        let illustrator = list
            .iter()
            .find(|specialist| specialist.id == "scientific_illustrator")
            .unwrap();
        assert_eq!(illustrator.instructions, SCIENTIFIC_ILLUSTRATOR_RUBRIC);
        assert_eq!(
            illustrator.skills,
            Some(vec!["figure-composer".into(), "figure-style".into()])
        );

        let mut depmap = get(&store, "depmap_r_agent").await.unwrap();
        depmap.instructions = "replace rubric".into();
        depmap.model_id = "r-model".into();
        depmap.skills = Some(vec!["depmap-coding-agent".into()]);
        let list = upsert(&store, depmap).await.unwrap();
        let depmap = list
            .iter()
            .find(|specialist| specialist.id == "depmap_r_agent")
            .unwrap();
        assert_eq!(depmap.instructions, DEPMAP_R_AGENT_RUBRIC);
        assert_eq!(depmap.model_id, "r-model");
        assert_eq!(depmap.skills, Some(vec!["depmap-coding-agent".into()]));
        let _ = std::fs::remove_file(&tmp);
    }

    #[tokio::test]
    async fn specialist_llm_falls_back_to_active_for_empty_or_dangling() {
        let (store, tmp) = test_store().await;
        // No model profiles configured: active resolution still returns the
        // env/default fallback chain from load_settings.
        let spec = Specialist {
            model_id: "no-such".into(),
            review_backend: None,
            ..builtin_reviewer()
        };
        let (provider, api_url, model, _key, _mt, _re, _st) = specialist_llm(&store, &spec).await;
        assert!(!provider.is_empty());
        assert!(!api_url.is_empty());
        assert!(!model.is_empty());
        let _ = std::fs::remove_file(&tmp);
    }

    #[tokio::test]
    async fn session_specialist_set_get_and_lock() {
        let (store, tmp) = test_store().await;
        ensure(&store).await;
        store.create_project("p1", "proj", "").await.unwrap();
        store.create_frame("f1", "p1", "OPERON", "m").await.unwrap();
        set_frame_specialist(&store, "f1", "reviewer")
            .await
            .unwrap();
        assert_eq!(
            session_specialist(&store, "f1").await.unwrap().id,
            "reviewer"
        );
        // Clearing works.
        set_frame_specialist(&store, "f1", "").await.unwrap();
        assert!(session_specialist(&store, "f1").await.is_none());
        let _ = std::fs::remove_file(&tmp);
    }

    #[tokio::test]
    async fn project_default_specialist_seeds_only_new_frames() {
        let (store, tmp) = test_store().await;
        ensure(&store).await;
        store.create_project("p1", "proj", "").await.unwrap();
        store
            .create_frame("old", "p1", "OPERON", "m")
            .await
            .unwrap();

        set_project_default_specialist(&store, "p1", "depmap_r_agent")
            .await
            .unwrap();
        assert_eq!(
            project_default_specialist_id(&store, "p1").await.as_deref(),
            Some("depmap_r_agent")
        );
        assert!(session_specialist(&store, "old").await.is_none());

        store
            .create_frame("new", "p1", "OPERON", "m")
            .await
            .unwrap();
        inherit_project_default_specialist(&store, "p1", "new")
            .await
            .unwrap();
        assert_eq!(
            session_specialist(&store, "new").await.unwrap().id,
            "depmap_r_agent"
        );

        assert!(set_project_default_specialist(&store, "p1", "missing")
            .await
            .is_err());
        set_project_default_specialist(&store, "p1", "")
            .await
            .unwrap();
        assert!(project_default_specialist_id(&store, "p1").await.is_none());
        let _ = std::fs::remove_file(&tmp);
    }

    #[tokio::test]
    async fn reviewer_model_binding_feeds_review_config() {
        let (store, tmp) = test_store().await;
        let mut r = get(&store, "reviewer").await.unwrap();
        r.model_id = "does-not-exist".into();
        upsert(&store, r).await.unwrap();
        // Dangling binding falls back to the active chain — never errors.
        let spec = get(&store, "reviewer").await.unwrap();
        let (_p, _u, model, _k, _mt, _re, _st) = specialist_llm(&store, &spec).await;
        assert!(!model.is_empty());
        let _ = std::fs::remove_file(&tmp);
    }

    #[tokio::test]
    async fn reviewer_backend_roundtrips_and_keeps_legacy_http_binding() {
        let (store, tmp) = test_store().await;
        let mut reviewer = get(&store, "reviewer").await.unwrap();
        reviewer.review_backend = Some(crate::review::ReviewBackendConfig::AcpAgent {
            profile_id: "acp-1".into(),
        });
        upsert(&store, reviewer).await.unwrap();
        assert_eq!(
            get(&store, "reviewer").await.unwrap().review_backend,
            Some(crate::review::ReviewBackendConfig::AcpAgent {
                profile_id: "acp-1".into()
            })
        );

        let mut reviewer = get(&store, "reviewer").await.unwrap();
        reviewer.review_backend = Some(crate::review::ReviewBackendConfig::HttpModel {
            profile_id: "http-2".into(),
        });
        upsert(&store, reviewer).await.unwrap();
        let reviewer = get(&store, "reviewer").await.unwrap();
        assert_eq!(reviewer.model_id, "http-2");
        assert_eq!(
            reviewer.review_backend,
            Some(crate::review::ReviewBackendConfig::HttpModel {
                profile_id: "http-2".into()
            })
        );
        let _ = std::fs::remove_file(&tmp);
    }
}

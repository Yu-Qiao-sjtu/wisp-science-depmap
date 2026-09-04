//! Project-scoped DepMap knowledge routing.
//!
//! The knowledge query tool is deliberately read-only and bounded. It never
//! opens raw DepMap matrices and never starts a recomputation; a coverage gap
//! must transition to the persisted Run path explicitly.

use crate::models;
use futures_util::{stream, StreamExt};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;
use tokio::process::Command;
use url::{Host, Url};
use wisp_llm::ToolSchema;
use wisp_tools::{Tool, ToolEnv, ToolResult};

const TOOL_NAME: &str = "depmap_query";
const EVIDENCE_TOOL_NAME: &str = "depmap_evidence";
const ROUTE_TOOL_NAME: &str = "depmap_agent_route";
const EVIDENCE_HISTORY_TOOL_NAME: &str = "depmap_evidence_history";
const PROJECT_RUNS_TOOL_NAME: &str = "depmap_project_runs";
const VALIDATE_RUN_TOOL_NAME: &str = "depmap_validate_run";
const SKILL_NAME: &str = "depmap-knowledge-query";
const MAX_REMOTE_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const MAX_VALIDATION_JSON_BYTES: u64 = 2 * 1024 * 1024;
const MAX_TOP_LIMIT: i64 = 100;
// One evidence request fans out to a bounded set of module queries. Three retained rows
// per query are enough for topic triage while keeping the complete bundle in
// the model's first tool result. Surgical follow-ups remain available through
// `depmap_query` after the initial view.
const MAX_EVIDENCE_LIMIT: i64 = 3;
const MAX_EVIDENCE_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_LEDGER_PAYLOAD_BYTES: usize = 256 * 1024;
const EVIDENCE_SECTIONS: &[&str] = &[
    "core",
    "networks",
    "mutation",
    "cnv",
    "drug",
    "enrichment",
    "tcga",
];
const MATRIX_MODULES: &[&str] = &[
    "effect_correlation",
    "expression_correlation",
    "expression_dependency",
    "damaging_mutation_dependency",
    "custom_missense_mutation_dependency",
    "hotspot_mutation_dependency",
    "cnv_amplification_dependency",
];
const LINEAGE_EVENTS: &[&str] = &["damaging", "custom_missense", "hotspot"];
const DRUG_OMICS: &[&str] = &["effect", "expression", "cnv"];
const DIRECTION_FOCI: &[&str] = &[
    "all",
    "transcription_factor",
    "pathway",
    "network",
    "cnv",
    "drug",
];
const LINEAGE_NETWORK_FAMILIES: &[&str] = &[
    "effect_correlation",
    "expression_correlation",
    "expression_dependency",
];
const LINEAGE_DEPENDENCY_RANKINGS: &[&str] = &["selective", "mean_dependency"];
const CANONICAL_LINEAGES: &[&str] = &[
    "Adrenal Gland",
    "Ampulla of Vater",
    "Biliary Tract",
    "Bladder Urinary Tract",
    "Bone",
    "Bowel",
    "Breast",
    "Cervix",
    "CNS Brain",
    "Embryonal",
    "Esophagus Stomach",
    "Eye",
    "Fibroblast",
    "Hair",
    "Head and Neck",
    "Kidney",
    "Liver",
    "Lung",
    "Lymphoid",
    "Muscle",
    "Myeloid",
    "Normal",
    "Other",
    "Ovary Fallopian Tube",
    "Pancreas",
    "Peripheral Nervous System",
    "Pleura",
    "Prostate",
    "Skin",
    "Soft Tissue",
    "Testis",
    "Thyroid",
    "Uterus",
    "Vulva Vagina",
];
const CHINESE_LINEAGE_ALIASES: &[(&str, &str)] = &[
    ("肾上腺", "Adrenal Gland"),
    ("肾上腺癌", "Adrenal Gland"),
    ("肾上腺肿瘤", "Adrenal Gland"),
    ("Vater壶腹", "Ampulla of Vater"),
    ("壶腹部", "Ampulla of Vater"),
    ("壶腹癌", "Ampulla of Vater"),
    ("壶腹部癌", "Ampulla of Vater"),
    ("胆道", "Biliary Tract"),
    ("胆道癌", "Biliary Tract"),
    ("胆管癌", "Biliary Tract"),
    ("胆囊癌", "Biliary Tract"),
    ("膀胱", "Bladder Urinary Tract"),
    ("膀胱癌", "Bladder Urinary Tract"),
    ("尿路", "Bladder Urinary Tract"),
    ("尿路癌", "Bladder Urinary Tract"),
    ("尿路上皮癌", "Bladder Urinary Tract"),
    ("骨", "Bone"),
    ("骨癌", "Bone"),
    ("骨肿瘤", "Bone"),
    ("骨肉瘤", "Bone"),
    ("肠道", "Bowel"),
    ("肠癌", "Bowel"),
    ("大肠癌", "Bowel"),
    ("结肠癌", "Bowel"),
    ("直肠癌", "Bowel"),
    ("结直肠癌", "Bowel"),
    ("乳腺", "Breast"),
    ("乳腺癌", "Breast"),
    ("乳癌", "Breast"),
    ("宫颈", "Cervix"),
    ("宫颈癌", "Cervix"),
    ("子宫颈癌", "Cervix"),
    ("中枢神经系统", "CNS Brain"),
    ("脑", "CNS Brain"),
    ("脑癌", "CNS Brain"),
    ("脑肿瘤", "CNS Brain"),
    ("胶质瘤", "CNS Brain"),
    ("胚胎性", "Embryonal"),
    ("胚胎性肿瘤", "Embryonal"),
    ("胚胎肿瘤", "Embryonal"),
    ("食管", "Esophagus Stomach"),
    ("食管癌", "Esophagus Stomach"),
    ("胃", "Esophagus Stomach"),
    ("胃癌", "Esophagus Stomach"),
    ("食管胃", "Esophagus Stomach"),
    ("食管胃癌", "Esophagus Stomach"),
    ("胃食管癌", "Esophagus Stomach"),
    ("眼", "Eye"),
    ("眼部", "Eye"),
    ("眼部肿瘤", "Eye"),
    ("眼癌", "Eye"),
    ("葡萄膜黑色素瘤", "Eye"),
    ("成纤维细胞", "Fibroblast"),
    ("成纤维细胞系", "Fibroblast"),
    ("毛发", "Hair"),
    ("毛囊", "Hair"),
    ("头颈", "Head and Neck"),
    ("头颈癌", "Head and Neck"),
    ("口腔癌", "Head and Neck"),
    ("咽癌", "Head and Neck"),
    ("喉癌", "Head and Neck"),
    ("肾", "Kidney"),
    ("肾癌", "Kidney"),
    ("肾脏癌", "Kidney"),
    ("肾细胞癌", "Kidney"),
    ("肝", "Liver"),
    ("肝癌", "Liver"),
    ("肝脏癌", "Liver"),
    ("肝脏肿瘤", "Liver"),
    ("肺", "Lung"),
    ("肺癌", "Lung"),
    ("肺部肿瘤", "Lung"),
    ("淋巴", "Lymphoid"),
    ("淋巴系统", "Lymphoid"),
    ("淋巴系统肿瘤", "Lymphoid"),
    ("淋巴瘤", "Lymphoid"),
    ("淋巴细胞白血病", "Lymphoid"),
    ("肌肉", "Muscle"),
    ("肌肉肿瘤", "Muscle"),
    ("髓系", "Myeloid"),
    ("髓系肿瘤", "Myeloid"),
    ("髓系白血病", "Myeloid"),
    ("急性髓系白血病", "Myeloid"),
    ("正常", "Normal"),
    ("正常组织", "Normal"),
    ("正常细胞", "Normal"),
    ("其他", "Other"),
    ("其他肿瘤", "Other"),
    ("卵巢", "Ovary Fallopian Tube"),
    ("卵巢癌", "Ovary Fallopian Tube"),
    ("输卵管", "Ovary Fallopian Tube"),
    ("输卵管癌", "Ovary Fallopian Tube"),
    ("卵巢输卵管", "Ovary Fallopian Tube"),
    ("胰腺", "Pancreas"),
    ("胰腺癌", "Pancreas"),
    ("胰癌", "Pancreas"),
    ("外周神经系统", "Peripheral Nervous System"),
    ("周围神经系统", "Peripheral Nervous System"),
    ("外周神经系统肿瘤", "Peripheral Nervous System"),
    ("神经母细胞瘤", "Peripheral Nervous System"),
    ("胸膜", "Pleura"),
    ("胸膜肿瘤", "Pleura"),
    ("胸膜间皮瘤", "Pleura"),
    ("间皮瘤", "Pleura"),
    ("前列腺", "Prostate"),
    ("前列腺癌", "Prostate"),
    ("皮肤", "Skin"),
    ("皮肤癌", "Skin"),
    ("皮肤肿瘤", "Skin"),
    ("黑色素瘤", "Skin"),
    ("软组织", "Soft Tissue"),
    ("软组织肿瘤", "Soft Tissue"),
    ("软组织肉瘤", "Soft Tissue"),
    ("睾丸", "Testis"),
    ("睾丸癌", "Testis"),
    ("睾丸肿瘤", "Testis"),
    ("甲状腺", "Thyroid"),
    ("甲状腺癌", "Thyroid"),
    ("甲状腺肿瘤", "Thyroid"),
    ("子宫", "Uterus"),
    ("子宫癌", "Uterus"),
    ("子宫体癌", "Uterus"),
    ("子宫内膜癌", "Uterus"),
    ("外阴", "Vulva Vagina"),
    ("外阴癌", "Vulva Vagina"),
    ("阴道", "Vulva Vagina"),
    ("阴道癌", "Vulva Vagina"),
    ("外阴阴道", "Vulva Vagina"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
enum KnowledgeProvider {
    Local { root: PathBuf },
    Remote { endpoint: Url },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct KnowledgeWorkspace {
    provider: KnowledgeProvider,
    release: Option<String>,
}

#[derive(Clone)]
pub(crate) struct DepMapQueryTool {
    project_root: PathBuf,
    query_script: PathBuf,
    store: wisp_store::Store,
    project_id: String,
    frame_id: String,
}

pub(crate) struct DepMapEvidenceTool {
    query: DepMapQueryTool,
}

/// Records a typed, inspectable routing decision before the DepMap Agent uses
/// evidence or compute tools. The model extracts the user's intent and
/// entities; the host validates the required slots and chooses the execution
/// level. This tool never supplies scientific evidence or starts work.
pub(crate) struct DepMapAgentRouteTool;

pub(crate) struct DepMapEvidenceHistoryTool {
    store: wisp_store::Store,
    project_id: String,
    frame_id: String,
}

/// Specialist-only replacement for the generic completion tool.  The model
/// must submit machine-readable evidence bindings alongside its prose; this
/// tool verifies them against evidence persisted by DepMap tools during the
/// active turn before the answer becomes user-visible.
pub(crate) struct DepMapCompletionGateTool {
    store: wisp_store::Store,
    project_id: String,
    frame_id: String,
    evidence_not_before_ms: AtomicI64,
}

impl DepMapCompletionGateTool {
    pub(crate) fn new(store: wisp_store::Store, project_id: String, frame_id: String) -> Self {
        Self {
            store,
            project_id,
            frame_id,
            evidence_not_before_ms: AtomicI64::new(chrono::Utc::now().timestamp_millis()),
        }
    }
}

#[async_trait::async_trait]
impl Tool for DepMapCompletionGateTool {
    fn name(&self) -> &str {
        "attempt_completion"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "attempt_completion",
            "Submit the final DepMap answer through the host grounding gate. For every scientific data claim, add a compact marker such as [E1] in result and provide a matching evidence_bindings item. Numerical claims must point to the exact JSON Pointer containing that value in a successful depmap_query or depmap_evidence result from this turn. The host rejects stale evidence, missing pointers, unsupported numbers, causal overclaims, and hidden sparse-result states.",
            json!({
                "type":"object",
                "properties": {
                    "result": {"type":"string","description":"Complete user-facing answer. Put [E1], [E2], etc. immediately after the claim each binding supports."},
                    "evidence_bindings": {
                        "type":"array",
                        "description":"Machine-verifiable support for scientific claims; use [] only for a non-scientific blocker/status answer.",
                        "items": {
                            "type":"object",
                            "properties": {
                                "label":{"type":"string","pattern":"^E[1-9][0-9]*$"},
                                "claim":{"type":"string","description":"Exact substring copied from result, including the [E#] marker."},
                                "evidence_id":{"type":"string"},
                                "json_pointer":{"type":"string","description":"RFC 6901 pointer into the persisted compact evidence payload."}
                            },
                            "required":["label","claim","evidence_id","json_pointer"],
                            "additionalProperties":false
                        }
                    }
                },
                "required":["result","evidence_bindings"],
                "additionalProperties":false
            }),
        )
    }

    async fn run(&self, args: &Value, _env: &dyn ToolEnv) -> ToolResult {
        let result = match args
            .get("result")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(value) => value.to_string(),
            None => {
                return ToolResult::fail(grounding_failure(vec![
                    "result must contain the complete final user-facing answer".into(),
                ]))
            }
        };
        let bindings = match args.get("evidence_bindings").and_then(Value::as_array) {
            Some(value) => value,
            None => {
                return ToolResult::fail(grounding_failure(vec![
                    "evidence_bindings must be an array (use [] only for a non-scientific blocker/status answer)".into(),
                ]))
            }
        };
        let not_before = self.evidence_not_before_ms.load(Ordering::SeqCst);
        match validate_completion_grounding(
            &self.store,
            &self.project_id,
            &self.frame_id,
            not_before,
            &result,
            bindings,
        )
        .await
        {
            Ok(()) => {
                self.evidence_not_before_ms
                    .store(chrono::Utc::now().timestamp_millis(), Ordering::SeqCst);
                ToolResult::ok(completion_with_evidence_index(&result, bindings)).stop_turn()
            }
            Err(errors) => ToolResult::fail(grounding_failure(errors)),
        }
    }
}

impl DepMapEvidenceHistoryTool {
    pub(crate) fn new(store: wisp_store::Store, project_id: String, frame_id: String) -> Self {
        Self {
            store,
            project_id,
            frame_id,
        }
    }
}

#[async_trait::async_trait]
impl Tool for DepMapEvidenceHistoryTool {
    fn name(&self) -> &str {
        EVIDENCE_HISTORY_TOOL_NAME
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            EVIDENCE_HISTORY_TOOL_NAME,
            "Read persisted DepMap evidence from this conversation. Use mode=recent to recover compact evidence identities, or mode=get with an exact evidence_id to retrieve one stored compact payload. Do not use this instead of a fresh query when the user changed the gene, cancer, release, or scientific scope.",
            json!({
                "type":"object",
                "properties": {
                    "mode":{"type":"string","enum":["recent","get"]},
                    "evidence_id":{"type":"string"},
                    "limit":{"type":"integer","minimum":1,"maximum":20}
                },
                "required":["mode"],
                "additionalProperties":false
            }),
        )
    }

    fn read_only(&self) -> bool {
        true
    }

    fn preview(&self, args: &Value) -> String {
        args.get("evidence_id")
            .and_then(Value::as_str)
            .unwrap_or("recent DepMap evidence")
            .to_string()
    }

    async fn run(&self, args: &Value, _env: &dyn ToolEnv) -> ToolResult {
        match args.get("mode").and_then(Value::as_str) {
            Some("recent") => {
                let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(10);
                if !(1..=20).contains(&limit) {
                    return ToolResult::fail(blocked(
                        "invalid_evidence_history_request",
                        "limit must be between 1 and 20",
                    ));
                }
                match self
                    .store
                    .list_scientific_evidence(&self.project_id, &self.frame_id, limit as u32)
                    .await
                {
                    Ok(records) => ToolResult::ok(pretty(json!({
                        "state":"evidence_history",
                        "records": records.into_iter().map(|record| json!({
                            "evidence_id":record.evidence_id,
                            "provider":record.provider,
                            "provider_version":record.provider_version,
                            "tool_name":record.tool_name,
                            "arguments":serde_json::from_str::<Value>(&record.canonical_arguments_json).unwrap_or(Value::Null),
                            "evidence_state":record.evidence_state,
                            "updated_at":record.updated_at
                        })).collect::<Vec<_>>()
                    }))),
                    Err(error) => {
                        ToolResult::fail(blocked("evidence_history_failed", error.to_string()))
                    }
                }
            }
            Some("get") => {
                let evidence_id = match required_string(args, "evidence_id") {
                    Ok(value) => value,
                    Err(error) => {
                        return ToolResult::fail(blocked("invalid_evidence_history_request", error))
                    }
                };
                match self
                    .store
                    .get_scientific_evidence(&self.project_id, &self.frame_id, &evidence_id)
                    .await
                {
                    Ok(Some(record)) => ToolResult::ok(pretty(json!({
                        "state":"evidence_recovered",
                        "evidence_id":record.evidence_id,
                        "provider":record.provider,
                        "provider_version":record.provider_version,
                        "tool_name":record.tool_name,
                        "arguments":serde_json::from_str::<Value>(&record.canonical_arguments_json).unwrap_or(Value::Null),
                        "evidence_state":record.evidence_state,
                        "semantics":serde_json::from_str::<Value>(&record.semantics_json).unwrap_or(Value::Null),
                        "provenance":serde_json::from_str::<Value>(&record.provenance_json).unwrap_or(Value::Null),
                        "payload":serde_json::from_str::<Value>(&record.compact_payload_json).unwrap_or(Value::Null),
                        "updated_at":record.updated_at
                    }))),
                    Ok(None) => ToolResult::fail(blocked(
                        "evidence_not_found",
                        "No evidence record with that id exists in this conversation.",
                    )),
                    Err(error) => {
                        ToolResult::fail(blocked("evidence_history_failed", error.to_string()))
                    }
                }
            }
            _ => ToolResult::fail(blocked(
                "invalid_evidence_history_request",
                "mode must be recent or get",
            )),
        }
    }
}

fn depmap_route_schema() -> Value {
    let intents = crate::depmap_capabilities::route_intents().unwrap_or_default();
    let phenotypes = crate::depmap_capabilities::concept_ids("phenotypes").unwrap_or_default();
    let molecular_focus =
        crate::depmap_capabilities::concept_ids("molecular_focus").unwrap_or_default();
    let mechanisms = crate::depmap_capabilities::concept_ids("mechanisms").unwrap_or_default();
    let evidence_sources =
        crate::depmap_capabilities::concept_ids("evidence_sources").unwrap_or_default();
    let requested_outputs =
        crate::depmap_capabilities::concept_ids("requested_outputs").unwrap_or_default();
    let execution_policies =
        crate::depmap_capabilities::concept_ids("execution_policies").unwrap_or_default();
    json!({
        "type":"object",
        "properties": {
            "intent": {
                "type":"string",
                "enum":intents,
                "description":"Classify the user's requested operation. Use cancer_direction_discovery or topic_exploration for a cancer-scoped request asking what topics or directions the existing data can support."
            },
            "gene": {"type":"string"},
            "cancer": {"type":"string"},
            "source_gene": {"type":"string"},
            "target_gene": {"type":"string"},
            "partner_gene": {"type":"string"},
            "drug": {"type":"string"},
            "contrast_id": {"type":"string"},
            "layer": {"type":"string"},
            "event": {"type":"string"},
            "family": {"type":"string"},
            "cohort": {"type":"string"},
            "contrast": {"type":"string"},
            "omic": {"type":"string"},
            "evidence_focus": {
                "type":"string",
                "enum":["all","transcription_factor","pathway","network","cnv","drug"],
                "description":"The scientific evidence family explicitly requested by the user. Choose transcription_factor for TF, transcriptional-regulator, or transcription-factor-focused questions. Choose all only when the user supplied no scientific focus."
            },
            "phenotypes": {
                "type":"array","items":{"type":"string","enum":phenotypes},"uniqueItems":true,"maxItems":6,
                "description":"Canonical biological phenotypes explicitly requested by the user. Preserve every supplied phenotype instead of compressing it into evidence_focus."
            },
            "molecular_focus": {
                "type":"array","items":{"type":"string","enum":molecular_focus},"uniqueItems":true,"maxItems":6,
                "description":"Canonical molecular layers explicitly requested by the user; for example transcription_factor."
            },
            "mechanisms": {
                "type":"array","items":{"type":"string","enum":mechanisms},"uniqueItems":true,"maxItems":6,
                "description":"Mechanistic relationships explicitly requested by the user. Do not infer a mechanism from a generic association request."
            },
            "evidence_sources": {
                "type":"array","items":{"type":"string","enum":evidence_sources},"uniqueItems":true,"maxItems":6
            },
            "requested_outputs": {
                "type":"array","items":{"type":"string","enum":requested_outputs},"uniqueItems":true,"maxItems":6
            },
            "execution_policy": {
                "type":"string","enum":execution_policies,
                "description":"Use precomputed_only unless the user explicitly authorizes proposing new statistical analysis. Durable execution still requires separate approval."
            },
            "coverage_status": {
                "type":"string",
                "enum":["FOUND","NOT_RETAINED","INELIGIBLE","NOT_COMPUTED","MODULE_UNAVAILABLE"],
                "description":"For new_analysis only: exact coverage state returned by the preceding validated evidence plan. Only NOT_COMPUTED can open the computation path."
            },
            "user_authorized_new_analysis": {
                "type":"boolean",
                "description":"For new_analysis only: true only when the user's current request explicitly asks to run a new analysis. This does not replace the separate Run or Workflow approval."
            },
            "unresolved_concepts": {
                "type":"array","items":{"type":"string","minLength":1,"maxLength":128},"uniqueItems":true,"maxItems":8,
                "description":"User-supplied scientific concepts that cannot be mapped to a declared canonical slot. Preserve them; never silently drop or guess them."
            },
            "explicit_workflow_request": {"type":"boolean"}
        },
        "required":["intent"],
        "additionalProperties":false
    })
}

fn non_empty_arg(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn string_array_arg(args: &Value, key: &str) -> Result<Vec<String>, String> {
    let Some(value) = args.get(key) else {
        return Ok(Vec::new());
    };
    let values = value
        .as_array()
        .ok_or_else(|| format!("{key} must be an array of strings"))?;
    let mut result = Vec::new();
    for value in values {
        let item = value
            .as_str()
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .ok_or_else(|| format!("{key} must contain only non-empty strings"))?;
        if !result.iter().any(|existing| existing == item) {
            result.push(item.to_string());
        }
    }
    Ok(result)
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

fn canonicalize_concept_values(
    category: &str,
    values: Vec<String>,
) -> Result<(Vec<String>, Vec<String>), String> {
    let mut canonical = Vec::new();
    let mut unresolved = Vec::new();
    for value in values {
        if let Some(id) = crate::depmap_capabilities::canonical_concept_id(category, &value)? {
            push_unique(&mut canonical, id);
        } else {
            push_unique(&mut unresolved, value);
        }
    }
    Ok((canonical, unresolved))
}

fn normalize_scientific_slots(
    phenotypes: Vec<String>,
    molecular_focus: Vec<String>,
    mechanisms: Vec<String>,
) -> Result<(Vec<String>, Vec<String>, Vec<String>, Vec<String>), String> {
    let mut normalized_phenotypes = Vec::new();
    let mut normalized_molecular = Vec::new();
    let mut normalized_mechanisms = Vec::new();
    let mut unresolved = Vec::new();
    for (declared_category, values) in [
        ("phenotypes", phenotypes),
        ("molecular_focus", molecular_focus),
        ("mechanisms", mechanisms),
    ] {
        for value in values {
            if let Some(id) =
                crate::depmap_capabilities::canonical_concept_id(declared_category, &value)?
            {
                match declared_category {
                    "phenotypes" => push_unique(&mut normalized_phenotypes, id),
                    "molecular_focus" => push_unique(&mut normalized_molecular, id),
                    "mechanisms" => push_unique(&mut normalized_mechanisms, id),
                    _ => unreachable!(),
                }
                continue;
            }
            let Some((actual_category, id)) =
                crate::depmap_capabilities::unique_concept_category(&value)?
            else {
                push_unique(&mut unresolved, value);
                continue;
            };
            match actual_category.as_str() {
                "phenotypes" => push_unique(&mut normalized_phenotypes, id),
                "molecular_focus" => push_unique(&mut normalized_molecular, id),
                "mechanisms" => push_unique(&mut normalized_mechanisms, id),
                _ => unreachable!(),
            }
        }
    }
    Ok((
        normalized_phenotypes,
        normalized_molecular,
        normalized_mechanisms,
        unresolved,
    ))
}

fn insert_binding(bindings: &mut BTreeMap<String, Value>, name: &str, value: Option<String>) {
    if let Some(value) = value {
        bindings.insert(name.to_string(), Value::String(value));
    }
}

fn legacy_focus_as_molecular_focus(focus: Option<&str>) -> Option<&'static str> {
    match focus {
        Some("transcription_factor") => Some("transcription_factor"),
        Some("pathway") => Some("pathway"),
        Some("network") => Some("dependency_network"),
        Some("cnv") => Some("copy_number"),
        Some("drug") => Some("drug_response"),
        _ => None,
    }
}

fn concept_contracts(category: &str, values: &[String]) -> Result<Vec<Value>, String> {
    values
        .iter()
        .map(|value| crate::depmap_capabilities::concept_contract(category, value))
        .collect()
}

fn normalized_unverified_entity(
    entity_type: &str,
    original: &str,
    label: &str,
    namespace: &str,
) -> Value {
    json!({
        "schema_version":"wisp.entity-resolution.v1",
        "entity_type":entity_type,
        "original_term":original,
        "status":"NORMALIZED_UNVERIFIED",
        "selected":null,
        "candidates":[{
            "canonical_id":format!("{namespace}:{label}"),
            "label":label,
            "namespace":namespace,
            "matched_by":"query_safe_normalization",
            "metadata":{}
        }],
        "requires_user_confirmation":false,
        "is_scientific_evidence":false,
        "note":"The installed knowledge catalog or bounded evidence query must verify existence before this candidate is treated as resolved."
    })
}

fn resolved_concept_entity(entity_type: &str, concept_id: &str, namespace: &str) -> Value {
    json!({
        "schema_version":"wisp.entity-resolution.v1",
        "entity_type":entity_type,
        "original_term":concept_id,
        "status":"RESOLVED",
        "selected":{
            "canonical_id":format!("{namespace}:{concept_id}"),
            "label":concept_id,
            "namespace":namespace,
            "matched_by":"capability_concept_registry",
            "metadata":{}
        },
        "candidates":[],
        "requires_user_confirmation":false,
        "is_scientific_evidence":false
    })
}

fn depmap_route(args: &Value) -> Result<Value, String> {
    let intent = required_string(args, "intent")?;
    let gene = non_empty_arg(args, "gene");
    let cancer = non_empty_arg(args, "cancer");
    let source_gene = non_empty_arg(args, "source_gene");
    let target_gene = non_empty_arg(args, "target_gene");
    let partner_gene = non_empty_arg(args, "partner_gene");
    let drug = non_empty_arg(args, "drug");
    let contrast_id = non_empty_arg(args, "contrast_id");
    let layer = non_empty_arg(args, "layer");
    let event = non_empty_arg(args, "event");
    let family = non_empty_arg(args, "family");
    let cohort = non_empty_arg(args, "cohort");
    let contrast = non_empty_arg(args, "contrast");
    let omic = non_empty_arg(args, "omic");
    let evidence_focus = non_empty_arg(args, "evidence_focus");
    let phenotypes = string_array_arg(args, "phenotypes")?;
    let molecular_focus = string_array_arg(args, "molecular_focus")?;
    let mechanisms = string_array_arg(args, "mechanisms")?;
    let (phenotypes, mut molecular_focus, mechanisms, inferred_unresolved) =
        normalize_scientific_slots(phenotypes, molecular_focus, mechanisms)?;
    if molecular_focus.is_empty() {
        if let Some(legacy) = legacy_focus_as_molecular_focus(evidence_focus.as_deref()) {
            molecular_focus.push(legacy.to_string());
        }
    }
    let (mut evidence_sources, source_unresolved) = canonicalize_concept_values(
        "evidence_sources",
        string_array_arg(args, "evidence_sources")?,
    )?;
    let (mut requested_outputs, output_unresolved) = canonicalize_concept_values(
        "requested_outputs",
        string_array_arg(args, "requested_outputs")?,
    )?;
    let mut unresolved_concepts = string_array_arg(args, "unresolved_concepts")?;
    inferred_unresolved
        .into_iter()
        .chain(source_unresolved)
        .chain(output_unresolved)
        .for_each(|value| push_unique(&mut unresolved_concepts, value));
    let topic_request = matches!(
        intent.as_str(),
        "cancer_direction_discovery" | "topic_exploration"
    );
    if topic_request && evidence_sources.is_empty() {
        evidence_sources.push("depmap".into());
    }
    if topic_request && requested_outputs.is_empty() {
        requested_outputs.push("candidate_topics".into());
    }
    let supplied_execution_policy = non_empty_arg(args, "execution_policy");
    let execution_policy = match supplied_execution_policy.as_deref() {
        Some(value) => {
            crate::depmap_capabilities::canonical_concept_id("execution_policies", value)?
                .unwrap_or_else(|| {
                    push_unique(&mut unresolved_concepts, value.to_string());
                    // Unknown provider wording must fail toward the read-only policy;
                    // it must never accidentally authorize new analysis.
                    "precomputed_only".into()
                })
        }
        None => "precomputed_only".into(),
    };
    let explicit_workflow = args
        .get("explicit_workflow_request")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let coverage_status = non_empty_arg(args, "coverage_status");
    let user_authorized_new_analysis = args
        .get("user_authorized_new_analysis")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let canonical_lineage = cancer.as_deref().and_then(recognized_canonical_lineage);
    let mut bindings: BTreeMap<String, Value> = BTreeMap::new();
    for (name, value) in [
        ("gene", gene.as_ref()),
        ("cancer", cancer.as_ref()),
        ("canonical_lineage", canonical_lineage.as_ref()),
        ("source_gene", source_gene.as_ref()),
        ("target_gene", target_gene.as_ref()),
        ("partner_gene", partner_gene.as_ref()),
        ("drug", drug.as_ref()),
        ("contrast_id", contrast_id.as_ref()),
        ("layer", layer.as_ref()),
        ("event", event.as_ref()),
        ("family", family.as_ref()),
        ("cohort", cohort.as_ref()),
        ("contrast", contrast.as_ref()),
        ("omic", omic.as_ref()),
        ("evidence_focus", evidence_focus.as_ref()),
    ] {
        insert_binding(&mut bindings, name, value.cloned());
    }
    for (name, values) in [
        ("phenotypes", &phenotypes),
        ("molecular_focus", &molecular_focus),
        ("mechanisms", &mechanisms),
        ("evidence_sources", &evidence_sources),
        ("requested_outputs", &requested_outputs),
        ("unresolved_concepts", &unresolved_concepts),
    ] {
        if !values.is_empty() {
            bindings.insert(name.into(), json!(values));
        }
    }
    bindings.insert("execution_policy".into(), json!(execution_policy));
    let route = crate::depmap_capabilities::resolve_route(&intent, &bindings)?;
    let mut missing = crate::depmap_capabilities::missing_fields(route, &bindings);
    if cancer.is_some()
        && canonical_lineage.is_none()
        && !missing.iter().any(|field| field == "canonical_lineage")
    {
        missing.push("canonical_lineage".into());
    }
    let new_analysis_gate = intent == "new_analysis";
    if new_analysis_gate {
        if execution_policy != "allow_new_analysis" {
            missing.push("execution_policy=allow_new_analysis".into());
        }
        if coverage_status.as_deref() != Some("NOT_COMPUTED") {
            missing.push("validated_coverage_status=NOT_COMPUTED".into());
        }
        if !user_authorized_new_analysis {
            missing.push("explicit_user_authorization".into());
        }
    }
    let resolution_query =
        crate::depmap_capabilities::entity_resolution_query(&missing, &bindings)?;
    let needs_resolution = resolution_query.is_some();
    let mut execution_level = route.execution_level.clone();
    let mut approval = route.requires_approval;
    let mut strategy = route.strategy.clone();
    let mut tools = route.allowed_next_tools.clone();

    let requires_user_input = !missing.is_empty() && !needs_resolution;
    if needs_resolution {
        execution_level = "L1_DIRECT".into();
        approval = false;
        strategy = "Resolve the supplied cancer term against the maintained lineage vocabulary before any scientific query.".into();
        tools = vec!["depmap_resolve_entity".into()];
    } else if explicit_workflow && !requires_user_input {
        execution_level = "L4_DURABLE".into();
        approval = true;
        strategy = "The user explicitly requested a registered Workflow; create only its approval-gated draft.".into();
        tools = vec!["start_workflow".into()];
    }

    let recommended_query = if needs_resolution {
        resolution_query.unwrap_or(Value::Null)
    } else if requires_user_input || explicit_workflow {
        Value::Null
    } else {
        crate::depmap_capabilities::recommended_query(route, &bindings)?
    };
    let query_contract = recommended_query
        .pointer("/arguments/mode")
        .and_then(Value::as_str)
        .map(crate::depmap_capabilities::query_contract)
        .transpose()?
        .or_else(|| {
            recommended_query
                .get("tool")
                .and_then(Value::as_str)
                .and_then(|tool| crate::depmap_capabilities::tool_contract(tool).ok())
        })
        .unwrap_or(Value::Null);
    let candidate_query_contracts = if needs_resolution || explicit_workflow {
        json!([])
    } else {
        crate::depmap_capabilities::candidate_contracts(route)?
    };
    let phenotype_contracts = concept_contracts("phenotypes", &phenotypes)?;
    let molecular_focus_contracts = concept_contracts("molecular_focus", &molecular_focus)?;
    let mechanism_contracts = concept_contracts("mechanisms", &mechanisms)?;
    let relation_predicate = mechanism_contracts
        .iter()
        .find_map(|contract| contract["relation_predicate"].as_str())
        .unwrap_or("candidate_association_with")
        .to_string();
    let phenotype_hints =
        crate::depmap_capabilities::concept_routing_hints("phenotypes", &phenotypes)?;
    let molecular_hints =
        crate::depmap_capabilities::concept_routing_hints("molecular_focus", &molecular_focus)?;
    let mut routing_question_tags = Vec::new();
    let mut routing_entity_sets = Vec::new();
    for hints in [&phenotype_hints, &molecular_hints] {
        for value in hints["question_tags"].as_array().into_iter().flatten() {
            if let Some(value) = value.as_str() {
                push_unique(&mut routing_question_tags, value.to_string());
            }
        }
        for value in hints["entity_sets"].as_array().into_iter().flatten() {
            if let Some(value) = value.as_str() {
                push_unique(&mut routing_entity_sets, value.to_string());
            }
        }
    }
    let research_relations: Vec<Value> = phenotypes
        .iter()
        .flat_map(|phenotype| {
            let relation_predicate = relation_predicate.clone();
            molecular_focus.iter().map(move |focus| {
                json!({
                    "subject": {"slot":"molecular_focus", "id":focus},
                "predicate": relation_predicate,
                    "object": {"slot":"phenotype", "id":phenotype},
                    "evidence_requirement": "direct_result_or_declared_proxy"
                })
            })
        })
        .collect();
    let routing_hints = json!({
        "question_tags": routing_question_tags,
        "entity_sets": routing_entity_sets
    });
    let mut entity_resolutions = Vec::new();
    if let (Some(original), Some(canonical)) = (&cancer, &canonical_lineage) {
        entity_resolutions.push(crate::depmap_entities::resolved_cancer_entity(
            original, canonical,
        ));
    }
    if let Some(gene) = &gene {
        entity_resolutions.push(normalized_unverified_entity(
            "gene",
            gene,
            &gene.to_uppercase(),
            "hgnc-symbol-candidate",
        ));
    }
    for (entity_type, namespace, values) in [
        ("phenotype", "wisp-phenotype", &phenotypes),
        ("molecular_focus", "wisp-molecular-focus", &molecular_focus),
        ("mechanism", "wisp-mechanism", &mechanisms),
        ("evidence_source", "wisp-evidence-source", &evidence_sources),
        (
            "requested_output",
            "wisp-requested-output",
            &requested_outputs,
        ),
    ] {
        for value in values {
            entity_resolutions.push(resolved_concept_entity(entity_type, value, namespace));
        }
    }
    Ok(json!({
        "state": if needs_resolution { "needs_resolution" } else if requires_user_input { "needs_input" } else { "routed" },
        "intent": intent,
        "execution_level": execution_level,
        "requires_user_input": requires_user_input,
        "missing_fields": missing,
        "requires_approval": approval,
        "explicit_workflow_request": explicit_workflow,
        "entities": {
            "gene": gene,
            "cancer_term": cancer,
            "canonical_lineage": canonical_lineage,
            "source_gene": source_gene,
            "target_gene": target_gene,
            "partner_gene": partner_gene,
            "drug": drug,
            "contrast_id": contrast_id,
            "layer": layer,
            "event": event,
            "family": family,
            "cohort": cohort,
            "contrast": contrast,
            "omic": omic
            ,"evidence_focus": evidence_focus
        },
        "semantic_slots": {
            "disease": {"raw": cancer, "canonical_lineage": canonical_lineage},
            "phenotypes": phenotypes,
            "molecular_focus": molecular_focus,
            "research_relations": research_relations,
            "mechanisms": mechanisms,
            "evidence_sources": evidence_sources,
            "requested_outputs": requested_outputs,
            "execution_policy": execution_policy,
            "coverage_status": coverage_status,
            "user_authorized_new_analysis": user_authorized_new_analysis,
            "unresolved_concepts": unresolved_concepts
        },
        "scientific_intent": {
            "schema_version": "wisp.scientific-intent.v1",
            "task_type": intent,
            "disease_mentions": cancer.as_ref().map(|value| vec![json!({"text":value})]).unwrap_or_default(),
            "gene_mentions": gene.as_ref().map(|value| vec![json!({"text":value,"normalized_candidate":value.to_uppercase(),"resolution_status":"NORMALIZED_UNVERIFIED"})]).unwrap_or_default(),
            "phenotypes": phenotypes,
            "molecular_focus": molecular_focus,
            "research_relations": research_relations,
            "mechanisms": mechanisms,
            "evidence_sources": evidence_sources,
            "requested_outputs": requested_outputs,
            "execution_policy": execution_policy,
            "coverage_status": coverage_status,
            "user_authorized_new_analysis": user_authorized_new_analysis,
            "unresolved_concepts": unresolved_concepts,
            "resolved_entities": {
                "depmap_lineage": canonical_lineage
            },
            "entity_resolutions": entity_resolutions,
            "entity_registry": crate::depmap_entities::registry_summary()?,
            "routing_hints": routing_hints
        },
        "semantic_contracts": {
            "phenotypes": phenotype_contracts,
            "molecular_focus": molecular_focus_contracts,
            "mechanisms": mechanism_contracts
        },
        "strategy": strategy,
        "recommended_query": recommended_query,
        "query_contract": query_contract,
        "candidate_query_contracts": candidate_query_contracts,
        "allowed_next_tools": tools,
        "capability_registry": crate::depmap_capabilities::registry_summary(&route.id)?,
        "guardrails": {
            "route_is_evidence": false,
            "workflow_semantic_match_alone_is_sufficient": false,
            "do_not_invent_missing_entities": true,
            "new_analysis_gate": {
                "required_coverage_status":"NOT_COMPUTED",
                "requires_explicit_user_authorization":true,
                "requires_execution_policy":"allow_new_analysis",
                "requires_separate_run_or_workflow_approval":true,
                "passed": !new_analysis_gate || missing.is_empty()
            }
        }
    }))
}

#[async_trait::async_trait]
impl Tool for DepMapAgentRouteTool {
    fn name(&self) -> &str {
        ROUTE_TOOL_NAME
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            ROUTE_TOOL_NAME,
            "Classify one new DepMap request into a host-validated execution level and a capability-registry-backed query contract. Supply only the intent and entities extracted from the user's request. Use once per new request, not for a follow-up that only interprets the current tool result. Execute its recommended_query when present and treat the routing record as control data, never scientific evidence.",
            depmap_route_schema(),
        )
    }

    fn read_only(&self) -> bool {
        true
    }

    fn preview(&self, args: &Value) -> String {
        args.get("intent")
            .and_then(Value::as_str)
            .unwrap_or("DepMap request")
            .to_string()
    }

    async fn run(&self, args: &Value, _env: &dyn ToolEnv) -> ToolResult {
        match depmap_route(args) {
            Ok(route) => {
                let mut allowed = route["allowed_next_tools"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                // Resolution may require one re-route, and every scoped turn
                // must retain safe ways to ask or finish without falling back
                // to shell/filesystem tools.
                if route["state"] == "needs_resolution" {
                    allowed.push(ROUTE_TOOL_NAME.to_string());
                }
                allowed.extend(["ask_user".to_string(), "attempt_completion".to_string()]);
                allowed.sort();
                allowed.dedup();
                let success = route["state"] == "routed";
                let required_call = (route["recommended_query"]["single_call"] == true)
                    .then(|| {
                        Some((
                            route["recommended_query"]["tool"].as_str()?.to_string(),
                            route["recommended_query"].get("arguments")?.clone(),
                        ))
                    })
                    .flatten();
                let content = pretty(route);
                let mut result = if success {
                    ToolResult::ok(content).restrict_next_tools(allowed)
                } else {
                    ToolResult::fail(content).restrict_next_tools(allowed)
                };
                if let Some((tool, arguments)) = required_call {
                    result = result.require_next_tool_call(tool, arguments);
                }
                result
            }
            Err(error) => ToolResult::fail(blocked("invalid_agent_route", error)),
        }
    }
}

pub(crate) struct DepMapProjectRunsTool {
    store: wisp_store::Store,
    scope: wisp_store::StateScope,
}

impl DepMapProjectRunsTool {
    pub(crate) fn new(store: wisp_store::Store, scope: wisp_store::StateScope) -> Self {
        Self { store, scope }
    }
}

#[async_trait::async_trait]
impl Tool for DepMapProjectRunsTool {
    fn name(&self) -> &str {
        PROJECT_RUNS_TOOL_NAME
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            PROJECT_RUNS_TOOL_NAME,
            "Recover compact persisted Run identities only when the user asks to continue, inspect, validate, or create computation. Do not call this for query-only evidence or topic inventory. Use get_run or monitor_run with a returned id for detail. This tool never starts a Run.",
            json!({
                "type": "object",
                "properties": {
                    "status": {
                        "type": "string",
                        "enum": ["draft","submitted","running","paused","cancelling","succeeded","failed","cancelled","timed_out","lost"]
                    },
                    "title_contains": {"type":"string"},
                    "limit": {"type":"integer","minimum":1,"maximum":50}
                },
                "additionalProperties": false
            }),
        )
    }

    fn read_only(&self) -> bool {
        true
    }

    fn preview(&self, args: &Value) -> String {
        args.get("title_contains")
            .and_then(Value::as_str)
            .unwrap_or("recent project Runs")
            .to_string()
    }

    async fn run(&self, args: &Value, _env: &dyn ToolEnv) -> ToolResult {
        let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(20);
        if !(1..=50).contains(&limit) {
            return ToolResult::fail(blocked("invalid_limit", "limit must be between 1 and 50"));
        }
        let status = args.get("status").and_then(Value::as_str);
        let title = args
            .get("title_contains")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let runs = match self.store.list_run_summaries_in_scope(&self.scope).await {
            Ok(runs) => filter_run_summaries(runs, status, title, limit as usize),
            Err(error) => return ToolResult::fail(blocked("run_index_failed", error.to_string())),
        };
        let runs = runs
            .into_iter()
            .map(|run| {
                json!({
                    "id": run.id,
                    "context_id": run.context_id,
                    "title": run.title,
                    "kind": run.kind,
                    "status": run.status.as_str(),
                    "created_at": run.created_at,
                    "started_at": run.started_at,
                    "ended_at": run.ended_at,
                    "exit_code": run.exit_code,
                    "harvested_at": run.harvested_at
                })
            })
            .collect::<Vec<_>>();
        ToolResult::ok(pretty(json!({
            "state": "project_cycle",
            "scope": self.scope,
            "runs": runs,
            "next": "Reuse a returned Run id with get_run or monitor_run. Do not resubmit an equivalent active or succeeded Run."
        })))
    }
}

fn filter_run_summaries(
    runs: Vec<wisp_store::RunSummary>,
    status: Option<&str>,
    title_contains: Option<&str>,
    limit: usize,
) -> Vec<wisp_store::RunSummary> {
    let title_contains = title_contains.map(str::to_ascii_lowercase);
    runs.into_iter()
        .filter(|run| status.is_none_or(|status| run.status.as_str() == status))
        .filter(|run| {
            title_contains
                .as_ref()
                .is_none_or(|query| run.title.to_ascii_lowercase().contains(query))
        })
        .take(limit)
        .collect()
}

pub(crate) struct DepMapValidateRunTool {
    store: wisp_store::Store,
    scope: wisp_store::StateScope,
    project_root: PathBuf,
    project_id: String,
    frame_id: String,
}

impl DepMapValidateRunTool {
    pub(crate) fn new(
        store: wisp_store::Store,
        scope: wisp_store::StateScope,
        project_root: PathBuf,
        project_id: String,
        frame_id: String,
    ) -> Self {
        Self {
            store,
            scope,
            project_root,
            project_id,
            frame_id,
        }
    }
}

async fn persist_validated_run_evidence(
    store: &wisp_store::Store,
    project_id: &str,
    frame_id: &str,
    run_id: &str,
    run_dir: &str,
    response: &Value,
) -> Result<wisp_store::ScientificEvidenceRecord, String> {
    let arguments = json!({"run_id":run_id,"run_dir":run_dir});
    let semantics = json!({
        "evidence_kind":"validated_run",
        "claim_scope":"validated_documents.result",
        "qa_required":true
    });
    let provenance = json!({
        "source":"wisp_run_manager",
        "run_id":run_id,
        "run_dir":run_dir,
        "artifacts":response.get("artifacts").cloned().unwrap_or_else(|| json!([]))
    });
    let compact_payload = compact_ledger_payload(response);
    store
        .upsert_scientific_evidence(wisp_store::NewScientificEvidence {
            project_id,
            frame_id,
            provider: "wisp_validated_run",
            provider_version: response["dataset_release"].as_str(),
            tool_name: VALIDATE_RUN_TOOL_NAME,
            arguments: &arguments,
            evidence_state: "run_validated",
            semantics: &semantics,
            provenance: &provenance,
            compact_payload: &compact_payload,
        })
        .await
        .map_err(|error| error.to_string())
}

#[async_trait::async_trait]
impl Tool for DepMapValidateRunTool {
    fn name(&self) -> &str {
        VALIDATE_RUN_TOOL_NAME
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            VALIDATE_RUN_TOOL_NAME,
            "Validate one completed DepMap R Run before scientific interpretation. Requires a succeeded persisted Run plus project-relative run_manifest.json, result.json, and qc.json in the declared run directory. Returns run_validated only when execution, provenance, result, and QC gates pass.",
            json!({
                "type":"object",
                "properties": {
                    "run_id": {"type":"string"},
                    "run_dir": {
                        "type":"string",
                        "description":"Project-relative directory containing run_manifest.json, result.json, and qc.json"
                    }
                },
                "required":["run_id","run_dir"],
                "additionalProperties": false
            }),
        )
    }

    fn read_only(&self) -> bool {
        true
    }

    fn preview(&self, args: &Value) -> String {
        args.get("run_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    }

    async fn run(&self, args: &Value, _env: &dyn ToolEnv) -> ToolResult {
        let run_id = match required_string(args, "run_id") {
            Ok(run_id) => run_id,
            Err(error) => return ToolResult::fail(blocked("invalid_validation", error)),
        };
        let run_dir = match required_string(args, "run_dir") {
            Ok(run_dir) => run_dir,
            Err(error) => return ToolResult::fail(blocked("invalid_validation", error)),
        };
        if Path::new(&run_dir).is_absolute() {
            return ToolResult::fail(blocked(
                "invalid_run_dir",
                "run_dir must be project-relative",
            ));
        }
        if let Err(error) = wisp_tools::safety::validate_relative_pattern(&run_dir) {
            return ToolResult::fail(blocked("invalid_run_dir", error));
        }
        match self.store.run_visible_in_scope(&run_id, &self.scope).await {
            Ok(true) => {}
            Ok(false) => {
                return ToolResult::fail(blocked(
                    "run_not_visible",
                    "Run does not belong to this project state scope",
                ))
            }
            Err(error) => return ToolResult::fail(blocked("run_lookup_failed", error.to_string())),
        }
        let run = match self.store.get_run(&run_id).await {
            Ok(Some(run)) => run,
            Ok(None) => return ToolResult::fail(blocked("run_not_found", "Run not found")),
            Err(error) => return ToolResult::fail(blocked("run_lookup_failed", error.to_string())),
        };
        let directory = match wisp_tools::safety::resolve_under_root(&self.project_root, &run_dir) {
            Ok(directory) if directory.is_dir() => directory,
            Ok(_) => {
                return ToolResult::fail(blocked("run_dir_missing", "run_dir is not a directory"))
            }
            Err(error) => return ToolResult::fail(blocked("run_dir_missing", error)),
        };
        let manifest = match read_bounded_json(&directory.join("run_manifest.json")).await {
            Ok(value) => value,
            Err(error) => return ToolResult::fail(blocked("manifest_invalid", error)),
        };
        let result = match read_bounded_json(&directory.join("result.json")).await {
            Ok(value) => value,
            Err(error) => return ToolResult::fail(blocked("result_invalid", error)),
        };
        let qc = match read_bounded_json(&directory.join("qc.json")).await {
            Ok(value) => value,
            Err(error) => return ToolResult::fail(blocked("qc_invalid", error)),
        };
        let (checks, mut failures, warnings) = validate_result_documents(&manifest, &result, &qc);
        if run.status != wisp_store::RunStatus::Succeeded || run.exit_code != Some(0) {
            failures.push(json!({
                "name":"persisted_run_succeeded",
                "detail": format!("status={}, exit_code={:?}", run.status.as_str(), run.exit_code)
            }));
        }
        let required_outputs = ["run_manifest.json", "result.json", "qc.json"];
        let declared: Vec<crate::harvest::OutputSpec> =
            serde_json::from_str(&run.output_specs_json).unwrap_or_default();
        for filename in required_outputs {
            let relative = Path::new(&run_dir)
                .join(filename)
                .to_string_lossy()
                .replace('\\', "/");
            if !declared.iter().any(|spec| {
                glob::Pattern::new(&spec.glob).is_ok_and(|pattern| pattern.matches(&relative))
            }) {
                failures.push(json!({
                    "name":"declared_output",
                    "detail": format!("Run output_specs did not declare {relative}")
                }));
            }
        }
        let artifacts = self
            .store
            .list_run_outputs(&run_id)
            .await
            .unwrap_or_default();
        let mut response = json!({
            "state": if failures.is_empty() { "run_validated" } else { "validation_failed" },
            "run_id": run_id,
            "run_dir": run_dir,
            "dataset_release": manifest.get("dataset_release"),
            "checks": checks,
            "blocking_failures": failures,
            "warnings": warnings,
            "artifacts": artifacts,
            "validated_documents": {
                "manifest": manifest,
                "result": result,
                "qc": qc
            }
        });
        if response["state"] == "run_validated" {
            let record = match persist_validated_run_evidence(
                &self.store,
                &self.project_id,
                &self.frame_id,
                &run_id,
                &run_dir,
                &response,
            )
            .await
            {
                Ok(record) => record,
                Err(error) => {
                    return ToolResult::fail(blocked(
                        "evidence_ledger_failed",
                        format!(
                            "Run passed QA but its evidence record could not be persisted: {error}"
                        ),
                    ))
                }
            };
            response["evidence_ref"] = json!({
                "evidence_id":record.evidence_id,
                "provider":record.provider,
                "provider_version":record.provider_version,
                "evidence_state":record.evidence_state,
                "result_pointer":"/validated_documents/result"
            });
            ToolResult::ok(pretty(response))
        } else {
            ToolResult::fail(pretty(response))
        }
    }
}

fn validate_result_documents(
    manifest: &Value,
    result: &Value,
    qc: &Value,
) -> (Vec<Value>, Vec<Value>, Vec<Value>) {
    let mut checks = Vec::new();
    let mut failures = Vec::new();
    let mut warnings = Vec::new();
    let mut required = |name: &str, pass: bool, detail: String| {
        checks.push(json!({
            "name": name,
            "status": if pass { "pass" } else { "fail" },
            "detail": detail
        }));
        if !pass {
            failures.push(json!({"name":name,"detail":detail}));
        }
    };
    required(
        "manifest_schema",
        manifest.get("schema_version").and_then(Value::as_i64) == Some(1),
        "run_manifest schema_version must be 1".into(),
    );
    required(
        "language_is_r",
        manifest.get("language").and_then(Value::as_str) == Some("R"),
        "run_manifest language must be R".into(),
    );
    required(
        "analysis_identity",
        nonempty_json_string(manifest, "analysis_id"),
        "run_manifest analysis_id is required".into(),
    );
    required(
        "release_identified",
        nonempty_json_string(manifest, "dataset_release"),
        "run_manifest dataset_release is required".into(),
    );
    required(
        "result_schema",
        result.get("schema_version").and_then(Value::as_i64) == Some(1),
        "result schema_version must be 1".into(),
    );
    required(
        "result_status",
        result.get("status").and_then(Value::as_str) == Some("ok"),
        "result status must be ok".into(),
    );
    required(
        "target_resolved",
        result
            .get("targets")
            .and_then(Value::as_array)
            .is_some_and(|targets| !targets.is_empty()),
        "result targets must contain at least one resolved target".into(),
    );
    let n_before = result.pointer("/cohort/n_before").and_then(Value::as_u64);
    let n_after = result.pointer("/cohort/n_after").and_then(Value::as_u64);
    required(
        "cohort_counts",
        matches!((n_before, n_after), (Some(before), Some(after)) if after <= before),
        "cohort n_before/n_after must be non-negative and n_after <= n_before".into(),
    );
    required(
        "qc_schema",
        qc.get("schema_version").and_then(Value::as_i64) == Some(1),
        "qc schema_version must be 1".into(),
    );
    required(
        "qc_status",
        qc.get("status").and_then(Value::as_str) == Some("pass"),
        "qc status must be pass".into(),
    );
    required(
        "qc_blocking_failures_empty",
        qc.get("blocking_failures")
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty),
        "qc blocking_failures must be an empty array".into(),
    );
    if let Some(observations) = result.get("observations").and_then(Value::as_array) {
        for (index, observation) in observations.iter().enumerate() {
            if !nonempty_json_string(observation, "direction") {
                warnings.push(json!({
                    "name":"observation_direction_missing",
                    "detail":format!("observations[{index}] has no explicit direction semantics")
                }));
            }
        }
    }
    (checks, failures, warnings)
}

fn nonempty_json_string(value: &Value, key: &str) -> bool {
    value
        .get(key)
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
}

async fn read_bounded_json(path: &Path) -> Result<Value, String> {
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if metadata.len() > MAX_VALIDATION_JSON_BYTES {
        return Err(format!(
            "{} exceeds the {} byte validation limit",
            path.display(),
            MAX_VALIDATION_JSON_BYTES
        ));
    }
    read_json_file(path).await
}

impl DepMapQueryTool {
    pub(crate) fn from_project(
        project_root: PathBuf,
        skills: &wisp_skills::SkillIndex,
        store: wisp_store::Store,
        project_id: String,
        frame_id: String,
    ) -> Option<Self> {
        let skill = skills.get(SKILL_NAME)?;
        Some(Self {
            project_root,
            query_script: skill.dir.join("scripts").join("query_depmap_kb.R"),
            store,
            project_id,
            frame_id,
        })
    }

    pub(crate) fn evidence_tool(&self) -> DepMapEvidenceTool {
        DepMapEvidenceTool {
            query: self.clone(),
        }
    }

    async fn workspace(&self) -> Result<KnowledgeWorkspace, String> {
        resolve_workspace(&self.project_root).await
    }

    async fn persist_scientific_result(
        &self,
        tool_name: &str,
        args: &Value,
        result: ToolResult,
    ) -> ToolResult {
        if !result.success {
            return result;
        }
        let mut parsed = match serde_json::from_str::<Value>(&result.content) {
            Ok(Value::Object(parsed)) => Value::Object(parsed),
            _ => return result,
        };
        let provider = parsed
            .get("provider")
            .and_then(Value::as_str)
            .unwrap_or("depmap");
        let provider_version = parsed.get("release").and_then(Value::as_str);
        let evidence_state = parsed
            .get("state")
            .and_then(Value::as_str)
            .unwrap_or("precomputed_query");
        let semantics = parsed
            .get("semantics")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let provenance = parsed
            .get("provenance")
            .or_else(|| parsed.pointer("/result/provenance"))
            .cloned()
            .unwrap_or_else(|| {
                json!({
                    "source":"precomputed_depmap_knowledge",
                    "tool":tool_name
                })
            });
        let ledger_arguments = parsed.get("query").unwrap_or(args);
        let compact_payload = compact_ledger_payload(&parsed);
        let record = match self
            .store
            .upsert_scientific_evidence(wisp_store::NewScientificEvidence {
                project_id: &self.project_id,
                frame_id: &self.frame_id,
                provider,
                provider_version,
                tool_name,
                arguments: ledger_arguments,
                evidence_state,
                semantics: &semantics,
                provenance: &provenance,
                compact_payload: &compact_payload,
            })
            .await
        {
            Ok(record) => record,
            Err(error) => {
                return ToolResult::fail(blocked(
                    "evidence_ledger_failed",
                    format!(
                    "The query succeeded but its evidence record could not be persisted: {error}"
                ),
                ))
            }
        };
        parsed["evidence_ref"] = json!({
            "evidence_id": record.evidence_id,
            "ledger_record_id": record.id,
            "project_id": record.project_id,
            "frame_id": record.frame_id,
            "provider": record.provider,
            "provider_version": record.provider_version,
            "tool_name": record.tool_name,
            "evidence_state": record.evidence_state
        });
        ToolResult::ok(pretty(parsed))
    }

    async fn run_status(&self, workspace: &KnowledgeWorkspace) -> ToolResult {
        match &workspace.provider {
            KnowledgeProvider::Local { root } => {
                let qa_path = root.join("depmap-26q1-qa.json");
                let qa = match read_json_file(&qa_path).await {
                    Ok(qa) => qa,
                    Err(error) => return ToolResult::fail(blocked("knowledge_qa_missing", error)),
                };
                if !qa
                    .get("qa_status")
                    .and_then(Value::as_str)
                    .is_some_and(|status| status.eq_ignore_ascii_case("PASS"))
                {
                    return ToolResult::fail(blocked(
                        "knowledge_qa_not_pass",
                        format!("{} does not report qa_status=PASS", qa_path.display()),
                    ));
                }
                ToolResult::ok(pretty(json!({
                    "state": "provider_ready",
                    "provider": "local",
                    "release": workspace.release,
                    "knowledge_root": root,
                    "qa": qa,
                    "next": "Use depmap_query with a bounded query mode."
                })))
            }
            KnowledgeProvider::Remote { endpoint } => {
                let health_url = match endpoint_url(endpoint, "health") {
                    Ok(url) => url,
                    Err(error) => {
                        return ToolResult::fail(blocked("invalid_remote_endpoint", error))
                    }
                };
                match remote_json(reqwest::Method::GET, health_url, None, depmap_api_token()).await
                {
                    Ok(health) => ToolResult::ok(pretty(json!({
                        "state": "provider_ready",
                        "provider": "remote",
                        "transport": "configured_endpoint",
                        "release": workspace.release,
                        "endpoint": endpoint,
                        "health": health,
                        "next": "Use depmap_query with a bounded query mode."
                    }))),
                    Err(error) => ToolResult::fail(blocked("remote_health_failed", error)),
                }
            }
        }
    }

    async fn run_query(&self, workspace: &KnowledgeWorkspace, args: &Value) -> ToolResult {
        let query = match validated_query(args) {
            Ok(query) => query,
            Err(error) => return ToolResult::fail(blocked("invalid_query", error)),
        };
        let result = match &workspace.provider {
            KnowledgeProvider::Local { root } => self.run_local(root, workspace, &query).await,
            KnowledgeProvider::Remote { endpoint } => {
                self.run_remote(endpoint, workspace, &query).await
            }
        };
        attach_capability_contract(&query, result)
    }

    async fn run_local(
        &self,
        root: &Path,
        workspace: &KnowledgeWorkspace,
        query: &Value,
    ) -> ToolResult {
        if !self.query_script.is_file() {
            return ToolResult::fail(blocked(
                "query_tool_missing",
                format!("{} is missing", self.query_script.display()),
            ));
        }
        let rscript = match which::which("Rscript") {
            Ok(path) => path,
            Err(error) => {
                return ToolResult::fail(blocked(
                    "rscript_missing",
                    format!("Rscript is required for the local provider: {error}"),
                ))
            }
        };
        let mut command = Command::new(rscript);
        command
            .arg(&self.query_script)
            .arg("--kb-root")
            .arg(root)
            .current_dir(&self.project_root)
            .kill_on_drop(true);
        for (key, value) in query.as_object().expect("validated query is an object") {
            let value = match value {
                Value::String(value) => value.clone(),
                Value::Number(value) => value.to_string(),
                Value::Bool(value) => value.to_string(),
                _ => continue,
            };
            command
                .arg(format!("--{}", key.replace('_', "-")))
                .arg(value);
        }
        let output = match tokio::time::timeout(Duration::from_secs(45), command.output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(error)) => {
                return ToolResult::fail(blocked("local_query_failed", error.to_string()))
            }
            Err(_) => {
                return ToolResult::fail(blocked(
                    "local_query_timeout",
                    "The bounded knowledge query exceeded 45 seconds.",
                ))
            }
        };
        if !output.status.success() {
            let start = output.stderr.len().saturating_sub(8 * 1024);
            let stderr = String::from_utf8_lossy(&output.stderr[start..])
                .trim()
                .to_string();
            return ToolResult::ok(pretty(json!({
                "state": "coverage_gap",
                "provider": "local",
                "release": workspace.release,
                "query": query,
                "reason": if stderr.is_empty() { "local query returned no matching result" } else { &stderr },
                "new_analysis_started": false
            })));
        }
        if output.stdout.len() > MAX_REMOTE_RESPONSE_BYTES {
            return ToolResult::fail(blocked(
                "local_response_too_large",
                format!(
                    "local query response exceeds {} bytes",
                    MAX_REMOTE_RESPONSE_BYTES
                ),
            ));
        }
        let result = match serde_json::from_slice::<Value>(&output.stdout) {
            Ok(result) => result,
            Err(error) => {
                return ToolResult::fail(blocked(
                    "invalid_local_response",
                    format!("query helper did not return JSON: {error}"),
                ))
            }
        };
        ToolResult::ok(pretty(json!({
            "state": classify_result_state(&result),
            "provider": "local",
            "release": workspace.release,
            "query": query,
            "semantics": query_semantics(query),
            "result": result,
            "new_analysis_started": false
        })))
    }

    async fn run_remote(
        &self,
        endpoint: &Url,
        workspace: &KnowledgeWorkspace,
        query: &Value,
    ) -> ToolResult {
        let query_url = match endpoint_url(endpoint, "query") {
            Ok(url) => url,
            Err(error) => return ToolResult::fail(blocked("invalid_remote_endpoint", error)),
        };
        match remote_json(
            reqwest::Method::POST,
            query_url,
            Some(query.clone()),
            depmap_api_token(),
        )
        .await
        {
            Ok(result) => ToolResult::ok(pretty(json!({
                "state": classify_result_state(&result),
                "provider": "remote",
                "release": workspace.release,
                "query": query,
                "semantics": query_semantics(query),
                "result": result,
                "new_analysis_started": false
            }))),
            Err(error) if error.contains("422 Unprocessable Entity") => {
                ToolResult::fail(remote_contract_mismatch(query, error))
            }
            Err(error) => ToolResult::fail(blocked("remote_query_failed", error)),
        }
    }
}

#[derive(Debug, Clone)]
struct EvidenceQuery {
    section: &'static str,
    label: &'static str,
    scope: &'static str,
    query: Value,
}

fn evidence_query_plan(
    gene: &str,
    lineage: &str,
    sections: &[String],
    limit: i64,
) -> Vec<EvidenceQuery> {
    let selected = |section: &str| sections.iter().any(|value| value == section);
    let mut queries = Vec::new();
    if selected("core") {
        queries.push(EvidenceQuery {
            section: "core",
            label: "core_dependency",
            scope: "release_core",
            query: json!({"mode":"core","gene":gene}),
        });
    }
    if selected("networks") {
        for (label, family) in [
            ("co_dependency", "effect_correlation"),
            ("expression_correlation", "expression_correlation"),
            ("expression_dependency", "expression_dependency"),
        ] {
            queries.push(EvidenceQuery {
                section: "networks",
                label,
                scope: "lineage",
                query: json!({
                    "mode":"lineage_network",
                    "family":family,
                    "lineage":lineage,
                    "source":gene,
                    "limit":limit
                }),
            });
        }
    }
    if selected("mutation") {
        for (label, module) in [
            (
                "damaging_mutation_dependency",
                "damaging_mutation_dependency",
            ),
            (
                "custom_missense_mutation_dependency",
                "custom_missense_mutation_dependency",
            ),
            ("hotspot_mutation_dependency", "hotspot_mutation_dependency"),
        ] {
            queries.push(EvidenceQuery {
                section: "mutation",
                label,
                // The current sparse lineage mutation contract needs a target pair.
                // Source-to-all discovery is therefore deliberately labelled pan-cancer.
                scope: "pan_cancer",
                query: json!({"mode":"top","module":module,"source":gene,"limit":limit}),
            });
        }
    }
    if selected("cnv") {
        queries.push(EvidenceQuery {
            section: "cnv",
            label: "amplification_dependency",
            scope: "lineage",
            query: json!({
                "mode":"lineage_cnv",
                "lineage":lineage,
                "source":gene,
                "limit":limit
            }),
        });
    }
    if selected("drug") {
        for &omic in DRUG_OMICS {
            queries.push(EvidenceQuery {
                section: "drug",
                label: omic,
                scope: "lineage",
                query: json!({
                    "mode":"lineage_drug",
                    "omic":omic,
                    "lineage":lineage,
                    "target":gene,
                    "limit":limit
                }),
            });
        }
    }
    if selected("enrichment") {
        queries.push(EvidenceQuery {
            section: "enrichment",
            label: "pathway_and_tf",
            scope: "lineage",
            query: json!({
                "mode":"enrichment",
                "lineage":lineage,
                "source":gene,
                "limit":limit
            }),
        });
    }
    if selected("tcga") {
        queries.push(EvidenceQuery {
            section: "tcga",
            label: "tcga_expression_survival",
            scope: "tcga_projects_mapped_to_lineage",
            query: json!({
                "mode":"tcga_expression_survival",
                "gene":gene,
                "lineage":lineage,
                "endpoint":"OS",
                "limit":limit
            }),
        });
    }
    queries
}

fn evidence_input(args: &Value) -> Result<(String, String, Vec<String>, i64), String> {
    let gene = required_string(args, "gene")?;
    let lineage = canonical_lineage_label(&required_string(args, "lineage")?);
    let limit = match args.get("limit") {
        None => MAX_EVIDENCE_LIMIT,
        Some(value) => value
            .as_i64()
            .ok_or_else(|| "'limit' must be an integer".to_string())?,
    };
    if !(1..=MAX_EVIDENCE_LIMIT).contains(&limit) {
        return Err(format!("limit must be between 1 and {MAX_EVIDENCE_LIMIT}"));
    }
    let sections = match args.get("sections") {
        None => EVIDENCE_SECTIONS
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        Some(value) => {
            let values = value
                .as_array()
                .ok_or_else(|| "'sections' must be an array".to_string())?;
            if values.is_empty() {
                return Err("'sections' must contain at least one section".into());
            }
            let mut seen = HashSet::new();
            let mut sections = Vec::with_capacity(values.len());
            for value in values {
                let section = value
                    .as_str()
                    .ok_or_else(|| "every section must be a string".to_string())?;
                if !EVIDENCE_SECTIONS.contains(&section) {
                    return Err(format!("unsupported evidence section '{section}'"));
                }
                if !seen.insert(section) {
                    return Err(format!("duplicate evidence section '{section}'"));
                }
                sections.push(section.to_string());
            }
            sections
        }
    };
    Ok((gene, lineage, sections, limit))
}

fn provider_name(workspace: &KnowledgeWorkspace) -> &'static str {
    match &workspace.provider {
        KnowledgeProvider::Local { .. } => "local",
        KnowledgeProvider::Remote { .. } => "remote",
    }
}

fn compact_gap(entry: &Value) -> Value {
    json!({
        "section": entry.get("section"),
        "label": entry.get("label"),
        "scope": entry.get("scope"),
        "state": entry.get("state"),
        "status": entry.pointer("/result/status"),
        "reason": entry.pointer("/result/reason")
            .or_else(|| entry.get("reason"))
            .or_else(|| entry.pointer("/error/message")),
    })
}

fn normalized_coverage_result(result: &Value) -> Value {
    let mut normalized = result.clone();
    if normalized.get("status").and_then(Value::as_str) == Some("not_testable")
        && normalized
            .get("reason")
            .and_then(Value::as_str)
            .is_some_and(|reason| reason.starts_with("Error in read_cell"))
    {
        normalized["reason"] = Value::String(
            "the requested source event is absent, ineligible, or lacks a source block; this is a coverage state, not an upstream read failure"
                .into(),
        );
    }
    normalized
}

fn core_focus(result: &Value, lineage: &str) -> Option<Value> {
    let gene_summary = result
        .get("summary")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .cloned();
    let lineage_summary = result
        .get("lineages")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find(|row| {
                row.get("lineage")
                    .and_then(Value::as_str)
                    .is_some_and(|value| lineage_match_key(value) == lineage_match_key(lineage))
            })
        })
        .cloned();
    if gene_summary.is_none() && lineage_summary.is_none() {
        return None;
    }
    Some(json!({
        "gene_summary": gene_summary,
        "requested_lineage_summary": lineage_summary,
        "interpretation_guard": "These are current-query descriptive summaries only; they do not contain a lineage-vs-rest test, rank, subtype, distribution shape, or causal claim."
    }))
}

fn compact_manifest(manifest: &Value) -> Value {
    const KEYS: &[&str] = &[
        "status",
        "release",
        "method",
        "multiple_testing",
        "endpoint",
        "expression_scale",
        "target_gene_count",
        "project_count",
        "lineage_sample_n",
        "matched_sample_count",
        "sample_n",
        "min_n",
        "top_k",
        "top_k_each_direction",
    ];
    let Some(object) = manifest.as_object() else {
        return manifest.clone();
    };
    Value::Object(
        KEYS.iter()
            .filter_map(|key| {
                object
                    .get(*key)
                    .cloned()
                    .map(|value| ((*key).into(), value))
            })
            .collect(),
    )
}

fn compact_evidence_ref(provenance: &Value) -> Value {
    match provenance.as_array() {
        // Every current provider lists the durable manifest before the
        // implementation-level Parquet block. The manifest is the stable
        // evidence reference; returning every block path only duplicates
        // storage detail in model context.
        Some(paths) => paths.first().cloned().unwrap_or(Value::Null),
        None => provenance.clone(),
    }
}

/// Project a provider response into the evidence bundle's model-facing
/// contract. Provider endpoints may return complete manifests, source indexes,
/// and every lineage summary; those remain available via a surgical query but
/// must not force the Agent to re-read a spilled 80+ KiB tool-output file.
fn compact_evidence_result(result: &Value, label: &str, limit: i64) -> Value {
    let Some(object) = result.as_object() else {
        return result.clone();
    };
    let mut compact = serde_json::Map::new();
    for key in ["status", "reason", "family", "feature", "collection"] {
        if let Some(value) = object.get(key) {
            compact.insert(key.into(), value.clone());
        }
    }
    if let Some(page_info) = object.get("page_info") {
        compact.insert("page_info".into(), page_info.clone());
    }
    if label != "core_dependency" {
        if let Some(rows) = object.get("rows").and_then(Value::as_array) {
            compact.insert(
                "rows".into(),
                Value::Array(rows.iter().take(limit as usize).cloned().collect()),
            );
        }
        if let Some(result) = object.get("result") {
            compact.insert(
                "result".into(),
                match result.as_array() {
                    Some(rows) => Value::Array(rows.iter().take(limit as usize).cloned().collect()),
                    None => result.clone(),
                },
            );
        }
        if let Some(manifest) = object.get("manifest") {
            compact.insert("manifest".into(), compact_manifest(manifest));
        }
    }
    if let Some(provenance) = object.get("provenance") {
        compact.insert("evidence_ref".into(), compact_evidence_ref(provenance));
    }
    Value::Object(compact)
}

fn query_semantics(query: &Value) -> Value {
    let mode = query
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match mode {
        "lineage_network" => json!({
            "metric":"correlation",
            "interpretation":"signed correlation between the named source and target feature; it is not a group mean difference"
        }),
        "lineage_cnv" => json!({
            "metric":"mean_difference",
            "interpretation":"amplified minus control Gene Effect; negative means amplified models are more dependent"
        }),
        "lineage_drug" => json!({
            "metric":"pearson_r",
            "phenotype":"PRISM AUC",
            "interpretation":"lower AUC means greater sensitivity; interpret the sign against the named omic feature"
        }),
        "enrichment" => json!({
            "metric":"enrichment_z",
            "interpretation":"retained pathway or TF enrichment statistic, not a gene-gene correlation"
        }),
        "pair" | "top" => match query
            .get("module")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "damaging_mutation_dependency"
            | "custom_missense_mutation_dependency"
            | "hotspot_mutation_dependency" => json!({
                "metric":"mean_difference",
                "interpretation":"mutant minus wild-type Gene Effect; negative means mutant models are more dependent",
                "scope":"pan_cancer"
            }),
            "cnv_amplification_dependency" => json!({
                "metric":"mean_difference",
                "interpretation":"amplified minus control Gene Effect; negative means amplified models are more dependent"
            }),
            _ => json!({
                "metric":"correlation",
                "interpretation":"signed association; preserve the source and target matrix definitions"
            }),
        },
        "lineage_catalog" => json!({
            "metric":"module_availability",
            "interpretation":"coverage inventory only; it contains no gene-level association"
        }),
        "lineage_dependency" => json!({
            "metric":"gene_effect_lineage_vs_rest",
            "ranking":query.get("ranking").and_then(Value::as_str).unwrap_or("selective"),
            "interpretation":"effect_mean_difference is lineage mean Gene Effect minus the rest mean; negative means stronger dependency in the lineage. It is not log fold-change. The selective ranking uses the precomputed one-sided Welch test, within-lineage BH FDR, and rank_more_dependent; mean_dependency is descriptive and sorts the lineage Gene Effect mean. Selective does not imply that a validated housekeeping/common-essential filter was applied."
        }),
        "lineage_directions" => json!({
            "metric":"family_specific_shortlists",
            "interpretation":"fixed-filter selection over precomputed lineage network, expression-dependency, CNV, enrichment, and PRISM sparse rows. Each family keeps its own metric and rank; cross-family recurrence is not a combined significance score. Candidates are hypothesis-generating, not proof of novelty or causality."
        }),
        "tcga_expression_survival" => json!({
            "metric":"cox_score_z",
            "expression_scale":"log2(TPM+1)",
            "interpretation":"signed univariate Cox score association between tumour expression and the named censored survival endpoint; this is not a DepMap cell-line/patient sample join or a causal effect"
        }),
        _ => json!({"metric":"mixed","interpretation":"use the exact returned field names"}),
    }
}

fn assemble_evidence(
    workspace: &KnowledgeWorkspace,
    gene: &str,
    lineage: &str,
    requested_sections: &[String],
    limit: i64,
    results: Vec<(EvidenceQuery, ToolResult)>,
) -> Value {
    let total_queries = results.len();
    let mut grouped: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut gaps = Vec::new();
    let mut successful = 0usize;
    let mut blocked_queries = 0usize;
    let mut retained_bytes = 0usize;
    let mut focus_core = Value::Null;

    for (planned, tool_result) in results {
        let parsed = serde_json::from_str::<Value>(&tool_result.content)
            .unwrap_or_else(|_| json!({"state":"blocked","message":tool_result.content}));
        let state = parsed
            .get("state")
            .and_then(Value::as_str)
            .unwrap_or(if tool_result.success {
                "precomputed_query"
            } else {
                "blocked"
            })
            .to_string();
        let semantics = query_semantics(&planned.query);
        let capability_contract = parsed
            .get("capability_contract")
            .cloned()
            .or_else(|| {
                planned
                    .query
                    .get("mode")
                    .and_then(Value::as_str)
                    .and_then(|mode| crate::depmap_capabilities::query_contract(mode).ok())
            })
            .unwrap_or(Value::Null);
        let mut entry = json!({
            "section": planned.section,
            "label": planned.label,
            "scope": planned.scope,
            "state": state,
            "semantics": semantics,
            "capability_contract": capability_contract,
        });
        if tool_result.success {
            if let Some(result) = parsed.get("result") {
                let normalized = normalized_coverage_result(result);
                if planned.label == "core_dependency" {
                    if let Some(focus) = core_focus(&normalized, lineage) {
                        focus_core = focus;
                    }
                }
                entry["result"] = compact_evidence_result(&normalized, planned.label, limit);
            }
            if let Some(reason) = parsed.get("reason") {
                entry["reason"] = reason.clone();
            }
        } else {
            blocked_queries += 1;
            entry["error"] = parsed;
        }

        let entry_bytes = serde_json::to_vec(&entry).map_or(0, |bytes| bytes.len());
        if retained_bytes.saturating_add(entry_bytes) > MAX_EVIDENCE_RESPONSE_BYTES {
            gaps.push(json!({
                "section": planned.section,
                "label": planned.label,
                "scope": planned.scope,
                "state": "response_limit",
                "reason": format!("evidence bundle is capped at {MAX_EVIDENCE_RESPONSE_BYTES} bytes")
            }));
            continue;
        }
        retained_bytes += entry_bytes;
        if tool_result.success && state != "coverage_gap" {
            successful += 1;
        } else {
            gaps.push(compact_gap(&entry));
        }
        grouped
            .entry(planned.section.to_string())
            .or_default()
            .push(entry);
    }

    let mut sections = serde_json::Map::new();
    for section in requested_sections {
        let queries = grouped.remove(section).unwrap_or_default();
        let ready = queries
            .iter()
            .filter(|entry| entry["state"] != "coverage_gap" && entry["state"] != "blocked")
            .count();
        let section_state = if queries.is_empty() || ready == 0 {
            "coverage_gap"
        } else if ready == queries.len() {
            "ready"
        } else {
            "partial"
        };
        sections.insert(
            section.clone(),
            json!({"state":section_state,"queries":queries}),
        );
    }
    let state = if successful == 0 && blocked_queries > 0 {
        "blocked"
    } else if successful == 0 {
        "coverage_gap"
    } else if gaps.is_empty() {
        "evidence_ready"
    } else {
        "evidence_partial"
    };
    json!({
        "schema_version": 1,
        "state": state,
        "provider": provider_name(workspace),
        "release": workspace.release,
        "subject": {"gene":gene,"lineage":lineage},
        "focus": {"core": focus_core},
        "selection": {
            "requested_sections": requested_sections,
            "limit_per_query": limit,
            "response_contract": "compact initial evidence view; use one surgical depmap_query follow-up for omitted provider detail",
            "retention_note": "rows are a bounded view of retained sparse results; exact module method and retention metadata remain in each evidence_ref manifest",
            "mutation_scope_note": "Mutation source-to-all scans are pan-cancer because the current lineage mutation contract requires a specified target pair."
        },
        "summary": {
            "query_count":total_queries,
            "successful_queries":successful,
            "blocked_queries":blocked_queries
        },
        "coverage_gaps": gaps,
        "sections": sections,
        "provenance": {
            "source": "precomputed_depmap_knowledge",
            "contract": EVIDENCE_TOOL_NAME,
            "dynamic_view": true,
            "capability_registry": crate::depmap_capabilities::registry_summary("gene_evidence_in_lineage").ok(),
            "result_state_contract": crate::depmap_capabilities::result_states().ok()
        },
        "new_analysis_started": false
    })
}

fn depmap_evidence_schema() -> Value {
    json!({
        "type":"object",
        "properties": {
            "gene": {"type":"string","description":"Resolved gene symbol"},
            "lineage": {"type":"string","description":"DepMap lineage/cancer label"},
            "sections": {
                "type":"array",
                "minItems":1,
                "maxItems":EVIDENCE_SECTIONS.len(),
                "uniqueItems":true,
                "items":{"type":"string","enum":EVIDENCE_SECTIONS}
            },
            "limit": {"type":"integer","minimum":1,"maximum":MAX_EVIDENCE_LIMIT}
        },
        "required":["gene","lineage"],
        "additionalProperties":false
    })
}

#[async_trait::async_trait]
impl Tool for DepMapEvidenceTool {
    fn name(&self) -> &str {
        EVIDENCE_TOOL_NAME
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            EVIDENCE_TOOL_NAME,
            "Build one bounded, read-only evidence bundle for a user-supplied gene and cancer lineage from the active precomputed DepMap provider. Use depmap_query mode=lineage_catalog instead when no gene was supplied; never invent an anchor gene. The result preserves query-level provenance, metric semantics, and explicit coverage gaps and never starts a new analysis.",
            depmap_evidence_schema(),
        )
    }

    fn read_only(&self) -> bool {
        true
    }

    fn context_result_budget(&self) -> Option<usize> {
        // The projected twelve-query evidence contract is normally below
        // 26 KiB as pretty JSON. Keeping it in the first tool result avoids a
        // spill-file read loop while remaining far below the provider cap.
        Some(32 * 1024)
    }

    fn preview(&self, args: &Value) -> String {
        let gene = args.get("gene").and_then(Value::as_str).unwrap_or_default();
        let lineage = args
            .get("lineage")
            .and_then(Value::as_str)
            .unwrap_or_default();
        format!("{gene} in {lineage}").trim().to_string()
    }

    async fn run(&self, args: &Value, _env: &dyn ToolEnv) -> ToolResult {
        let (gene, lineage, sections, limit) = match evidence_input(args) {
            Ok(input) => input,
            Err(error) => return ToolResult::fail(blocked("invalid_evidence_request", error)),
        };
        let workspace = match self.query.workspace().await {
            Ok(workspace) => workspace,
            Err(error) => return ToolResult::fail(blocked("configuration_blocked", error)),
        };
        let status = self.query.run_status(&workspace).await;
        if !status.success {
            return status;
        }
        let plan = evidence_query_plan(&gene, &lineage, &sections, limit);
        let query_tool = self.query.clone();
        let workspace_for_queries = workspace.clone();
        let mut completed = stream::iter(plan.into_iter().enumerate())
            .map(move |(index, planned)| {
                let query_tool = query_tool.clone();
                let workspace = workspace_for_queries.clone();
                async move {
                    let result = query_tool.run_query(&workspace, &planned.query).await;
                    (index, planned, result)
                }
            })
            .buffer_unordered(3)
            .collect::<Vec<_>>()
            .await;
        completed.sort_by_key(|(index, _, _)| *index);
        let results = completed
            .into_iter()
            .map(|(_, planned, result)| (planned, result))
            .collect();
        let evidence = assemble_evidence(&workspace, &gene, &lineage, &sections, limit, results);
        if evidence["state"] == "blocked" {
            ToolResult::fail(pretty(evidence))
        } else {
            self.query
                .persist_scientific_result(
                    EVIDENCE_TOOL_NAME,
                    &json!({
                        "gene":gene,
                        "lineage":lineage,
                        "sections":sections,
                        "limit":limit
                    }),
                    ToolResult::ok(pretty(evidence)),
                )
                .await
        }
    }
}

#[async_trait::async_trait]
impl Tool for DepMapQueryTool {
    fn name(&self) -> &str {
        TOOL_NAME
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            TOOL_NAME,
            "Query the active project's precomputed DepMap knowledge provider through a flat model-compatible schema. This tool is read-only and keeps full matrices out of context. Choose the mode from depmap_agent_route.recommended_query or the current schema, not from model memory. Every successful result carries a capability_contract with its scope, metric, allowed claims, and forbidden claims. Sparse results distinguish FOUND, NOT_RETAINED, INELIGIBLE, NOT_COMPUTED, and MODULE_UNAVAILABLE. Never repeat an empty-argument or rejected mode call and never start raw-data analysis from a coverage gap.",
            depmap_query_schema(),
        )
    }

    fn read_only(&self) -> bool {
        true
    }

    fn preview(&self, args: &Value) -> String {
        let mode = args.get("mode").and_then(Value::as_str).unwrap_or("status");
        let subject = args
            .get("gene")
            .or_else(|| args.get("source"))
            .or_else(|| args.get("drug"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        format!("{mode} {subject}").trim().to_string()
    }

    async fn run(&self, args: &Value, _env: &dyn ToolEnv) -> ToolResult {
        let workspace = match self.workspace().await {
            Ok(workspace) => workspace,
            Err(error) => return ToolResult::fail(blocked("configuration_blocked", error)),
        };
        if args.get("mode").and_then(Value::as_str) == Some("status") {
            self.run_status(&workspace).await
        } else {
            let result = self.run_query(&workspace, args).await;
            self.persist_scientific_result(TOOL_NAME, args, result)
                .await
        }
    }
}

fn depmap_query_schema() -> Value {
    let modes = crate::depmap_capabilities::query_modes().unwrap_or_default();
    let phenotypes = crate::depmap_capabilities::concept_ids("phenotypes").unwrap_or_default();
    let molecular_focus =
        crate::depmap_capabilities::concept_ids("molecular_focus").unwrap_or_default();
    let mechanisms = crate::depmap_capabilities::concept_ids("mechanisms").unwrap_or_default();
    let evidence_sources =
        crate::depmap_capabilities::concept_ids("evidence_sources").unwrap_or_default();
    let requested_outputs =
        crate::depmap_capabilities::concept_ids("requested_outputs").unwrap_or_default();
    let execution_policies =
        crate::depmap_capabilities::concept_ids("execution_policies").unwrap_or_default();
    json!({
        "type":"object",
        "description":"Flat model-compatible schema. Runtime validation enforces the fields required by each mode.",
        "properties": {
            "mode": {"type":"string","enum":modes},
            "gene": {"type":"string"},
            "module": {"type":"string","enum":MATRIX_MODULES},
            "source": {"type":"string"},
            "target": {"type":"string"},
            "limit": {"type":"integer","minimum":1,"maximum":MAX_TOP_LIMIT,"default":20,"description":"Page size. Omit to use the capability default. Read page_info to distinguish the returned window from the full retained result set."},
            "cursor": {"type":"string","minLength":1,"maxLength":2048,"description":"Opaque next_cursor from the preceding response. Reuse the same scientific filters; do not construct or edit cursors."},
            "event": {"type":"string","enum":LINEAGE_EVENTS},
            "lineage": {"type":"string"},
            "pathway": {"type":"string"},
            "drug": {"type":"string"},
            "omic": {"type":"string","enum":DRUG_OMICS},
            "family": {"type":"string","enum":LINEAGE_NETWORK_FAMILIES},
            "ranking": {"type":"string","enum":LINEAGE_DEPENDENCY_RANKINGS,"description":"For lineage_dependency: selective (default; one-sided FDR-significant lineage-vs-rest effects ordered by precomputed rank) or mean_dependency (descriptive lowest lineage mean Gene Effect)."},
            "focus": {"type":"string","enum":DIRECTION_FOCI,"description":"For lineage_directions only: restrict the returned evidence families to the requested scientific focus."},
            "phenotypes": {"type":"array","items":{"type":"string","enum":phenotypes},"uniqueItems":true,"maxItems":6},
            "molecular_focus": {"type":"array","items":{"type":"string","enum":molecular_focus},"uniqueItems":true,"maxItems":6},
            "mechanisms": {"type":"array","items":{"type":"string","enum":mechanisms},"uniqueItems":true,"maxItems":6},
            "evidence_sources": {"type":"array","items":{"type":"string","enum":evidence_sources},"uniqueItems":true,"maxItems":6},
            "requested_outputs": {"type":"array","items":{"type":"string","enum":requested_outputs},"uniqueItems":true,"maxItems":6},
            "execution_policy": {"type":"string","enum":execution_policies},
            "unresolved_concepts": {"type":"array","items":{"type":"string","minLength":1,"maxLength":128},"uniqueItems":true,"maxItems":8},
            "question_tags": {"type":"array","items":{"type":"string","minLength":1,"maxLength":128},"uniqueItems":true,"maxItems":8,"description":"Canonical intent tags returned by depmap_agent_route routing_hints."},
            "entity_sets": {"type":"array","items":{"type":"string","minLength":1,"maxLength":128},"uniqueItems":true,"maxItems":8,"description":"Canonical entity-set selectors returned by depmap_agent_route routing_hints."},
            "collection": {"type":"string"},
            "term": {"type":"string"},
            "reciprocal": {"type":"boolean"},
            "project": {"type":"string","description":"Optional TCGA project code, for example TCGA-BRCA or BRCA"},
            "endpoint": {"type":"string","enum":["OS","DSS","DFI","PFI"]}
        },
        "required":["mode"],
        "additionalProperties":false
    })
}

fn validated_query(args: &Value) -> Result<Value, String> {
    let mode = required_string(args, "mode")?;
    let capability = crate::depmap_capabilities::query_capability(&mode)?;
    let mut query = serde_json::Map::new();
    query.insert("mode".into(), Value::String(mode.clone()));
    for key in &capability.required_arguments {
        let value = required_string(args, key)?;
        let value = if key == "lineage" {
            canonical_lineage_label(&value)
        } else {
            value
        };
        query.insert(key.clone(), Value::String(value));
    }
    for alternatives in &capability.required_any {
        if !alternatives.iter().any(|key| {
            args.get(key)
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
        }) {
            return Err(format!(
                "{mode} requires at least one of {}",
                alternatives.join(", ")
            ));
        }
    }
    if matches!(mode.as_str(), "pair" | "top") {
        require_allowed(&query, "module", MATRIX_MODULES)?;
    }
    if mode == "lineage" {
        require_allowed(&query, "event", LINEAGE_EVENTS)?;
    }
    if mode == "drug" {
        require_allowed(&query, "omic", DRUG_OMICS)?;
    }
    if mode == "lineage_network" {
        require_allowed(&query, "family", LINEAGE_NETWORK_FAMILIES)?;
    }
    if mode == "lineage_dependency" {
        let ranking = args
            .get("ranking")
            .and_then(Value::as_str)
            .unwrap_or("selective")
            .trim();
        if !LINEAGE_DEPENDENCY_RANKINGS.contains(&ranking) {
            return Err(format!("unsupported ranking '{ranking}'"));
        }
        query.insert("ranking".into(), Value::String(ranking.to_string()));
    }
    if mode == "lineage_directions" {
        let focus = args
            .get("focus")
            .and_then(Value::as_str)
            .unwrap_or("all")
            .trim();
        if !DIRECTION_FOCI.contains(&focus) {
            return Err(format!("unsupported lineage direction focus '{focus}'"));
        }
        query.insert("focus".into(), Value::String(focus.to_string()));
    }
    if mode == "topic_plan" {
        for (field, category) in [
            ("phenotypes", "phenotypes"),
            ("molecular_focus", "molecular_focus"),
            ("mechanisms", "mechanisms"),
            ("evidence_sources", "evidence_sources"),
            ("requested_outputs", "requested_outputs"),
        ] {
            let values = validated_concept_array(args, field, category)?;
            if !values.is_empty() {
                query.insert(field.into(), json!(values));
            }
        }
        let unresolved = validated_free_text_array(args, "unresolved_concepts")?;
        if !unresolved.is_empty() {
            query.insert("unresolved_concepts".into(), json!(unresolved));
        }
        let execution_policy = args
            .get("execution_policy")
            .and_then(Value::as_str)
            .unwrap_or("precomputed_only")
            .trim();
        let policies = crate::depmap_capabilities::concept_ids("execution_policies")?;
        if !policies
            .iter()
            .any(|candidate| candidate == execution_policy)
        {
            return Err(format!("unsupported execution_policy '{execution_policy}'"));
        }
        query.insert("execution_policy".into(), json!(execution_policy));
    }
    if mode == "capability_catalog" {
        for field in ["question_tags", "entity_sets"] {
            let values = validated_free_text_array(args, field)?;
            if !values.is_empty() {
                query.insert(field.into(), json!(values));
            }
        }
        if let Some(lineage) = args
            .get("lineage")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            query.insert(
                "lineage".into(),
                Value::String(canonical_lineage_label(lineage)),
            );
        }
    }
    if mode == "lineage_drug" {
        require_allowed(&query, "omic", DRUG_OMICS)?;
        if args
            .get("drug")
            .and_then(Value::as_str)
            .is_none_or(|value| value.trim().is_empty())
            && args
                .get("target")
                .and_then(Value::as_str)
                .is_none_or(|value| value.trim().is_empty())
        {
            return Err("lineage_drug requires non-empty 'drug', 'target', or both".into());
        }
    }
    for key in ["target", "drug", "collection", "term"] {
        if let Some(value) = args
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            query.insert(key.into(), Value::String(value.to_string()));
        }
    }
    if mode == "tcga_expression_survival" {
        if let Some(lineage) = args
            .get("lineage")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            query.insert(
                "lineage".into(),
                Value::String(canonical_lineage_label(lineage)),
            );
        }
        if let Some(project) = args
            .get("project")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let project = project.to_ascii_uppercase();
            let project = if project.starts_with("TCGA-") {
                project
            } else {
                format!("TCGA-{project}")
            };
            query.insert("project".into(), Value::String(project));
        }
        let endpoint = args
            .get("endpoint")
            .and_then(Value::as_str)
            .unwrap_or("OS")
            .trim()
            .to_ascii_uppercase();
        if !["OS", "DSS", "DFI", "PFI"].contains(&endpoint.as_str()) {
            return Err("endpoint must be one of OS, DSS, DFI, or PFI".into());
        }
        query.insert("endpoint".into(), Value::String(endpoint));
    }
    if let Some(reciprocal) = args.get("reciprocal").and_then(Value::as_bool) {
        query.insert("reciprocal".into(), Value::Bool(reciprocal));
    }
    if let Some(cursor) = args
        .get("cursor")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if capability.default_limit.is_none() {
            return Err(format!(
                "mode '{mode}' does not return a pageable result window"
            ));
        }
        if cursor.len() > 2048 || cursor.chars().any(char::is_control) {
            return Err("cursor must be at most 2048 printable characters".into());
        }
        query.insert("cursor".into(), Value::String(cursor.to_string()));
    }
    if let Some(default_limit) = capability.default_limit {
        let limit = args
            .get("limit")
            .and_then(Value::as_i64)
            .unwrap_or(default_limit);
        let maximum_limit = capability.maximum_limit.unwrap_or(MAX_TOP_LIMIT);
        let minimum_limit = capability.minimum_limit.unwrap_or(1);
        if !(minimum_limit..=maximum_limit).contains(&limit) {
            return Err(format!(
                "limit must be between {minimum_limit} and {maximum_limit}"
            ));
        }
        query.insert("limit".into(), json!(limit));
    }
    Ok(Value::Object(query))
}

fn attach_capability_contract(query: &Value, mut result: ToolResult) -> ToolResult {
    if !result.success {
        return result;
    }
    let Some(mode) = query.get("mode").and_then(Value::as_str) else {
        return result;
    };
    let Ok(mut parsed) = serde_json::from_str::<Value>(&result.content) else {
        return result;
    };
    let Value::Object(ref mut object) = parsed else {
        return result;
    };
    let Ok(contract) = crate::depmap_capabilities::query_contract(mode) else {
        return result;
    };
    object.insert("capability_contract".into(), contract);
    if let Ok(states) = crate::depmap_capabilities::result_states() {
        object.insert("result_state_contract".into(), states);
    }
    object.entry("query").or_insert_with(|| query.clone());
    result.content = pretty(parsed);
    result
}

fn required_string(args: &Value, key: &str) -> Result<String, String> {
    let value = args
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("mode requires non-empty '{key}'"))?;
    if value.chars().count() > 256 || value.chars().any(char::is_control) {
        return Err(format!("'{key}' must be at most 256 printable characters"));
    }
    Ok(value)
}

fn validated_free_text_array(args: &Value, key: &str) -> Result<Vec<String>, String> {
    let Some(value) = args.get(key) else {
        return Ok(Vec::new());
    };
    let values = value
        .as_array()
        .ok_or_else(|| format!("'{key}' must be an array"))?;
    if values.len() > 8 {
        return Err(format!("'{key}' must contain at most 8 values"));
    }
    let mut result = Vec::new();
    for value in values {
        let value = value
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| format!("'{key}' must contain non-empty strings"))?;
        if value.chars().count() > 128 || value.chars().any(char::is_control) {
            return Err(format!(
                "'{key}' values must be at most 128 printable characters"
            ));
        }
        if !result.iter().any(|item| item == value) {
            result.push(value.to_string());
        }
    }
    Ok(result)
}

fn validated_concept_array(args: &Value, key: &str, category: &str) -> Result<Vec<String>, String> {
    let values = validated_free_text_array(args, key)?;
    let supported = crate::depmap_capabilities::concept_ids(category)?;
    for value in &values {
        if !supported.iter().any(|candidate| candidate == value) {
            return Err(format!("unsupported {category} concept '{value}'"));
        }
    }
    Ok(values)
}

fn lineage_match_key(value: &str) -> String {
    value
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
        .collect()
}

fn canonical_lineage_label(value: &str) -> String {
    crate::depmap_entities::canonical_lineage_label(value)
}

fn recognized_canonical_lineage(value: &str) -> Option<String> {
    crate::depmap_entities::recognized_canonical_lineage(value)
}

fn require_allowed(
    query: &serde_json::Map<String, Value>,
    key: &str,
    allowed: &[&str],
) -> Result<(), String> {
    let value = query.get(key).and_then(Value::as_str).unwrap_or_default();
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(format!("unsupported {key} '{value}'"))
    }
}

async fn resolve_workspace(project_root: &Path) -> Result<KnowledgeWorkspace, String> {
    let config_path = project_root.join(".wisp").join("depmap-agent.json");
    let config = if config_path.is_file() {
        read_json_file(&config_path)
            .await
            .map_err(|error| format!("invalid {}: {error}", config_path.display()))?
    } else {
        json!({})
    };
    let provider = env_or_config(
        "DEPMAP_KNOWLEDGE_PROVIDER",
        &config,
        "knowledge_provider",
        "provider",
    )
    .unwrap_or_else(|| "local".into())
    .to_ascii_lowercase();
    let release = env_or_config(
        "DEPMAP_KNOWLEDGE_RELEASE",
        &config,
        "knowledge_release",
        "release",
    );
    match provider.as_str() {
        "local" => {
            let configured =
                env_or_config("DEPMAP_KNOWLEDGE_ROOT", &config, "knowledge_root", "root");
            let root = if let Some(root) = configured {
                resolve_project_path(project_root, &root)
            } else if project_root.join("depmap-26q1-qa.json").is_file() {
                project_root.to_path_buf()
            } else {
                project_root.join("knowledge")
            };
            let root = dunce::canonicalize(&root).map_err(|error| {
                format!(
                    "local knowledge root {} is unavailable: {error}",
                    root.display()
                )
            })?;
            let qa = read_json_file(&root.join("depmap-26q1-qa.json")).await?;
            if !qa
                .get("qa_status")
                .and_then(Value::as_str)
                .is_some_and(|status| status.eq_ignore_ascii_case("PASS"))
            {
                return Err("local knowledge provider has not passed QA".into());
            }
            let release = qa
                .get("release")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or(release);
            Ok(KnowledgeWorkspace {
                provider: KnowledgeProvider::Local { root },
                release,
            })
        }
        "remote" => {
            let endpoint = env_or_config(
                "DEPMAP_KNOWLEDGE_ENDPOINT",
                &config,
                "knowledge_endpoint",
                "endpoint",
            )
            .ok_or_else(|| "remote knowledge provider requires an endpoint".to_string())?;
            let endpoint = validate_endpoint(&endpoint)?;
            if config.pointer("/knowledge/tunnel").is_some() {
                return Err(
                    "knowledge.tunnel is not supported by the DepMap agent; connect the server through Wisp Science or provide an already reachable HTTPS/loopback endpoint"
                        .into(),
                );
            }
            Ok(KnowledgeWorkspace {
                provider: KnowledgeProvider::Remote { endpoint },
                release,
            })
        }
        _ => Err("knowledge provider must be 'local' or 'remote'".into()),
    }
}

fn env_or_config(
    env_name: &str,
    config: &Value,
    legacy_key: &str,
    nested_key: &str,
) -> Option<String> {
    std::env::var(env_name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            config
                .get("knowledge")
                .and_then(|knowledge| knowledge.get(nested_key))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .or_else(|| {
            config
                .get(legacy_key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
}

fn resolve_project_path(project_root: &Path, value: &str) -> PathBuf {
    let value = PathBuf::from(value);
    if value.is_absolute() {
        value
    } else {
        project_root.join(value)
    }
}

fn validate_endpoint(endpoint: &str) -> Result<Url, String> {
    let mut url = Url::parse(endpoint).map_err(|error| format!("invalid endpoint: {error}"))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err("remote credentials must not be embedded in the endpoint URL".into());
    }
    let secure = url.scheme() == "https";
    let loopback_http = url.scheme() == "http"
        && match url.host() {
            Some(Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
            Some(Host::Ipv4(host)) => host.is_loopback(),
            Some(Host::Ipv6(host)) => host.is_loopback(),
            None => false,
        };
    if !secure && !loopback_http {
        return Err(
            "remote endpoint must use HTTPS; HTTP is allowed only for loopback testing".into(),
        );
    }
    url.set_fragment(None);
    if !url.path().ends_with('/') {
        url.set_path(&format!("{}/", url.path()));
    }
    Ok(url)
}

fn endpoint_url(endpoint: &Url, route: &str) -> Result<Url, String> {
    endpoint
        .join(route)
        .map_err(|error| format!("cannot append {route} to endpoint: {error}"))
}

async fn remote_json(
    method: reqwest::Method,
    url: Url,
    body: Option<Value>,
    bearer_token: Option<String>,
) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| error.to_string())?;
    let mut request = client.request(method, url.clone());
    if let Some(token) = bearer_token {
        request = request.bearer_auth(token);
    }
    if let Some(body) = body {
        request = request.json(&body);
    }
    let mut response = request
        .send()
        .await
        .map_err(|error| format!("{url}: {error}"))?;
    let status = response.status();
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| error.to_string())? {
        if bytes.len() + chunk.len() > MAX_REMOTE_RESPONSE_BYTES {
            return Err(format!(
                "remote response exceeds {} bytes",
                MAX_REMOTE_RESPONSE_BYTES
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    if !status.is_success() {
        let message = String::from_utf8_lossy(&bytes);
        return Err(format!("remote provider returned {status}: {message}"));
    }
    serde_json::from_slice(&bytes).map_err(|error| format!("remote response is not JSON: {error}"))
}

fn depmap_api_token() -> Option<String> {
    std::env::var("DEPMAP_KNOWLEDGE_API_TOKEN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| models::service_credential("depmap_knowledge_api_token"))
}

async fn read_json_file(path: &Path) -> Result<Value, String> {
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("{}: {error}", path.display()))
}

fn classify_result_state(result: &Value) -> &'static str {
    match result.get("status").and_then(Value::as_str) {
        Some(
            "not_testable" | "NOT_RETAINED" | "INELIGIBLE" | "NOT_COMPUTED" | "MODULE_UNAVAILABLE",
        ) => "coverage_gap",
        _ => "precomputed_query",
    }
}

fn grounding_failure(errors: Vec<String>) -> String {
    pretty(json!({
        "state":"grounding_rejected",
        "errors":errors,
        "next":"Revise the final answer. Remove unsupported claims or bind each data claim to a current-turn evidence_id and exact JSON Pointer. Preserve sparse-result state names verbatim. Then call attempt_completion again.",
        "answer_published":false
    }))
}

fn plan_requests_literature(value: &Value) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(key, value)| {
            (key == "capabilities"
                && value.as_array().is_some_and(|items| {
                    items
                        .iter()
                        .any(|item| item.as_str() == Some("literature_search"))
                }))
                || plan_requests_literature(value)
        }),
        Value::Array(values) => values.iter().any(plan_requests_literature),
        _ => false,
    }
}

fn literature_locator_count(value: &Value) -> usize {
    match value {
        Value::Object(object) => object
            .iter()
            .map(|(key, value)| {
                let own = matches!(
                    key.to_ascii_lowercase().as_str(),
                    "doi" | "pmid" | "pmcid" | "url" | "paper_id" | "reference"
                ) && value.as_str().is_some_and(|value| !value.trim().is_empty());
                usize::from(own) + literature_locator_count(value)
            })
            .sum(),
        Value::Array(values) => values.iter().map(literature_locator_count).sum(),
        _ => 0,
    }
}

/// Promote completed literature-capability deliveries into the same ledger as
/// DepMap and TCGA observations. The record preserves the complete compact
/// delivery and its child-tool provenance. A delivery without a traceable
/// locator remains explicitly unverified and cannot support a positive claim.
pub(crate) async fn register_literature_deliveries(
    store: &wisp_store::Store,
    project_id: &str,
    frame_id: &str,
    deliveries: &[wisp_store::AgentWorkflowDelivery],
) -> Result<Vec<Value>, String> {
    let mut refs = Vec::new();
    for delivery in deliveries {
        let Some(workflow) = store
            .get_agent_workflow(&delivery.workflow_id)
            .await
            .map_err(|error| error.to_string())?
        else {
            continue;
        };
        let plan: Value = match serde_json::from_str(&workflow.plan_json) {
            Ok(plan) if plan_requests_literature(&plan) => plan,
            _ => continue,
        };
        let Some(raw_result) = delivery.result_json.as_deref() else {
            continue;
        };
        let payload: Value = serde_json::from_str(raw_result)
            .map_err(|error| format!("invalid literature delivery {}: {error}", delivery.id))?;
        let succeeded = payload
            .pointer("/result/status")
            .and_then(Value::as_str)
            .is_some_and(|status| status.eq_ignore_ascii_case("succeeded"));
        let locator_count = literature_locator_count(&payload);
        let evidence_state = if succeeded && locator_count > 0 {
            "literature_retrieval"
        } else {
            "literature_unverified"
        };
        let arguments = json!({
            "workflow_id":delivery.workflow_id,
            "delivery_id":delivery.id,
            "generation":delivery.generation
        });
        let semantics = json!({
            "evidence_kind":"literature",
            "validation":"traceable_locator",
            "locator_count":locator_count,
            "entailment_validated":false
        });
        let provenance = json!({
            "source":"wisp_delegated_literature",
            "workflow_id":delivery.workflow_id,
            "delivery_id":delivery.id,
            "generation":delivery.generation,
            "plan":plan
        });
        let compact_payload = compact_ledger_payload(&payload);
        let record = store
            .upsert_scientific_evidence(wisp_store::NewScientificEvidence {
                project_id,
                frame_id,
                provider: "wisp_delegated_literature",
                provider_version: None,
                tool_name: "delegate_tasks_completion",
                arguments: &arguments,
                evidence_state,
                semantics: &semantics,
                provenance: &provenance,
                compact_payload: &compact_payload,
            })
            .await
            .map_err(|error| error.to_string())?;
        refs.push(json!({
            "evidence_id":record.evidence_id,
            "evidence_state":record.evidence_state,
            "workflow_id":delivery.workflow_id,
            "delivery_id":delivery.id,
            "locator_count":locator_count,
            "payload_pointer":"/"
        }));
    }
    Ok(refs)
}

pub(crate) fn literature_evidence_prompt(refs: &[Value]) -> Option<String> {
    (!refs.is_empty()).then(|| {
        format!(
            "<literature_evidence_ledger>\n{}\n</literature_evidence_ledger>\nThese are current-turn literature Evidence Ledger records. Use depmap_evidence_history to inspect an exact record before binding a claim. literature_retrieval proves only that a traceable source locator was returned; it does not by itself validate entailment. literature_unverified cannot support a positive claim.",
            pretty(json!({"records":refs}))
        )
    })
}

fn completion_has_scientific_claim(result: &str) -> bool {
    let lower = result.to_lowercase();
    let scientific = [
        "depmap",
        "crispr",
        "fdr",
        "p值",
        "p 值",
        "样本",
        "模型数",
        "基因数",
        "相关",
        "依赖",
        "富集",
        "药敏",
        "突变",
        "扩增",
        "cnv",
        "auc",
        "hazard",
        "effect",
        "correlation",
        "enrichment",
        "dependency",
        "显著",
        "数据表明",
        "结果显示",
    ]
    .iter()
    .any(|term| lower.contains(term));
    let blocker = [
        "无法查询",
        "查询失败",
        "连接失败",
        "配置缺失",
        "尚未查询",
        "未执行查询",
        "blocked",
        "not configured",
        "query failed",
    ]
    .iter()
    .any(|term| lower.contains(term));
    scientific && !blocker
}

fn collect_scalar_values(value: &Value, numbers: &mut Vec<f64>, strings: &mut Vec<String>) {
    match value {
        Value::Number(number) => {
            if let Some(value) = number.as_f64() {
                numbers.push(value);
            }
        }
        Value::String(value) => {
            strings.push(value.clone());
            if let Ok(number) = value.parse::<f64>() {
                numbers.push(number);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_scalar_values(value, numbers, strings);
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                collect_scalar_values(value, numbers, strings);
            }
        }
        Value::Null | Value::Bool(_) => {}
    }
}

fn numeric_tokens(text: &str) -> Vec<(String, f64)> {
    let regex = Regex::new(r"(?i)[+-]?(?:\d+\.\d+|\d+|\.\d+)(?:e[+-]?\d+)?%?")
        .expect("numeric grounding regex must compile");
    regex
        .find_iter(text)
        .filter_map(|found| {
            let start = found.start();
            let end = found.end();
            let before = text[..start].chars().next_back();
            let after = text[end..].chars().next();
            if before.is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_')
                || after.is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
            {
                return None;
            }
            if before == Some('E') && after == Some(']') {
                return None;
            }
            let line_prefix = text[..start].rsplit('\n').next().unwrap_or_default();
            let line_end = text[end..]
                .find('\n')
                .map(|offset| end + offset)
                .unwrap_or(text.len());
            let line_suffix = &text[end..line_end];
            if line_prefix.trim().is_empty()
                && (line_suffix.starts_with(". ")
                    || line_suffix.starts_with("、")
                    || line_suffix.starts_with(") "))
            {
                return None;
            }
            let raw = found.as_str().to_string();
            let percent = raw.ends_with('%');
            let parsed = raw.trim_end_matches('%').parse::<f64>().ok()?;
            if !percent
                && raw.len() == 4
                && raw.bytes().all(|byte| byte.is_ascii_digit())
                && (1900.0..=2100.0).contains(&parsed)
            {
                return None;
            }
            Some((raw, if percent { parsed / 100.0 } else { parsed }))
        })
        .collect()
}

fn scientific_numeric_tokens(text: &str) -> Vec<(String, f64)> {
    const QUANTITATIVE_CONTEXT: &[&str] = &[
        "fdr",
        "p值",
        "p 值",
        "样本",
        "模型数",
        "基因数",
        "相关",
        "效应",
        "均值",
        "差异",
        "排名",
        "显著",
        "auc",
        "hazard",
        "score",
        "correlation",
        "effect",
        "mean",
        "rank",
        "%",
    ];
    text.lines()
        .filter(|line| {
            let lower = line.to_lowercase();
            QUANTITATIVE_CONTEXT
                .iter()
                .any(|context| lower.contains(context))
        })
        .flat_map(|line| {
            numeric_tokens(line)
                .into_iter()
                .filter(|(raw, _)| {
                    [
                        "个课题",
                        " 个课题",
                        "个方向",
                        " 个方向",
                        "项建议",
                        " 项建议",
                    ]
                    .iter()
                    .all(|suffix| !line.contains(&format!("{raw}{suffix}")))
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn completion_with_evidence_index(result: &str, bindings: &[Value]) -> String {
    if bindings.is_empty() || result.contains("证据索引（机器核验）") {
        return result.to_string();
    }
    let rows = bindings
        .iter()
        .filter_map(|binding| {
            Some(format!(
                "- [{}] `{}` `{}`",
                binding.get("label")?.as_str()?,
                binding.get("evidence_id")?.as_str()?,
                binding.get("json_pointer")?.as_str()?
            ))
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        result.to_string()
    } else {
        format!(
            "{}\n\n证据索引（机器核验）\n\n{}",
            result.trim(),
            rows.join("\n")
        )
    }
}

fn number_matches(claim_raw: &str, claim: f64, evidence: f64) -> bool {
    if !claim.is_finite() || !evidence.is_finite() {
        return false;
    }
    let normalized = claim_raw.trim_end_matches('%');
    let mantissa = normalized.split(['e', 'E']).next().unwrap_or(normalized);
    let decimals = mantissa
        .split_once('.')
        .map_or(0_i32, |(_, decimals)| decimals.len() as i32);
    let exponent = normalized
        .split(['e', 'E'])
        .nth(1)
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(0);
    let mut tolerance = 0.5 * 10_f64.powi(exponent - decimals);
    if claim_raw.ends_with('%') {
        tolerance /= 100.0;
    }
    (claim - evidence).abs() <= tolerance.max(1e-12)
}

fn has_unqualified_overclaim(result: &str) -> Option<&'static str> {
    let lower = result.to_lowercase();
    let qualifiers = [
        "不能",
        "不可",
        "无法",
        "未能",
        "尚未",
        "不代表",
        "并非",
        "不能证明",
        "not ",
        "cannot",
        "does not",
        "no evidence",
        "unvalidated",
        "假设",
        "待验证",
        "需要验证",
        "可能",
        "是否",
        "hypothesis",
        "test whether",
        "may ",
    ];
    let groups: &[(&str, &[&str])] = &[
        (
            "causal",
            &["导致", "驱动了", "证明因果", "causal driver", "causes "],
        ),
        (
            "validated_synthetic_lethality",
            &[
                "已验证合成致死",
                "validated synthetic lethal",
                "proven synthetic lethal",
            ],
        ),
        (
            "clinical_efficacy",
            &[
                "临床有效",
                "改善患者生存",
                "clinical efficacy",
                "improves survival",
            ],
        ),
    ];
    for (category, phrases) in groups {
        for phrase in *phrases {
            let mut from = 0;
            while let Some(relative) = lower[from..].find(phrase) {
                let at = from + relative;
                let prefix_start = lower[..at]
                    .char_indices()
                    .rev()
                    .nth(15)
                    .map_or(0, |(index, _)| index);
                let phrase_end = at + phrase.len();
                let suffix_end = lower[phrase_end..]
                    .char_indices()
                    .nth(15)
                    .map_or(lower.len(), |(offset, _)| phrase_end + offset);
                let context = &lower[prefix_start..suffix_end];
                if !qualifiers
                    .iter()
                    .any(|qualifier| context.contains(qualifier))
                {
                    return Some(category);
                }
                from = at + phrase.len();
            }
        }
    }
    None
}

fn collect_sparse_states(value: &Value, states: &mut HashSet<String>) {
    match value {
        Value::String(value)
            if matches!(
                value.as_str(),
                "NOT_RETAINED" | "INELIGIBLE" | "NOT_COMPUTED" | "MODULE_UNAVAILABLE"
            ) =>
        {
            states.insert(value.clone());
        }
        Value::Array(values) => {
            for value in values {
                collect_sparse_states(value, states);
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                collect_sparse_states(value, states);
            }
        }
        _ => {}
    }
}

async fn validate_completion_grounding(
    store: &wisp_store::Store,
    project_id: &str,
    frame_id: &str,
    evidence_not_before_ms: i64,
    result: &str,
    bindings: &[Value],
) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    if result.trim().is_empty() {
        errors.push("result must contain the final user-facing answer".into());
    }
    if completion_has_scientific_claim(result) && bindings.is_empty() {
        errors.push(
            "the answer makes DepMap/scientific claims but supplies no evidence_bindings".into(),
        );
    }
    if let Some(category) = has_unqualified_overclaim(result) {
        errors.push(format!(
            "unsupported {category} claim: precomputed DepMap association evidence cannot establish this claim"
        ));
    }

    let mut bound_claims = Vec::new();
    let mut binding_labels = HashSet::new();
    let mut disclosed_states = HashSet::new();
    for (index, binding) in bindings.iter().enumerate() {
        let field = |name: &str| {
            binding
                .get(name)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
        };
        let Some(label) = field("label") else {
            errors.push(format!("evidence_bindings[{index}].label is required"));
            continue;
        };
        let Some(claim) = field("claim") else {
            errors.push(format!("evidence_bindings[{index}].claim is required"));
            continue;
        };
        if !binding_labels.insert(label.to_string()) {
            errors.push(format!("duplicate evidence binding label {label}"));
        }
        let Some(evidence_id) = field("evidence_id") else {
            errors.push(format!(
                "evidence_bindings[{index}].evidence_id is required"
            ));
            continue;
        };
        let Some(pointer) = field("json_pointer") else {
            errors.push(format!(
                "evidence_bindings[{index}].json_pointer is required"
            ));
            continue;
        };
        if !result.contains(claim) {
            errors.push(format!(
                "evidence_bindings[{index}].claim is not an exact substring of result"
            ));
        }
        let marker = format!("[{label}]");
        if !claim.contains(&marker) {
            errors.push(format!(
                "evidence_bindings[{index}].claim must include its {marker} marker"
            ));
        }
        let record = match store
            .get_scientific_evidence(project_id, frame_id, evidence_id)
            .await
        {
            Ok(Some(record)) => record,
            Ok(None) => {
                errors.push(format!(
                    "evidence_bindings[{index}] references unknown evidence_id {evidence_id}"
                ));
                continue;
            }
            Err(error) => {
                errors.push(format!(
                    "evidence_bindings[{index}] could not read evidence ledger: {error}"
                ));
                continue;
            }
        };
        if record.updated_at < evidence_not_before_ms {
            errors.push(format!(
                "evidence_bindings[{index}] references stale evidence from before the active turn"
            ));
            continue;
        }
        if matches!(
            record.evidence_state.as_str(),
            "coverage_gap" | "blocked" | "validation_failed" | "literature_unverified"
        ) && !claim.contains(&record.evidence_state)
        {
            errors.push(format!(
                "evidence_bindings[{index}] uses {} evidence as positive support",
                record.evidence_state
            ));
        }
        let payload: Value = match serde_json::from_str(&record.compact_payload_json) {
            Ok(payload) => payload,
            Err(error) => {
                errors.push(format!(
                    "evidence_bindings[{index}] has invalid persisted payload: {error}"
                ));
                continue;
            }
        };
        collect_sparse_states(&payload, &mut disclosed_states);
        let Some(pointed) = payload.pointer(pointer) else {
            errors.push(format!(
                "evidence_bindings[{index}] JSON Pointer {pointer} does not exist"
            ));
            continue;
        };
        let mut evidence_numbers = Vec::new();
        let mut evidence_strings = Vec::new();
        collect_scalar_values(pointed, &mut evidence_numbers, &mut evidence_strings);
        let claim_numbers = numeric_tokens(claim);
        if record.evidence_state == "literature_retrieval" {
            if !pointed.is_string()
                || !evidence_strings
                    .iter()
                    .any(|value| !value.trim().is_empty() && claim.contains(value))
            {
                errors.push(format!(
                    "evidence_bindings[{index}] literature claim must contain the exact DOI/PMID/URL/title locator string at {pointer}"
                ));
            }
        } else if !claim_numbers.is_empty()
            && claim_numbers.iter().any(|(raw, number)| {
                !evidence_numbers
                    .iter()
                    .any(|evidence| number_matches(raw, *number, *evidence))
            })
        {
            errors.push(format!(
                "evidence_bindings[{index}] claim contains a number not supported by {pointer}"
            ));
        } else if claim_numbers.is_empty() {
            if !pointed.is_string() {
                errors.push(format!(
                    "evidence_bindings[{index}] qualitative claim must point to one exact string field, not an object or array"
                ));
            } else if !evidence_strings.iter().any(|value| claim.contains(value)) {
                errors.push(format!(
                    "evidence_bindings[{index}] qualitative claim does not contain the value at {pointer}"
                ));
            }
        }
        bound_claims.push(claim.to_string());
    }

    if completion_has_scientific_claim(result) {
        for (raw, _) in scientific_numeric_tokens(result) {
            if !bound_claims.iter().any(|claim| {
                claim.contains(&raw)
                    && numeric_tokens(claim)
                        .iter()
                        .any(|(number, _)| number == &raw)
            }) {
                errors.push(format!(
                    "scientific number {raw} is not covered by an evidence binding"
                ));
            }
        }
    }
    for state in disclosed_states {
        if !result.contains(&state) {
            errors.push(format!(
                "bound evidence contains {state}; disclose that exact coverage state in result"
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn blocked(code: &str, message: impl Into<String>) -> String {
    pretty(json!({
        "state": "blocked",
        "code": code,
        "message": message.into(),
        "new_analysis_started": false
    }))
}

fn remote_contract_mismatch(query: &Value, message: impl Into<String>) -> String {
    let mode = query
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    pretty(json!({
        "state": "blocked",
        "code": "remote_contract_mismatch",
        "message": message.into(),
        "rejected_mode": mode,
        "retry_same_mode": false,
        "next": "Do not retry this mode or vary its optional arguments. Use the route's recommended_query when it names a different supported mode; otherwise report that the configured provider must be upgraded.",
        "new_analysis_started": false
    }))
}

fn pretty(value: Value) -> String {
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
}

fn compact_ledger_payload(value: &Value) -> Value {
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    if bytes.len() <= MAX_LEDGER_PAYLOAD_BYTES {
        return value.clone();
    }
    json!({
        "state": value.get("state"),
        "query": value.get("query"),
        "subject": value.get("subject"),
        "semantics": value.get("semantics"),
        "provenance": value.get("provenance"),
        "payload_omitted": true,
        "payload_bytes": bytes.len(),
        "payload_sha256": wisp_store::canonical_json_sha256(value).1,
        "reason": format!("ledger payload exceeds the {MAX_LEDGER_PAYLOAD_BYTES} byte compact-record limit")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RouteTestEnv;

    #[async_trait::async_trait]
    impl ToolEnv for RouteTestEnv {
        fn project_root(&self) -> &Path {
            Path::new(".")
        }

        async fn confirm(&self, _message: &str) -> bool {
            true
        }

        async fn emit(&self, _event: wisp_tools::ToolEvent) {}
    }

    #[test]
    fn agent_route_keeps_ordinary_requests_out_of_workflows() {
        let cancer_only = depmap_route(&json!({
            "intent":"cancer_inventory",
            "cancer":"结肠癌"
        }))
        .unwrap();
        assert_eq!(cancer_only["state"], "routed");
        assert_eq!(cancer_only["execution_level"], "L1_DIRECT");
        assert_eq!(cancer_only["requires_approval"], false);
        assert_eq!(cancer_only["entities"]["canonical_lineage"], "Bowel");
        assert_eq!(cancer_only["allowed_next_tools"], json!(["depmap_query"]));
        assert_eq!(
            cancer_only["capability_registry"]["route_id"],
            "cancer_inventory"
        );
        assert_eq!(
            cancer_only["query_contract"]["metric"],
            "module_availability"
        );

        let resolution = depmap_route(&json!({
            "intent":"lineage_resolution",
            "cancer":"结直肠癌"
        }))
        .unwrap();
        assert_eq!(resolution["execution_level"], "L1_DIRECT");
        assert_eq!(
            resolution["recommended_query"],
            json!({
                "tool":"depmap_resolve_entity",
                "arguments":{"entity_type":"cancer","term":"结直肠癌"},
                "single_call":true
            })
        );
        assert_eq!(
            resolution["query_contract"]["metric"],
            "entity_resolution_state"
        );

        let dependency_ranking = depmap_route(&json!({
            "intent":"cancer_dependency_ranking",
            "cancer":"乳腺癌"
        }))
        .unwrap();
        assert_eq!(dependency_ranking["execution_level"], "L1_DIRECT");
        assert_eq!(dependency_ranking["requires_approval"], false);
        assert_eq!(
            dependency_ranking["entities"]["canonical_lineage"],
            "Breast"
        );
        assert!(dependency_ranking["strategy"]
            .as_str()
            .unwrap()
            .contains("not a new analysis"));
        assert_eq!(
            dependency_ranking["recommended_query"]["arguments"]["mode"],
            "lineage_dependency"
        );

        let directions = depmap_route(&json!({
            "intent":"cancer_direction_discovery",
            "cancer":"肝癌"
        }))
        .unwrap();
        assert_eq!(directions["execution_level"], "L2_INVESTIGATE");
        assert_eq!(directions["entities"]["canonical_lineage"], "Liver");
        assert_eq!(
            directions["recommended_query"]["arguments"],
            json!({
                "mode":"topic_plan","lineage":"Liver",
                "evidence_sources":["depmap"],
                "requested_outputs":["candidate_topics"],
                "execution_policy":"precomputed_only","limit":20
            })
        );
        assert_eq!(directions["recommended_query"]["single_call"], true);
        assert_eq!(directions["allowed_next_tools"], json!(["depmap_query"]));
        assert!(directions["query_contract"]["forbidden_claims"]
            .as_array()
            .unwrap()
            .contains(&json!("proof of novelty")));

        let tf_directions = depmap_route(&json!({
            "intent":"cancer_direction_discovery",
            "cancer":"肝癌",
            "evidence_focus":"transcription_factor"
        }))
        .unwrap();
        assert_eq!(
            tf_directions["recommended_query"]["arguments"],
            json!({
                "mode":"topic_plan",
                "lineage":"Liver",
                "molecular_focus":["transcription_factor"],
                "evidence_sources":["depmap"],
                "requested_outputs":["candidate_topics"],
                "execution_policy":"precomputed_only",
                "limit":20
            })
        );

        let stemness_tf = depmap_route(&json!({
            "intent":"cancer_direction_discovery",
            "cancer":"肝癌",
            "phenotypes":["tumor_cell_stemness"],
            "molecular_focus":["transcription_factor"],
            "evidence_sources":["depmap"],
            "requested_outputs":["candidate_topics","feasibility"],
            "execution_policy":"precomputed_only"
        }))
        .unwrap();
        assert_eq!(
            stemness_tf["semantic_slots"],
            json!({
                "disease":{"raw":"肝癌","canonical_lineage":"Liver"},
                "phenotypes":["tumor_cell_stemness"],
                "molecular_focus":["transcription_factor"],
                "research_relations":[{
                    "subject":{"slot":"molecular_focus","id":"transcription_factor"},
                    "predicate":"candidate_association_with",
                    "object":{"slot":"phenotype","id":"tumor_cell_stemness"},
                    "evidence_requirement":"direct_result_or_declared_proxy"
                }],
                "mechanisms":[],
                "evidence_sources":["depmap"],
                "requested_outputs":["candidate_topics","feasibility"],
                "execution_policy":"precomputed_only",
                "coverage_status":null,
                "user_authorized_new_analysis":false,
                "unresolved_concepts":[]
            })
        );
        assert_eq!(
            stemness_tf["recommended_query"]["arguments"],
            json!({
                "mode":"topic_plan","lineage":"Liver",
                "phenotypes":["tumor_cell_stemness"],
                "molecular_focus":["transcription_factor"],
                "evidence_sources":["depmap"],
                "requested_outputs":["candidate_topics","feasibility"],
                "execution_policy":"precomputed_only","limit":20
            })
        );
        assert_eq!(
            stemness_tf["semantic_contracts"]["phenotypes"][0]["precomputed_status"],
            "NOT_COMPUTED"
        );
        assert_eq!(
            stemness_tf["scientific_intent"]["schema_version"],
            "wisp.scientific-intent.v1"
        );
        assert_eq!(
            stemness_tf["scientific_intent"]["routing_hints"]["question_tags"],
            json!([
                "tumor_cell_stemness",
                "stemness_proxy",
                "transcription_factor",
                "regulator"
            ])
        );
        assert_eq!(
            stemness_tf["scientific_intent"]["routing_hints"]["entity_sets"],
            json!(["dorothea_tf_abc"])
        );
        assert_eq!(
            stemness_tf["scientific_intent"]["research_relations"],
            json!([{
                "subject":{"slot":"molecular_focus","id":"transcription_factor"},
                "predicate":"candidate_association_with",
                "object":{"slot":"phenotype","id":"tumor_cell_stemness"},
                "evidence_requirement":"direct_result_or_declared_proxy"
            }])
        );

        let regulated_stemness = depmap_route(&json!({
            "intent":"cancer_direction_discovery",
            "cancer":"肝癌",
            "phenotypes":["tumor_cell_stemness"],
            "molecular_focus":["transcription_factor"],
            "mechanisms":["transcriptional_regulation"]
        }))
        .unwrap();
        assert_eq!(
            regulated_stemness["scientific_intent"]["research_relations"][0]["predicate"],
            "candidate_regulator_of"
        );

        let provider_shape_variation = depmap_route(&json!({
            "intent":"cancer_direction_discovery",
            "cancer":"liver cancer / hepatocellular carcinoma (LIHC)",
            "phenotypes":["tumor_cell_stemness"],
            "molecular_focus":["tumor_cell_stemness","转录因子方向"],
            "evidence_sources":["DepMap 26Q1"],
            "requested_outputs":["candidate_topics","validation-plan"],
            "execution_policy":"只读现有 26Q1 数据，不推断、不补充缺失概念"
        }))
        .unwrap();
        assert_eq!(
            provider_shape_variation["semantic_slots"]["phenotypes"],
            json!(["tumor_cell_stemness"])
        );
        assert_eq!(
            provider_shape_variation["semantic_slots"]["molecular_focus"],
            json!(["transcription_factor"])
        );
        assert_eq!(
            provider_shape_variation["semantic_slots"]["requested_outputs"],
            json!(["candidate_topics", "validation_plan"])
        );
        assert_eq!(
            provider_shape_variation["semantic_slots"]["execution_policy"],
            "precomputed_only"
        );
        assert!(
            provider_shape_variation["semantic_slots"]["unresolved_concepts"]
                .as_array()
                .unwrap()
                .contains(&json!("只读现有 26Q1 数据，不推断、不补充缺失概念"))
        );
        assert_eq!(
            provider_shape_variation["semantic_slots"]["disease"]["canonical_lineage"],
            "Liver"
        );

        let support_mapping = depmap_route(&json!({
            "intent":"study_support_mapping",
            "cancer":"肝癌"
        }))
        .unwrap();
        assert_eq!(support_mapping["execution_level"], "L2_INVESTIGATE");
        assert_eq!(support_mapping["requires_approval"], false);
        assert_eq!(support_mapping["entities"]["canonical_lineage"], "Liver");
        assert_eq!(
            support_mapping["recommended_query"]["arguments"],
            json!({"mode":"lineage_catalog","lineage":"Liver"})
        );
        assert_eq!(support_mapping["recommended_query"]["single_call"], true);
        assert_eq!(
            support_mapping["recommended_query"]["output_contract"]["required_buckets"],
            json!([
                "direct_precomputed_evidence",
                "new_computation_from_available_inputs",
                "missing_data_or_coverage",
                "literature_only_or_unverified_claims"
            ])
        );
        assert_eq!(
            support_mapping["recommended_query"]["output_contract"]["forbidden_shortcuts"],
            json!(["shell", "run_in_context", "filesystem_inventory"])
        );

        let gene_and_cancer = depmap_route(&json!({
            "intent":"gene_evidence",
            "gene":"KRAS",
            "cancer":"肺癌"
        }))
        .unwrap();
        assert_eq!(gene_and_cancer["execution_level"], "L1_DIRECT");
        assert_eq!(
            gene_and_cancer["allowed_next_tools"],
            json!(["depmap_evidence"])
        );

        let exploration = depmap_route(&json!({
            "intent":"topic_exploration",
            "cancer":"乳腺癌"
        }))
        .unwrap();
        assert_eq!(exploration["execution_level"], "L2_INVESTIGATE");
        assert_eq!(exploration["requires_approval"], false);

        let subtype = depmap_route(&json!({
            "intent":"subtype_evidence",
            "gene":"ESR1",
            "cancer":"乳腺癌"
        }))
        .unwrap();
        assert_eq!(
            subtype["recommended_query"]["tool"],
            "depmap_subtype_evidence"
        );
        assert_eq!(
            subtype["recommended_query"]["arguments"],
            json!({"gene":"ESR1","lineage":"Breast","limit":20})
        );
        assert_eq!(
            subtype["query_contract"]["metric"],
            "frozen_subtype_dependency_contrast"
        );

        let three_d = depmap_route(&json!({
            "intent":"three_d_evidence",
            "family":"codependency",
            "source_gene":"MYCN"
        }))
        .unwrap();
        assert_eq!(three_d["recommended_query"]["tool"], "depmap_3d_evidence");
        assert_eq!(
            three_d["recommended_query"]["arguments"],
            json!({"family":"codependency","source":"MYCN","limit":20})
        );
        assert_eq!(
            three_d["query_contract"]["scope"],
            "catalog_validated_3d_family"
        );
    }

    #[test]
    fn agent_route_requires_entities_and_approval_only_for_durable_work() {
        let missing =
            depmap_route(&json!({"intent":"gene_pair_evidence","source_gene":"KRAS"})).unwrap();
        assert_eq!(missing["state"], "needs_input");
        assert_eq!(missing["missing_fields"], json!(["target_gene"]));
        assert_eq!(
            missing["candidate_query_contracts"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            missing["candidate_query_contracts"][0]["reference"],
            "query:pair"
        );

        let synthetic = depmap_route(&json!({"intent":"synthetic_lethal_evidence"})).unwrap();
        assert_eq!(synthetic["state"], "needs_input");
        assert_eq!(
            synthetic["missing_fields"],
            json!(["source_gene_or_target_gene"])
        );

        let unresolved = depmap_route(&json!({
            "intent":"cancer_inventory",
            "cancer":"某个尚未映射的新癌种名称"
        }))
        .unwrap();
        assert_eq!(unresolved["state"], "needs_resolution");
        assert_eq!(unresolved["requires_user_input"], false);
        assert_eq!(unresolved["missing_fields"], json!(["canonical_lineage"]));
        assert_eq!(
            unresolved["recommended_query"],
            json!({
                "tool":"depmap_resolve_entity",
                "arguments":{"entity_type":"cancer","term":"某个尚未映射的新癌种名称"},
                "single_call":true
            })
        );
        assert_eq!(
            unresolved["allowed_next_tools"],
            json!(["depmap_resolve_entity"])
        );
        assert_eq!(
            unresolved["query_contract"]["metric"],
            "entity_resolution_state"
        );

        let report = depmap_route(&json!({
            "intent":"report_generation",
            "gene":"PTK7",
            "cancer":"肝癌"
        }))
        .unwrap();
        assert_eq!(report["execution_level"], "L4_DURABLE");
        assert_eq!(report["requires_approval"], true);
        assert_eq!(
            report["guardrails"]["workflow_semantic_match_alone_is_sufficient"],
            false
        );

        let explicit = depmap_route(&json!({
            "intent":"topic_exploration",
            "cancer":"肝癌",
            "explicit_workflow_request":true
        }))
        .unwrap();
        assert_eq!(explicit["execution_level"], "L4_DURABLE");
        assert_eq!(explicit["requires_approval"], true);
        assert_eq!(explicit["allowed_next_tools"], json!(["start_workflow"]));

        let blocked_new_analysis = depmap_route(&json!({
            "intent":"new_analysis",
            "execution_policy":"allow_new_analysis",
            "coverage_status":"NOT_RETAINED",
            "user_authorized_new_analysis":true
        }))
        .unwrap();
        assert_eq!(blocked_new_analysis["state"], "needs_input");
        assert!(blocked_new_analysis["missing_fields"]
            .as_array()
            .unwrap()
            .contains(&json!("validated_coverage_status=NOT_COMPUTED")));
        assert_eq!(
            blocked_new_analysis["guardrails"]["new_analysis_gate"]["passed"],
            false
        );

        let approved_new_analysis = depmap_route(&json!({
            "intent":"new_analysis",
            "execution_policy":"allow_new_analysis",
            "coverage_status":"NOT_COMPUTED",
            "user_authorized_new_analysis":true
        }))
        .unwrap();
        assert_eq!(approved_new_analysis["state"], "routed");
        assert_eq!(approved_new_analysis["requires_approval"], true);
        assert_eq!(
            approved_new_analysis["guardrails"]["new_analysis_gate"]["passed"],
            true
        );
    }

    #[test]
    fn agent_route_schema_is_flat_and_closed() {
        let schema = depmap_route_schema();
        assert_eq!(schema["required"], json!(["intent"]));
        assert_eq!(schema["additionalProperties"], false);
        assert!(schema.get("oneOf").is_none());
    }

    #[tokio::test]
    async fn agent_route_enforces_its_next_tool_scope() {
        let result = DepMapAgentRouteTool
            .run(
                &json!({
                    "intent":"cancer_direction_discovery",
                    "cancer":"肝癌",
                    "evidence_focus":"transcription_factor"
                }),
                &RouteTestEnv,
            )
            .await;
        assert!(result.success);
        assert_eq!(result.control, wisp_tools::ToolControl::StopBatch);
        let allowed = result.next_tool_allowlist.as_ref().unwrap();
        assert!(allowed.contains(&"depmap_query".to_string()));
        assert!(allowed.contains(&"attempt_completion".to_string()));
        assert!(!allowed.contains(&"shell".to_string()));
        assert!(!allowed.contains(&ROUTE_TOOL_NAME.to_string()));
        let requirements = result.next_tool_requirements.as_ref().unwrap();
        assert_eq!(requirements.len(), 1);
        assert_eq!(requirements[0].tool_name, "depmap_query");
        assert_eq!(
            requirements[0].arguments,
            json!({
                "mode":"topic_plan",
                "lineage":"Liver",
                "molecular_focus":["transcription_factor"],
                "evidence_sources":["depmap"],
                "requested_outputs":["candidate_topics"],
                "execution_policy":"precomputed_only",
                "limit":20
            })
        );
    }

    #[test]
    fn oversized_ledger_payload_is_replaced_by_a_hash_reference() {
        let payload =
            json!({"state":"precomputed_query","result":"x".repeat(MAX_LEDGER_PAYLOAD_BYTES)});
        let compact = compact_ledger_payload(&payload);
        assert_eq!(compact["payload_omitted"], true);
        assert!(compact["payload_sha256"]
            .as_str()
            .is_some_and(|v| v.len() == 64));
        assert_eq!(compact["state"], "precomputed_query");
    }

    #[tokio::test]
    async fn successful_query_result_returns_a_persisted_evidence_reference() {
        let root = std::env::temp_dir().join(format!(
            "wisp-depmap-evidence-ledger-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let store = wisp_store::Store::open(&root.join("store.sqlite"))
            .await
            .unwrap();
        store.create_project("p", "Project", ".").await.unwrap();
        store
            .create_frame("f", "p", "DepMap", "test-model")
            .await
            .unwrap();
        let tool = DepMapQueryTool {
            project_root: root.clone(),
            query_script: root.join("query.R"),
            store: store.clone(),
            project_id: "p".into(),
            frame_id: "f".into(),
        };
        let result = tool
            .persist_scientific_result(
                TOOL_NAME,
                &json!({"mode":"core","gene":"KRAS"}),
                ToolResult::ok(pretty(json!({
                    "state":"precomputed_query",
                    "provider":"local",
                    "release":"26Q1",
                    "query":{"mode":"core","gene":"KRAS"},
                    "semantics":{"metric":"gene_effect"},
                    "result":{"status":"FOUND","value":-0.8}
                }))),
            )
            .await;
        assert!(result.success);
        let value: Value = serde_json::from_str(&result.content).unwrap();
        let evidence_id = value["evidence_ref"]["evidence_id"].as_str().unwrap();
        assert_eq!(evidence_id.len(), 64);
        let records = store.list_scientific_evidence("p", "f", 10).await.unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].evidence_id, evidence_id);
        drop(tool);
        store.close().await;
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn validates_mode_specific_query_fields_and_bounds_top_limit() {
        let query = validated_query(&json!({
            "mode": "pair",
            "module": "effect_correlation",
            "source": "KRAS",
            "target": "RAF1"
        }))
        .unwrap();
        assert_eq!(query["source"], "KRAS");
        assert!(validated_query(&json!({"mode":"pair","source":"KRAS"})).is_err());
        assert!(validated_query(&json!({
            "mode":"top","module":"effect_correlation","source":"KRAS","limit":101
        }))
        .is_err());
        assert!(validated_query(&json!({
            "mode":"pair","module":"arbitrary","source":"KRAS","target":"RAF1"
        }))
        .is_err());
        let lineage = validated_query(&json!({
            "mode":"lineage_network",
            "family":"effect_correlation",
            "lineage":"Lung",
            "source":"KRAS",
            "target":"RAF1",
            "reciprocal":true,
            "limit":5
        }))
        .unwrap();
        assert_eq!(lineage["target"], "RAF1");
        assert_eq!(lineage["reciprocal"], true);
        assert_eq!(lineage["limit"], 5);
        let dependency = validated_query(&json!({
            "mode":"lineage_dependency",
            "lineage":"乳腺癌",
            "limit":10
        }))
        .unwrap();
        assert_eq!(dependency["lineage"], "Breast");
        assert_eq!(dependency["ranking"], "selective");
        assert_eq!(dependency["limit"], 10);
        let next_page = validated_query(&json!({
            "mode":"lineage_dependency",
            "lineage":"乳腺癌",
            "limit":10,
            "cursor":"opaque-cursor"
        }))
        .unwrap();
        assert_eq!(next_page["cursor"], "opaque-cursor");
        assert!(validated_query(&json!({
            "mode":"core","gene":"KRAS","cursor":"opaque-cursor"
        }))
        .is_err());
        let descriptive_dependency = validated_query(&json!({
            "mode":"lineage_dependency",
            "lineage":"colorectal cancer",
            "ranking":"mean_dependency"
        }))
        .unwrap();
        assert_eq!(descriptive_dependency["lineage"], "Bowel");
        assert_eq!(descriptive_dependency["ranking"], "mean_dependency");
        let directions = validated_query(&json!({
            "mode":"lineage_directions",
            "lineage":"肝癌",
            "limit":20
        }))
        .unwrap();
        assert_eq!(directions["lineage"], "Liver");
        assert_eq!(directions["limit"], 20);
        let topic = validated_query(&json!({
            "mode":"topic_plan",
            "lineage":"肝癌",
            "phenotypes":["tumor_cell_stemness"],
            "molecular_focus":["transcription_factor"],
            "evidence_sources":["depmap"],
            "requested_outputs":["candidate_topics"],
            "execution_policy":"precomputed_only"
        }))
        .unwrap();
        assert_eq!(topic["lineage"], "Liver");
        assert_eq!(topic["phenotypes"], json!(["tumor_cell_stemness"]));
        assert_eq!(topic["molecular_focus"], json!(["transcription_factor"]));
        assert_eq!(topic["limit"], 20);
        assert!(validated_query(&json!({
            "mode":"topic_plan","lineage":"Liver","phenotypes":["imagined_state"]
        }))
        .is_err());
        assert!(validated_query(&json!({
            "mode":"lineage_dependency",
            "lineage":"Breast",
            "ranking":"logfc"
        }))
        .is_err());
        assert!(validated_query(&json!({
            "mode":"lineage_drug","omic":"effect","lineage":"Lung"
        }))
        .is_err());
        let tcga = validated_query(&json!({
            "mode":"tcga_expression_survival",
            "gene":"ESR1",
            "lineage":"Breast Cancer",
            "project":"brca",
            "endpoint":"dss",
            "limit":10
        }))
        .unwrap();
        assert_eq!(tcga["lineage"], "Breast");
        assert_eq!(tcga["project"], "TCGA-BRCA");
        assert_eq!(tcga["endpoint"], "DSS");
        assert!(validated_query(&json!({
            "mode":"tcga_expression_survival","gene":"ESR1","endpoint":"RFS"
        }))
        .is_err());
    }

    #[test]
    fn query_schema_is_flat_for_model_compatibility_and_runtime_stays_strict() {
        let schema = depmap_query_schema();
        assert!(schema.get("oneOf").is_none());
        assert_eq!(schema["required"], json!(["mode"]));
        assert_eq!(
            schema["properties"]["module"]["enum"],
            json!(MATRIX_MODULES)
        );
        assert!(schema["properties"]["mode"]["enum"]
            .as_array()
            .unwrap()
            .contains(&json!("lineage_catalog")));
        assert!(schema["properties"]["mode"]["enum"]
            .as_array()
            .unwrap()
            .contains(&json!("lineage_dependency")));
        assert!(schema["properties"]["mode"]["enum"]
            .as_array()
            .unwrap()
            .contains(&json!("lineage_directions")));
        assert!(schema["properties"]["mode"]["enum"]
            .as_array()
            .unwrap()
            .contains(&json!("tcga_expression_survival")));
        assert_eq!(
            schema["properties"]["mode"]["enum"],
            json!(crate::depmap_capabilities::query_modes().unwrap())
        );
        assert!(validated_query(&json!({"mode":"pair"})).is_err());
        assert_eq!(
            validated_query(&json!({
                "mode":"lineage_directions",
                "lineage":"肝癌",
                "focus":"transcription_factor"
            }))
            .unwrap(),
            json!({
                "mode":"lineage_directions",
                "lineage":"Liver",
                "focus":"transcription_factor",
                "limit":20
            })
        );
        assert!(validated_query(&json!({
            "mode":"lineage_directions",
            "lineage":"肝癌",
            "focus":"stemness"
        }))
        .is_err());
        assert!(validated_query(&json!({
            "mode":"lineage_catalog","lineage":"Colorectal"
        }))
        .is_ok());
    }

    #[test]
    fn evidence_schema_and_input_keep_requests_bounded() {
        let schema = depmap_evidence_schema();
        assert_eq!(schema["required"], json!(["gene", "lineage"]));
        assert_eq!(schema["properties"]["limit"]["maximum"], 3);
        assert_eq!(
            schema["properties"]["sections"]["items"]["enum"],
            json!(EVIDENCE_SECTIONS)
        );

        let (_, _, sections, limit) =
            evidence_input(&json!({"gene":"KRAS","lineage":"Lung"})).unwrap();
        assert_eq!(sections, EVIDENCE_SECTIONS);
        assert_eq!(limit, 3);
        assert!(evidence_input(&json!({
            "gene":"KRAS","lineage":"Lung","sections":["core","core"]
        }))
        .is_err());
        assert!(evidence_input(&json!({
            "gene":"KRAS","lineage":"Lung","sections":["unknown"]
        }))
        .is_err());
        assert!(evidence_input(&json!({
            "gene":"KRAS","lineage":"Lung","limit":4
        }))
        .is_err());
        let (_, lineage, _, _) =
            evidence_input(&json!({"gene":"ESR1","lineage":"Breast Cancer"})).unwrap();
        assert_eq!(lineage, "Breast");
    }

    #[test]
    fn lineage_aliases_are_canonicalized_before_querying() {
        for (requested, expected) in [
            ("Breast Cancer", "Breast"),
            ("breast", "Breast"),
            ("Ovarian carcinoma", "Ovary Fallopian Tube"),
            ("colorectal cancer", "Bowel"),
            ("CNS/Brain", "CNS Brain"),
        ] {
            let query = validated_query(&json!({
                "mode":"lineage_network",
                "family":"effect_correlation",
                "lineage":requested,
                "source":"ESR1"
            }))
            .unwrap();
            assert_eq!(query["lineage"], expected);
        }
        let covered: HashSet<&str> = CHINESE_LINEAGE_ALIASES
            .iter()
            .map(|(_, canonical)| *canonical)
            .collect();
        assert_eq!(covered.len(), CANONICAL_LINEAGES.len());
        for canonical in CANONICAL_LINEAGES {
            assert!(covered.contains(canonical));
        }
        for (requested, expected) in CHINESE_LINEAGE_ALIASES {
            assert_eq!(canonical_lineage_label(requested), *expected);
        }
        assert_eq!(
            canonical_lineage_label("  卵巢 / 输卵管  "),
            "Ovary Fallopian Tube"
        );
    }

    #[test]
    fn old_remote_not_testable_stack_is_not_reported_as_a_read_failure() {
        let result = normalized_coverage_result(&json!({
            "status":"not_testable",
            "reason":"Error in read_cell(root, spec, source, target, top = TRUE,  :"
        }));
        assert_eq!(
            result["reason"],
            "the requested source event is absent, ineligible, or lacks a source block; this is a coverage state, not an upstream read failure"
        );
    }

    #[test]
    fn core_focus_exposes_only_the_current_requested_lineage_summary() {
        let focus = core_focus(
            &json!({
                "summary":[{"symbol":"ATF5","effect_n":1208,"damaging_mutation_n":10}],
                "lineages":[
                    {"symbol":"ATF5","lineage":"Liver","effect_n":25,"effect_mean":-0.1231},
                    {"symbol":"ATF5","lineage":"Lung","effect_n":126,"effect_mean":-0.1818}
                ]
            }),
            "liver",
        )
        .unwrap();
        assert_eq!(focus["gene_summary"]["symbol"], "ATF5");
        assert_eq!(focus["requested_lineage_summary"]["lineage"], "Liver");
        assert_eq!(focus["requested_lineage_summary"]["effect_n"], 25);
        assert!(focus["interpretation_guard"]
            .as_str()
            .unwrap()
            .contains("do not contain a lineage-vs-rest test"));
    }

    #[test]
    fn evidence_projection_drops_bulk_indexes_and_caps_rows() {
        let result = json!({
            "status":"FOUND",
            "family":"effect_correlation",
            "lineage":"Liver",
            "source":"ATF5",
            "rows":[
                {"target":"A","cor":0.9},
                {"target":"B","cor":0.8},
                {"target":"C","cor":0.7},
                {"target":"D","cor":0.6}
            ],
            "manifest":{
                "release":"26Q1",
                "status":"complete",
                "lineage_sample_n":25,
                "source_gene_count":18531,
                "storage":"large implementation detail"
            },
            "provenance":["manifest.json","block.parquet"],
            "source_index":[{"gene":"bulk"}],
            "page_info":{
                "returned_rows":3,
                "total_retained_rows":1200,
                "total_is_exact":true,
                "has_more":true,
                "next_cursor":"opaque"
            }
        });
        let compact = compact_evidence_result(&result, "co_dependency", 3);
        assert_eq!(compact["rows"].as_array().unwrap().len(), 3);
        assert_eq!(compact["manifest"]["lineage_sample_n"], 25);
        assert!(compact["manifest"].get("source_gene_count").is_none());
        assert!(compact.get("source_index").is_none());
        assert_eq!(compact["evidence_ref"], "manifest.json");
        assert_eq!(compact["page_info"]["total_retained_rows"], 1200);
        assert_eq!(compact["page_info"]["next_cursor"], "opaque");
    }

    #[test]
    fn evidence_plan_uses_lineage_queries_and_labels_mutation_scope() {
        let sections = EVIDENCE_SECTIONS
            .iter()
            .map(|value| (*value).to_string())
            .collect::<Vec<_>>();
        let plan = evidence_query_plan("KRAS", "Lung", &sections, 5);
        assert_eq!(plan.len(), 13);
        assert!(plan.iter().any(|query| {
            query.label == "co_dependency"
                && query.scope == "lineage"
                && query.query["mode"] == "lineage_network"
                && query.query["lineage"] == "Lung"
        }));
        let mutation = plan
            .iter()
            .find(|query| query.section == "mutation")
            .expect("mutation query");
        assert_eq!(mutation.scope, "pan_cancer");
        assert_eq!(mutation.query["mode"], "top");
        let tcga = plan
            .iter()
            .find(|query| query.section == "tcga")
            .expect("TCGA query");
        assert_eq!(tcga.query["mode"], "tcga_expression_survival");
        assert_eq!(tcga.query["gene"], "KRAS");
        assert_eq!(tcga.query["lineage"], "Lung");
        assert_eq!(tcga.query["endpoint"], "OS");
    }

    #[test]
    fn evidence_assembly_preserves_results_and_explicit_gaps() {
        let workspace = KnowledgeWorkspace {
            provider: KnowledgeProvider::Local {
                root: PathBuf::from("knowledge"),
            },
            release: Some("26Q1".into()),
        };
        let ready = EvidenceQuery {
            section: "core",
            label: "core_dependency",
            scope: "release_core",
            query: json!({"mode":"core","gene":"KRAS"}),
        };
        let missing = EvidenceQuery {
            section: "cnv",
            label: "amplification_dependency",
            scope: "lineage",
            query: json!({"mode":"lineage_cnv","lineage":"Lung","source":"KRAS"}),
        };
        let value = assemble_evidence(
            &workspace,
            "KRAS",
            "Lung",
            &["core".into(), "cnv".into()],
            5,
            vec![
                (
                    ready,
                    ToolResult::ok(pretty(json!({
                        "state":"precomputed_query",
                        "query":{"mode":"core","gene":"KRAS"},
                        "result":{"status":"FOUND","provenance":{"path":"core.parquet"}}
                    }))),
                ),
                (
                    missing,
                    ToolResult::ok(pretty(json!({
                        "state":"coverage_gap",
                        "query":{"mode":"lineage_cnv","lineage":"Lung","source":"KRAS"},
                        "result":{"status":"INELIGIBLE","n_amplified":2,"n_control":14}
                    }))),
                ),
            ],
        );
        assert_eq!(value["state"], "evidence_partial");
        assert_eq!(value["release"], "26Q1");
        assert_eq!(value["summary"]["query_count"], 2);
        assert_eq!(value["summary"]["successful_queries"], 1);
        assert_eq!(value["coverage_gaps"][0]["status"], "INELIGIBLE");
        assert_eq!(
            value["sections"]["core"]["queries"][0]["capability_contract"]["metric"],
            "gene_summary"
        );
        assert_eq!(
            value["provenance"]["capability_registry"]["schema_version"],
            2
        );
        assert!(value["provenance"]["result_state_contract"]
            .get("NOT_RETAINED")
            .is_some());
        assert_eq!(
            value["sections"]["core"]["queries"][0]["result"]["evidence_ref"]["path"],
            "core.parquet"
        );
        assert_eq!(value["new_analysis_started"], false);
    }

    #[test]
    fn evidence_assembly_distinguishes_provider_failure_from_coverage_gap() {
        let workspace = KnowledgeWorkspace {
            provider: KnowledgeProvider::Remote {
                endpoint: Url::parse("https://depmap.example.test/api/v1").unwrap(),
            },
            release: Some("26Q1".into()),
        };
        let planned = EvidenceQuery {
            section: "core",
            label: "core_dependency",
            scope: "release_core",
            query: json!({"mode":"core","gene":"ESR1"}),
        };
        let value = assemble_evidence(
            &workspace,
            "ESR1",
            "Breast",
            &["core".into()],
            5,
            vec![(
                planned,
                ToolResult::fail(blocked("remote_query_failed", "connection reset")),
            )],
        );
        assert_eq!(value["state"], "blocked");
        assert_eq!(value["summary"]["blocked_queries"], 1);
        assert_eq!(
            value["sections"]["core"]["queries"][0]["error"]["code"],
            "remote_query_failed"
        );
        assert_eq!(value["new_analysis_started"], false);
    }

    #[test]
    fn remote_endpoint_requires_https_except_loopback_and_rejects_credentials() {
        assert!(validate_endpoint("https://depmap.example.org/api/v1").is_ok());
        assert!(validate_endpoint("http://localhost:8787/api/v1").is_ok());
        assert!(validate_endpoint("http://127.0.0.1:8787/api/v1").is_ok());
        assert!(validate_endpoint("http://depmap.example.org/api/v1").is_err());
        assert!(validate_endpoint("https://token@depmap.example.org/api/v1").is_err());
    }

    #[test]
    fn successful_query_results_carry_machine_readable_claim_boundaries() {
        let query = json!({
            "mode":"lineage_network",
            "family":"effect_correlation",
            "lineage":"Lung",
            "source":"KRAS",
            "limit":20
        });
        let result = attach_capability_contract(
            &query,
            ToolResult::ok(pretty(json!({
                "state":"precomputed_query",
                "result":{"status":"FOUND","rows":[]}
            }))),
        );
        let value: Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(value["query"], query);
        assert_eq!(
            value["capability_contract"]["metric"],
            "family_declared_correlation"
        );
        assert!(value["capability_contract"]["forbidden_claims"]
            .as_array()
            .unwrap()
            .contains(&json!("synthetic lethality")));
        assert!(value["result_state_contract"].get("INELIGIBLE").is_some());
    }

    #[test]
    fn remote_endpoint_keeps_api_prefix_when_appending_routes() {
        let endpoint = validate_endpoint("https://depmap.example.org/api/v1").unwrap();
        assert_eq!(
            endpoint_url(&endpoint, "query").unwrap().as_str(),
            "https://depmap.example.org/api/v1/query"
        );
    }

    #[test]
    fn not_testable_is_a_coverage_gap_not_a_new_analysis() {
        assert_eq!(
            classify_result_state(&json!({"status":"not_testable"})),
            "coverage_gap"
        );
        assert_eq!(
            classify_result_state(&json!({"status":"tested"})),
            "precomputed_query"
        );
        for status in [
            "NOT_RETAINED",
            "INELIGIBLE",
            "NOT_COMPUTED",
            "MODULE_UNAVAILABLE",
        ] {
            assert_eq!(
                classify_result_state(&json!({"status":status})),
                "coverage_gap"
            );
        }
        assert_eq!(
            classify_result_state(&json!({"status":"FOUND"})),
            "precomputed_query"
        );
    }

    #[test]
    fn remote_contract_rejection_is_terminal_for_the_rejected_mode() {
        let value: Value = serde_json::from_str(&remote_contract_mismatch(
            &json!({"mode":"lineage_dependency","lineage":"Liver"}),
            "remote provider returned 422 Unprocessable Entity",
        ))
        .unwrap();
        assert_eq!(value["code"], "remote_contract_mismatch");
        assert_eq!(value["rejected_mode"], "lineage_dependency");
        assert_eq!(value["retry_same_mode"], false);
        assert_eq!(value["new_analysis_started"], false);
    }

    fn run_summary(id: &str, title: &str, status: wisp_store::RunStatus) -> wisp_store::RunSummary {
        wisp_store::RunSummary {
            id: id.into(),
            frame_id: None,
            context_id: "local".into(),
            title: title.into(),
            kind: "command".into(),
            status,
            created_at: 1,
            started_at: None,
            ended_at: None,
            exit_code: None,
            remote_workdir: None,
            timeout_secs: None,
            last_polled_at: None,
            last_poll_error: None,
            progress_json: "{}".into(),
            harvested_at: None,
            cleaned_at: None,
            cleanup_error: None,
            output_fingerprint: "".into(),
        }
    }

    #[test]
    fn project_cycle_filter_recovers_matching_runs_across_frames() {
        let runs = vec![
            run_summary("r2", "DepMap KRAS lineage", wisp_store::RunStatus::Running),
            run_summary("r1", "Other analysis", wisp_store::RunStatus::Succeeded),
        ];
        let filtered = filter_run_summaries(runs, Some("running"), Some("kras"), 20);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "r2");
    }

    #[test]
    fn result_contract_passes_only_with_r_release_targets_cohort_and_qc() {
        let manifest = json!({
            "schema_version":1,
            "analysis_id":"depmap-kras",
            "dataset_release":"26Q1",
            "language":"R"
        });
        let result = json!({
            "schema_version":1,
            "status":"ok",
            "targets":["KRAS"],
            "cohort":{"n_before":2100,"n_after":87},
            "observations":[{"direction":"negative Gene Effect means stronger dependency"}]
        });
        let qc = json!({
            "schema_version":1,
            "status":"pass",
            "blocking_failures":[]
        });
        let (_checks, failures, warnings) = validate_result_documents(&manifest, &result, &qc);
        assert!(failures.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn result_contract_blocks_failed_qc_and_impossible_cohort_counts() {
        let manifest = json!({
            "schema_version":1,
            "analysis_id":"depmap-kras",
            "dataset_release":"26Q1",
            "language":"Python"
        });
        let result = json!({
            "schema_version":1,
            "status":"ok",
            "targets":[],
            "cohort":{"n_before":5,"n_after":8}
        });
        let qc = json!({
            "schema_version":1,
            "status":"fail",
            "blocking_failures":["model ids duplicated"]
        });
        let (_checks, failures, _warnings) = validate_result_documents(&manifest, &result, &qc);
        assert!(failures.len() >= 5);
    }

    #[tokio::test]
    async fn validated_run_result_is_persisted_and_can_ground_a_number() {
        let root = std::env::temp_dir().join(format!("wisp-run-evidence-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let store = wisp_store::Store::open(&root.join("store.sqlite"))
            .await
            .unwrap();
        store.create_project("p", "Project", ".").await.unwrap();
        store
            .create_frame("f", "p", "Evidence", "test-model")
            .await
            .unwrap();
        let gate = DepMapCompletionGateTool::new(store.clone(), "p".into(), "f".into());
        let response = json!({
            "state":"run_validated",
            "dataset_release":"26Q1",
            "artifacts":[],
            "validated_documents":{
                "result":{"status":"ok","cohort":{"n_after":87},"estimate":-0.421},
                "qc":{"status":"pass"}
            }
        });
        let record =
            persist_validated_run_evidence(&store, "p", "f", "run-1", "analysis/run-1", &response)
                .await
                .unwrap();
        assert_eq!(record.evidence_state, "run_validated");
        let payload: Value = serde_json::from_str(&record.compact_payload_json).unwrap();
        assert_eq!(
            payload.pointer("/validated_documents/result/estimate"),
            Some(&json!(-0.421))
        );
        let claim = "通过QA的新计算Run给出估计值-0.421。[E1]";
        let accepted = gate
            .run(
                &json!({
                    "result":claim,
                    "evidence_bindings":[{
                        "label":"E1",
                        "claim":claim,
                        "evidence_id":record.evidence_id,
                        "json_pointer":"/validated_documents/result/estimate"
                    }]
                }),
                &RouteTestEnv,
            )
            .await;
        assert!(accepted.success, "{}", accepted.content);
        store.close().await;
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn literature_detection_uses_declared_capability_and_traceable_locators() {
        assert!(plan_requests_literature(&json!({
            "steps":[{"spec":{"capabilities":["literature_search"]}}]
        })));
        assert!(!plan_requests_literature(&json!({
            "steps":[{"spec":{"capabilities":["reasoning"],"prompt_template":"please use literature_search"}}]
        })));
        assert_eq!(
            literature_locator_count(&json!({
                "papers":[
                    {"doi":"10.1000/demo","title":"Paper A"},
                    {"pmid":"123456","url":"https://pubmed.ncbi.nlm.nih.gov/123456/"}
                ]
            })),
            3
        );
    }

    #[tokio::test]
    async fn workspace_resolves_nested_remote_project_configuration() {
        let root = std::env::temp_dir().join(format!("wisp-depmap-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join(".wisp")).unwrap();
        std::fs::write(
            root.join(".wisp").join("depmap-agent.json"),
            serde_json::to_vec(&json!({
                "schema_version":2,
                "knowledge": {
                    "provider":"remote",
                    "endpoint":"https://depmap.example.org/api/v1",
                    "release":"26Q1"
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let workspace = resolve_workspace(&root).await.unwrap();
        assert_eq!(workspace.release.as_deref(), Some("26Q1"));
        assert!(matches!(
            workspace.provider,
            KnowledgeProvider::Remote { .. }
        ));
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn workspace_rejects_managed_tunnel_configuration() {
        let root = std::env::temp_dir().join(format!("wisp-depmap-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join(".wisp")).unwrap();
        std::fs::write(
            root.join(".wisp").join("depmap-agent.json"),
            serde_json::to_vec(&json!({
                "schema_version":2,
                "knowledge": {
                    "provider":"remote",
                    "endpoint":"http://127.0.0.1:18876/api/v1",
                    "release":"26Q1",
                    "tunnel": {
                        "enabled":true,
                        "context_id":"ssh:lab-server",
                        "local_port":18876,
                        "remote_port":8876,
                        "access_authorized":true
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let error = resolve_workspace(&root).await.unwrap_err();
        assert!(error.contains("not supported by the DepMap agent"));
        assert!(error.contains("connect the server through Wisp Science"));
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn workspace_requires_local_qa_pass() {
        let root = std::env::temp_dir().join(format!("wisp-depmap-{}", uuid::Uuid::new_v4()));
        let knowledge = root.join("knowledge");
        std::fs::create_dir_all(&knowledge).unwrap();
        std::fs::write(
            knowledge.join("depmap-26q1-qa.json"),
            br#"{"qa_status":"PASS","release":"26Q1"}"#,
        )
        .unwrap();
        let workspace = resolve_workspace(&root).await.unwrap();
        assert_eq!(workspace.release.as_deref(), Some("26Q1"));
        assert!(matches!(
            workspace.provider,
            KnowledgeProvider::Local { .. }
        ));
        std::fs::write(
            knowledge.join("depmap-26q1-qa.json"),
            br#"{"qa_status":"FAIL","release":"26Q1"}"#,
        )
        .unwrap();
        assert!(resolve_workspace(&root).await.is_err());
        std::fs::remove_dir_all(root).ok();
    }

    async fn grounding_fixture() -> (PathBuf, wisp_store::Store, DepMapCompletionGateTool, String) {
        let root =
            std::env::temp_dir().join(format!("wisp-depmap-grounding-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let store = wisp_store::Store::open(&root.join("store.sqlite"))
            .await
            .unwrap();
        store.create_project("p", "Project", ".").await.unwrap();
        store
            .create_frame("f", "p", "Evidence", "test-model")
            .await
            .unwrap();
        let gate = DepMapCompletionGateTool::new(store.clone(), "p".into(), "f".into());
        let payload = json!({
            "state":"precomputed_query",
            "result":{
                "status":"FOUND",
                "rows":[{"gene":"SOX2","lineage":"Liver","r":0.347,"fdr":0.012,"n":25}]
            },
            "semantics":{"metric":"correlation"},
            "provenance":["manifest.json"]
        });
        let record = store
            .upsert_scientific_evidence(wisp_store::NewScientificEvidence {
                project_id: "p",
                frame_id: "f",
                provider: "depmap",
                provider_version: Some("26Q1"),
                tool_name: "depmap_query",
                arguments: &json!({"mode":"lineage_network","source":"SOX2"}),
                evidence_state: "precomputed_query",
                semantics: &payload["semantics"],
                provenance: &payload["provenance"],
                compact_payload: &payload,
            })
            .await
            .unwrap();
        (root, store, gate, record.evidence_id)
    }

    #[tokio::test]
    async fn completion_gate_accepts_current_exact_and_rounded_evidence_numbers() {
        let (root, _store, gate, evidence_id) = grounding_fixture().await;
        let claim = "在25个Liver模型中，SOX2相关系数为0.35，FDR为0.012。[E1]";
        let result = format!("基于相关结果推荐3个方向。\n{claim}");
        let answer = gate
            .run(
                &json!({
                    "result":result,
                    "evidence_bindings":[{
                        "label":"E1",
                        "claim":claim,
                        "evidence_id":evidence_id.clone(),
                        "json_pointer":"/result/rows/0"
                    }]
                }),
                &RouteTestEnv,
            )
            .await;
        assert!(answer.success, "{}", answer.content);
        assert_eq!(answer.control, wisp_tools::ToolControl::StopTurn);
        assert!(answer.content.contains("证据索引（机器核验）"));
        assert!(answer.content.contains(&evidence_id));
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn completion_gate_rejects_invented_numbers_and_stale_evidence() {
        let (root, _store, gate, evidence_id) = grounding_fixture().await;
        let invented = "SOX2在Liver中的相关系数为0.77。[E1]";
        let rejected = gate
            .run(
                &json!({
                    "result":invented,
                    "evidence_bindings":[{
                        "label":"E1","claim":invented,"evidence_id":evidence_id,
                        "json_pointer":"/result/rows/0/r"
                    }]
                }),
                &RouteTestEnv,
            )
            .await;
        assert!(!rejected.success);
        assert!(rejected.content.contains("number not supported"));

        let grounded = "SOX2在Liver中的相关系数为0.347。[E1]";
        let accepted = gate
            .run(
                &json!({
                    "result":grounded,
                    "evidence_bindings":[{
                        "label":"E1","claim":grounded,"evidence_id":evidence_id,
                        "json_pointer":"/result/rows/0/r"
                    }]
                }),
                &RouteTestEnv,
            )
            .await;
        assert!(accepted.success, "{}", accepted.content);
        tokio::time::sleep(Duration::from_millis(2)).await;
        let stale = gate
            .run(
                &json!({
                    "result":grounded,
                    "evidence_bindings":[{
                        "label":"E1","claim":grounded,"evidence_id":evidence_id,
                        "json_pointer":"/result/rows/0/r"
                    }]
                }),
                &RouteTestEnv,
            )
            .await;
        assert!(!stale.success);
        assert!(stale.content.contains("stale evidence"));
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn completion_gate_rejects_unqualified_causal_claims() {
        let (root, _store, gate, _evidence_id) = grounding_fixture().await;
        let rejected = gate
            .run(
                &json!({
                    "result":"DepMap结果证明SOX2导致肝癌干性。",
                    "evidence_bindings":[]
                }),
                &RouteTestEnv,
            )
            .await;
        assert!(!rejected.success);
        assert!(rejected.content.contains("unsupported causal claim"));
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn completion_gate_accepts_traceable_literature_but_rejects_unverified_delivery() {
        let (root, store, gate, _evidence_id) = grounding_fixture().await;
        let literature = store
            .upsert_scientific_evidence(wisp_store::NewScientificEvidence {
                project_id: "p",
                frame_id: "f",
                provider: "wisp_delegated_literature",
                provider_version: None,
                tool_name: "delegate_tasks_completion",
                arguments: &json!({"workflow_id":"wf-lit","generation":1}),
                evidence_state: "literature_retrieval",
                semantics: &json!({"evidence_kind":"literature","entailment_validated":false}),
                provenance: &json!({"workflow_id":"wf-lit"}),
                compact_payload: &json!({"papers":[{"pmid":"12345678"}]}),
            })
            .await
            .unwrap();
        let claim = "既往研究可追溯至 PMID 12345678。[E1]";
        let accepted = gate
            .run(
                &json!({
                    "result":claim,
                    "evidence_bindings":[{
                        "label":"E1",
                        "claim":claim,
                        "evidence_id":literature.evidence_id,
                        "json_pointer":"/papers/0/pmid"
                    }]
                }),
                &RouteTestEnv,
            )
            .await;
        assert!(accepted.success, "{}", accepted.content);

        let unverified = store
            .upsert_scientific_evidence(wisp_store::NewScientificEvidence {
                project_id: "p",
                frame_id: "f",
                provider: "wisp_delegated_literature",
                provider_version: None,
                tool_name: "delegate_tasks_completion",
                arguments: &json!({"workflow_id":"wf-unverified","generation":1}),
                evidence_state: "literature_unverified",
                semantics: &json!({"evidence_kind":"literature"}),
                provenance: &json!({"workflow_id":"wf-unverified"}),
                compact_payload: &json!({"summary":"no locator"}),
            })
            .await
            .unwrap();
        let rejected = gate
            .run(
                &json!({
                    "result":"文献证明该机制成立。[E1]",
                    "evidence_bindings":[{
                        "label":"E1",
                        "claim":"文献证明该机制成立。[E1]",
                        "evidence_id":unverified.evidence_id,
                        "json_pointer":"/summary"
                    }]
                }),
                &RouteTestEnv,
            )
            .await;
        assert!(!rejected.success);
        assert!(rejected.content.contains("literature_unverified"));
        store.close().await;
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn completion_gate_distinguishes_a_testable_hypothesis_from_a_causal_claim() {
        assert_eq!(
            has_unqualified_overclaim("DepMap结果证明SOX2导致肝癌干性。"),
            Some("causal")
        );
        assert_eq!(
            has_unqualified_overclaim("研究假设：SOX2可能导致肝癌干性，需要实验验证。"),
            None
        );
        assert_eq!(
            has_unqualified_overclaim("该相关性不能证明SOX2导致肝癌干性。"),
            None
        );
    }

    #[test]
    fn scientific_number_detection_ignores_topic_counts_and_publication_years() {
        let values =
            scientific_numeric_tokens("基于相关结果推荐3个方向，参考2024年研究；相关系数为0.35。");
        assert_eq!(values, vec![("0.35".into(), 0.35)]);
    }

    #[tokio::test]
    async fn remote_query_sends_bearer_token_and_parses_bounded_json() {
        use axum::{
            extract::Json,
            http::{HeaderMap, StatusCode},
            routing::post,
            Router,
        };
        async fn query(
            headers: HeaderMap,
            Json(body): Json<Value>,
        ) -> Result<Json<Value>, StatusCode> {
            if headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                != Some("Bearer depmap-test-token")
            {
                return Err(StatusCode::UNAUTHORIZED);
            }
            Ok(Json(json!({"status":"ok","echo":body})))
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, Router::new().route("/api/v1/query", post(query)))
                .await
                .unwrap();
        });
        let response = remote_json(
            reqwest::Method::POST,
            Url::parse(&format!("http://{address}/api/v1/query")).unwrap(),
            Some(json!({"mode":"core","gene":"KRAS"})),
            Some("depmap-test-token".into()),
        )
        .await
        .unwrap();
        assert_eq!(response["status"], "ok");
        assert_eq!(response["echo"]["gene"], "KRAS");
        server.abort();
    }
}

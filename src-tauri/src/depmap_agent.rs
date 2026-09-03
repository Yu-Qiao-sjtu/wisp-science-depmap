//! Project-scoped DepMap knowledge routing.
//!
//! The knowledge query tool is deliberately read-only and bounded. It never
//! opens raw DepMap matrices and never starts a recomputation; a coverage gap
//! must transition to the persisted Run path explicitly.

use crate::models;
use futures_util::{stream, StreamExt};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
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
    json!({
        "type":"object",
        "properties": {
            "intent": {
                "type":"string",
                "enum":[
                    "provider_status", "lineage_resolution", "cancer_inventory",
                    "cancer_dependency_ranking",
                    "cancer_direction_discovery", "gene_evidence",
                    "gene_pair_evidence", "drug_gene_evidence",
                    "evidence_comparison", "study_support_mapping", "result_interpretation",
                    "topic_exploration", "literature_validation",
                    "new_analysis", "report_generation"
                ]
            },
            "gene": {"type":"string"},
            "cancer": {"type":"string"},
            "source_gene": {"type":"string"},
            "target_gene": {"type":"string"},
            "drug": {"type":"string"},
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

fn depmap_route(args: &Value) -> Result<Value, String> {
    let intent = required_string(args, "intent")?;
    let gene = non_empty_arg(args, "gene");
    let cancer = non_empty_arg(args, "cancer");
    let source_gene = non_empty_arg(args, "source_gene");
    let target_gene = non_empty_arg(args, "target_gene");
    let drug = non_empty_arg(args, "drug");
    let explicit_workflow = args
        .get("explicit_workflow_request")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let mut missing = Vec::new();
    match intent.as_str() {
        "lineage_resolution"
        | "cancer_inventory"
        | "cancer_dependency_ranking"
        | "cancer_direction_discovery"
        | "study_support_mapping" => {
            if cancer.is_none() {
                missing.push("cancer");
            }
        }
        "gene_evidence" => {
            if gene.is_none() {
                missing.push("gene");
            }
        }
        "gene_pair_evidence" => {
            if source_gene.is_none() {
                missing.push("source_gene");
            }
            if target_gene.is_none() {
                missing.push("target_gene");
            }
        }
        "drug_gene_evidence" => {
            if drug.is_none() {
                missing.push("drug");
            }
            if target_gene.is_none() && gene.is_none() {
                missing.push("target_gene_or_gene");
            }
        }
        "provider_status"
        | "evidence_comparison"
        | "result_interpretation"
        | "topic_exploration"
        | "literature_validation"
        | "new_analysis"
        | "report_generation" => {}
        _ => return Err(format!("unsupported DepMap intent '{intent}'")),
    }

    let (mut execution_level, mut approval, mut strategy, mut tools): (
        &str,
        bool,
        &str,
        Vec<&str>,
    ) =
        match intent.as_str() {
            "provider_status" => (
                "L1_DIRECT",
                false,
                "Check only the configured provider health.",
                vec![TOOL_NAME],
            ),
            "lineage_resolution" | "cancer_inventory" => (
                "L1_DIRECT",
                false,
                "Resolve the cancer label and read its bounded precomputed catalog.",
                vec![TOOL_NAME],
            ),
            "cancer_dependency_ranking" => (
                "L1_DIRECT",
                false,
                "Read the bounded precomputed lineage dependency ranking; this is a query over an existing lineage-vs-rest test, not a new analysis.",
                vec![TOOL_NAME],
            ),
            "gene_evidence" if cancer.is_some() => (
                "L1_DIRECT",
                false,
                "Read one bounded gene-by-lineage evidence bundle.",
                vec![EVIDENCE_TOOL_NAME],
            ),
            "gene_evidence" => (
                "L1_DIRECT",
                false,
                "Read bounded pan-cancer core or association results; do not call the lineage evidence bundle without a cancer.",
                vec![TOOL_NAME],
            ),
            "result_interpretation" => (
                "L1_DIRECT",
                false,
                "Interpret only the current or recovered persisted evidence fields.",
                vec![EVIDENCE_HISTORY_TOOL_NAME],
            ),
            "gene_pair_evidence" | "drug_gene_evidence" => (
                "L1_DIRECT",
                false,
                "Use one surgical pair or drug query against precomputed results.",
                vec![TOOL_NAME],
            ),
            "cancer_direction_discovery" => (
                "L2_INVESTIGATE",
                false,
                "Read one bounded, precomputed lineage direction bundle. Preserve its separate family rankings and do not invent an anchor gene.",
                vec![TOOL_NAME],
            ),
            "evidence_comparison" | "topic_exploration" => (
                "L2_INVESTIGATE",
                false,
                "Assemble a small bounded set of direct queries, then rank only supported directions.",
                vec![TOOL_NAME, EVIDENCE_TOOL_NAME],
            ),
            "study_support_mapping" => (
                "L2_INVESTIGATE",
                false,
                "Map each proposed study claim to returned precomputed evidence, new computation using available inputs, a data gap, or literature-only support. Stay query-only and never inspect or guess filesystem paths.",
                vec![TOOL_NAME, EVIDENCE_TOOL_NAME],
            ),
            "literature_validation" => (
                "L3_DELEGATE",
                false,
                "Keep data evidence fixed and delegate a bounded, independently traceable literature check.",
                vec!["delegate_tasks"],
            ),
            "new_analysis" | "report_generation" => (
                "L4_DURABLE",
                true,
                "Create a persisted Run or registered Workflow only after explicit user approval.",
                vec!["start_workflow", "run_in_context"],
            ),
            _ => unreachable!(),
        };

    let requires_user_input = !missing.is_empty();
    if explicit_workflow && !requires_user_input {
        execution_level = "L4_DURABLE";
        approval = true;
        strategy = "The user explicitly requested a registered Workflow; create only its approval-gated draft.";
        tools = vec!["start_workflow"];
    }

    let canonical_lineage = cancer.as_deref().map(canonical_lineage_label);
    let recommended_query = match (intent.as_str(), canonical_lineage.as_deref()) {
        ("cancer_direction_discovery", Some(lineage)) if !requires_user_input => json!({
            "tool": TOOL_NAME,
            "arguments": {
                "mode": "lineage_directions",
                "lineage": lineage,
                "limit": 20
            },
            "single_call": true
        }),
        ("cancer_dependency_ranking", Some(lineage)) if !requires_user_input => json!({
            "tool": TOOL_NAME,
            "arguments": {
                "mode": "lineage_dependency",
                "lineage": lineage,
                "ranking": "selective",
                "limit": 20
            },
            "single_call": true
        }),
        ("study_support_mapping", Some(lineage)) if !requires_user_input => json!({
            "tool": TOOL_NAME,
            "arguments": {
                "mode": "lineage_catalog",
                "lineage": lineage
            },
            "single_call": true,
            "output_contract": {
                "required_buckets": [
                    "direct_precomputed_evidence",
                    "new_computation_from_available_inputs",
                    "missing_data_or_coverage",
                    "literature_only_or_unverified_claims"
                ],
                "forbidden_shortcuts": ["shell", "run_in_context", "filesystem_inventory"]
            }
        }),
        _ => Value::Null,
    };
    Ok(json!({
        "state": if requires_user_input { "needs_input" } else { "routed" },
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
            "drug": drug
        },
        "strategy": strategy,
        "recommended_query": recommended_query,
        "allowed_next_tools": tools,
        "guardrails": {
            "route_is_evidence": false,
            "workflow_semantic_match_alone_is_sufficient": false,
            "do_not_invent_missing_entities": true
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
            "Classify one new DepMap request into a host-validated execution level before querying evidence. Use cancer_dependency_ranking when the user asks for a cancer's top, strongest, selective, essential, or dependency genes without naming a gene. Use once per new request, not for a follow-up that only interprets the current tool result. This routing record is not scientific evidence. Ordinary status, cancer inventory, dependency ranking, gene, pair, drug, and initial topic exploration requests do not require a Workflow.",
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
            Ok(route) if route["state"] == "routed" => ToolResult::ok(pretty(route)),
            Ok(route) => ToolResult::fail(pretty(route)),
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
}

impl DepMapValidateRunTool {
    pub(crate) fn new(
        store: wisp_store::Store,
        scope: wisp_store::StateScope,
        project_root: PathBuf,
    ) -> Self {
        Self {
            store,
            scope,
            project_root,
        }
    }
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
        let response = json!({
            "state": if failures.is_empty() { "run_validated" } else { "validation_failed" },
            "run_id": run_id,
            "run_dir": run_dir,
            "dataset_release": manifest.get("dataset_release"),
            "checks": checks,
            "blocking_failures": failures,
            "warnings": warnings,
            "artifacts": artifacts
        });
        if response["state"] == "run_validated" {
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
        match &workspace.provider {
            KnowledgeProvider::Local { root } => self.run_local(root, workspace, &query).await,
            KnowledgeProvider::Remote { endpoint } => {
                self.run_remote(endpoint, workspace, &query).await
            }
        }
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
        let mut entry = json!({
            "section": planned.section,
            "label": planned.label,
            "scope": planned.scope,
            "state": state,
            "semantics": semantics,
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
            "dynamic_view": true
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
            "Query the active project's precomputed DepMap knowledge provider through a flat model-compatible schema. This tool is read-only and keeps full matrices out of context. Use mode=lineage_catalog for cancer-only availability, mode=lineage_dependency only for a cancer's dependency-gene ranking, and mode=lineage_directions for a cancer-only research-direction request without an anchor gene. Use mode=status only when provider health is actually needed. Sparse results distinguish FOUND, NOT_RETAINED, INELIGIBLE, NOT_COMPUTED, and MODULE_UNAVAILABLE. Never repeat an empty-argument or rejected mode call and never start raw-data analysis from a coverage gap.",
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
    json!({
        "type":"object",
        "description":"Flat model-compatible schema. Runtime validation enforces the fields required by each mode.",
        "properties": {
            "mode": {"type":"string","enum":[
                "status","catalog","lineage_catalog","lineage_dependency","lineage_directions","core","pair","top",
                "lineage","pathway","drug","lineage_network","lineage_cnv",
                "lineage_drug","enrichment","tcga_expression_survival"
            ]},
            "gene": {"type":"string"},
            "module": {"type":"string","enum":MATRIX_MODULES},
            "source": {"type":"string"},
            "target": {"type":"string"},
            "limit": {"type":"integer","minimum":1,"maximum":MAX_TOP_LIMIT},
            "event": {"type":"string","enum":LINEAGE_EVENTS},
            "lineage": {"type":"string"},
            "pathway": {"type":"string"},
            "drug": {"type":"string"},
            "omic": {"type":"string","enum":DRUG_OMICS},
            "family": {"type":"string","enum":LINEAGE_NETWORK_FAMILIES},
            "ranking": {"type":"string","enum":LINEAGE_DEPENDENCY_RANKINGS,"description":"For lineage_dependency: selective (default; one-sided FDR-significant lineage-vs-rest effects ordered by precomputed rank) or mean_dependency (descriptive lowest lineage mean Gene Effect)."},
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
    let mut query = serde_json::Map::new();
    query.insert("mode".into(), Value::String(mode.clone()));
    let required: &[&str] = match mode.as_str() {
        "catalog" => &[],
        "lineage_catalog" | "lineage_dependency" | "lineage_directions" => &["lineage"],
        "core" => &["gene"],
        "pair" => &["module", "source", "target"],
        "top" => &["module", "source"],
        "lineage" => &["event", "lineage", "source", "target"],
        "pathway" => &["pathway", "target"],
        "drug" => &["omic", "drug", "target"],
        "lineage_network" => &["family", "lineage", "source"],
        "lineage_cnv" => &["lineage", "source"],
        "lineage_drug" => &["omic", "lineage"],
        "enrichment" => &["lineage", "source"],
        "tcga_expression_survival" => &["gene"],
        _ => return Err(format!("unsupported query mode '{mode}'")),
    };
    for key in required {
        let value = required_string(args, key)?;
        let value = if *key == "lineage" {
            canonical_lineage_label(&value)
        } else {
            value
        };
        query.insert((*key).into(), Value::String(value));
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
    if matches!(
        mode.as_str(),
        "top"
            | "lineage_dependency"
            | "lineage_directions"
            | "lineage_network"
            | "lineage_cnv"
            | "lineage_drug"
            | "enrichment"
            | "tcga_expression_survival"
    ) {
        let limit = args.get("limit").and_then(Value::as_i64).unwrap_or(20);
        if !(1..=MAX_TOP_LIMIT).contains(&limit) {
            return Err(format!("limit must be between 1 and {MAX_TOP_LIMIT}"));
        }
        query.insert("limit".into(), json!(limit));
    }
    Ok(Value::Object(query))
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

fn lineage_match_key(value: &str) -> String {
    value
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
        .collect()
}

fn unicode_lineage_alias_key(value: &str) -> String {
    value
        .chars()
        .filter(|character| {
            !character.is_whitespace()
                && !matches!(
                    character,
                    '-' | '_' | '/' | ',' | '，' | '、' | '(' | ')' | '（' | '）'
                )
        })
        .flat_map(char::to_lowercase)
        .collect()
}

fn canonical_lineage_label(value: &str) -> String {
    let requested = value.trim();
    let unicode_key = unicode_lineage_alias_key(requested);
    if let Some((_, canonical)) = CHINESE_LINEAGE_ALIASES
        .iter()
        .find(|(alias, _)| unicode_lineage_alias_key(alias) == unicode_key)
    {
        return (*canonical).to_string();
    }
    let requested_key = lineage_match_key(requested);
    if let Some(canonical) = CANONICAL_LINEAGES
        .iter()
        .find(|candidate| lineage_match_key(candidate) == requested_key)
    {
        return (*canonical).to_string();
    }

    let lower = requested.to_ascii_lowercase();
    let without_suffix = [" cancer", " carcinoma", " tumors", " tumor", " lineage"]
        .iter()
        .find_map(|suffix| lower.strip_suffix(suffix).map(str::trim))
        .filter(|candidate| !candidate.is_empty())
        .unwrap_or(requested);
    let candidate_key = lineage_match_key(without_suffix);
    if let Some(canonical) = CANONICAL_LINEAGES
        .iter()
        .find(|candidate| lineage_match_key(candidate) == candidate_key)
    {
        return (*canonical).to_string();
    }

    match candidate_key.as_str() {
        "brain" | "cns" | "centralnervoussystem" => "CNS Brain".into(),
        "colon" | "colorectal" | "rectal" => "Bowel".into(),
        "esophageal" | "gastric" | "stomach" => "Esophagus Stomach".into(),
        "headneck" | "headandneck" => "Head and Neck".into(),
        "ovarian" | "ovary" | "fallopiantube" => "Ovary Fallopian Tube".into(),
        "pns" => "Peripheral Nervous System".into(),
        "bladder" | "urinarytract" => "Bladder Urinary Tract".into(),
        "vulvar" | "vaginal" | "vulva" | "vagina" => "Vulva Vagina".into(),
        _ => requested.to_string(),
    }
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
            json!({"mode":"lineage_directions","lineage":"Liver","limit":20})
        );
        assert_eq!(directions["recommended_query"]["single_call"], true);
        assert_eq!(directions["allowed_next_tools"], json!(["depmap_query"]));

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
    }

    #[test]
    fn agent_route_requires_entities_and_approval_only_for_durable_work() {
        let missing =
            depmap_route(&json!({"intent":"gene_pair_evidence","source_gene":"KRAS"})).unwrap();
        assert_eq!(missing["state"], "needs_input");
        assert_eq!(missing["missing_fields"], json!(["target_gene"]));

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
    }

    #[test]
    fn agent_route_schema_is_flat_and_closed() {
        let schema = depmap_route_schema();
        assert_eq!(schema["required"], json!(["intent"]));
        assert_eq!(schema["additionalProperties"], false);
        assert!(schema.get("oneOf").is_none());
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
        assert!(validated_query(&json!({"mode":"pair"})).is_err());
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
            "source_index":[{"gene":"bulk"}]
        });
        let compact = compact_evidence_result(&result, "co_dependency", 3);
        assert_eq!(compact["rows"].as_array().unwrap().len(), 3);
        assert_eq!(compact["manifest"]["lineage_sample_n"], 25);
        assert!(compact["manifest"].get("source_gene_count").is_none());
        assert!(compact.get("source_index").is_none());
        assert_eq!(compact["evidence_ref"], "manifest.json");
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

//! Non-exfiltrating remote-compute gateway for DepMap new analysis.
//!
//! Tests inject fakes. Production never opens a live SSH session from this
//! module; missing knowledge context is a typed blocked status.

use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

const CAPABILITY_MANIFEST: &str =
    include_str!("../../skills/depmap-coding-agent/references/capability-manifest.json");

#[derive(Debug, Deserialize)]
struct CapabilityManifest {
    datasets: Vec<Dataset>,
    capabilities: Vec<Capability>,
}

#[derive(Debug, Deserialize)]
struct Dataset {
    id: String,
    kind: String,
    path: String,
    #[serde(default)]
    source_inputs: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Capability {
    id: String,
    #[serde(default)]
    inputs: Vec<String>,
    analysis_label: Option<String>,
    relation_type: Option<String>,
}

pub(crate) trait DatasetInspector {
    fn exists(&self, relative_path: &str) -> bool;
}

pub(crate) struct ProjectDatasetInspector {
    project_root: PathBuf,
    data_root: PathBuf,
}

impl ProjectDatasetInspector {
    pub(crate) fn new(project_root: impl Into<PathBuf>) -> Self {
        let project_root = project_root.into();
        let configured = std::env::var("DEPMAP_DATA_ROOT")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                let config = std::fs::read(project_root.join(".wisp/depmap-agent.json")).ok()?;
                serde_json::from_slice::<Value>(&config)
                    .ok()?
                    .get("data_root")?
                    .as_str()
                    .map(str::to_string)
            });
        let data_root = configured
            .map(PathBuf::from)
            .map(|path| {
                if path.is_absolute() {
                    path
                } else {
                    project_root.join(path)
                }
            })
            .unwrap_or_else(|| project_root.join("data"));
        Self {
            project_root,
            data_root,
        }
    }
}

impl DatasetInspector for ProjectDatasetInspector {
    fn exists(&self, relative_path: &str) -> bool {
        let path = Path::new(relative_path);
        let candidate = path
            .strip_prefix("data")
            .map(|suffix| self.data_root.join(suffix))
            .unwrap_or_else(|_| self.project_root.join(path));
        candidate.is_file() || candidate.is_dir()
    }
}

fn capability_for_query(query: &Value) -> Option<&'static str> {
    match query.get("mode").and_then(Value::as_str)? {
        "lineage_network" => match query.get("family").and_then(Value::as_str)? {
            "effect_correlation" => Some("co_dependency"),
            "expression_correlation" => Some("gene_correlation"),
            "expression_dependency" => Some("predictive_biomarkers"),
            _ => None,
        },
        "top" | "pair" => match query.get("module").and_then(Value::as_str)? {
            "damaging_mutation_dependency" => Some("damaging_mutation_dependency"),
            "custom_missense_mutation_dependency" => Some("annotated_mutation_dependency"),
            "hotspot_mutation_dependency" => Some("mutation_dependency_official_gene_effect_v2"),
            _ => None,
        },
        "lineage_drug" => Some("prism_drug_sensitivity"),
        _ => None,
    }
}

fn is_coverage_gap(result: &Value) -> bool {
    matches!(
        result.get("status").and_then(Value::as_str),
        Some("NOT_COMPUTED" | "COVERAGE_GAP" | "MODULE_UNAVAILABLE")
    )
}

pub(crate) fn new_analysis_proposal(
    query: &Value,
    result: &Value,
    inspector: &dyn DatasetInspector,
) -> Option<Value> {
    if !is_coverage_gap(result) {
        return None;
    }
    let capability_id = capability_for_query(query)?;
    let manifest: CapabilityManifest = serde_json::from_str(CAPABILITY_MANIFEST)
        .expect("bundled DepMap capability manifest must be valid JSON");
    let capability = manifest
        .capabilities
        .iter()
        .find(|capability| capability.id == capability_id)?;
    let datasets: HashMap<&str, &Dataset> = manifest
        .datasets
        .iter()
        .map(|dataset| (dataset.id.as_str(), dataset))
        .collect();
    let mut required = Vec::new();
    let mut missing_derived = Vec::new();
    let mut missing_inputs = Vec::new();
    let mut visited = HashSet::new();

    fn visit<'a>(
        id: &'a str,
        datasets: &HashMap<&'a str, &'a Dataset>,
        inspector: &dyn DatasetInspector,
        visited: &mut HashSet<&'a str>,
        required: &mut Vec<&'a str>,
        missing_derived: &mut Vec<&'a str>,
        missing_inputs: &mut Vec<&'a str>,
    ) {
        if !visited.insert(id) {
            return;
        }
        let Some(dataset) = datasets.get(id).copied() else {
            return;
        };
        required.push(id);
        if inspector.exists(&dataset.path) {
            return;
        }
        if dataset.kind == "derived" {
            missing_derived.push(id);
            for source in &dataset.source_inputs {
                visit(
                    source,
                    datasets,
                    inspector,
                    visited,
                    required,
                    missing_derived,
                    missing_inputs,
                );
            }
        } else {
            missing_inputs.push(id);
        }
    }

    for input in &capability.inputs {
        visit(
            input,
            &datasets,
            inspector,
            &mut visited,
            &mut required,
            &mut missing_derived,
            &mut missing_inputs,
        );
    }
    let input_status = if !missing_inputs.is_empty() {
        "missing_inputs"
    } else if !missing_derived.is_empty() {
        "preprocessing_required"
    } else {
        "ready"
    };
    Some(json!({
        "state": "new_analysis_proposed",
        "capability_id": capability.id,
        "required_dataset_ids": required,
        "input_status": input_status,
        "missing_inputs": missing_inputs,
        "missing_derived": missing_derived,
        "statistical_family": capability.analysis_label.as_deref()
            .or(capability.relation_type.as_deref())
            .unwrap_or(capability.id.as_str()),
        "requires_authorization": true,
        "authorization_policy": "A follow-up request for detail is not authorization; execute only after an explicit request matching this proposal.",
        "execution_skill": "depmap-coding-agent",
        "acquisition_skill": if input_status == "missing_inputs" { Value::String("public-data-access".into()) } else { Value::Null },
        "release": "26Q1",
        "new_analysis_started": false,
        "query_process_read_raw_data": false,
        "portal_api_is_query_provider": false
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComputeKind {
    RebuildRankings,
    FillMissingColumn,
    RecomputeTfActivity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayDecision {
    pub state: &'static str,
    pub code: &'static str,
    pub evidence_status: Option<&'static str>,
    pub gated_run: bool,
    pub ssh_used: bool,
    pub matrix_export: bool,
}

impl GatewayDecision {
    pub fn to_json(&self) -> Value {
        json!({
            "state": self.state,
            "code": self.code,
            "status": self.evidence_status,
            "gated_run": self.gated_run,
            "ssh_used": self.ssh_used,
            "matrix_export": self.matrix_export,
            "new_analysis_started": false,
            "exfiltrates": false
        })
    }
}

pub trait RemoteComputeGateway {
    fn admit(
        &self,
        kind: ComputeKind,
        knowledge_context_ready: bool,
        mcp_connected: bool,
    ) -> GatewayDecision;
}

/// Default gateway: never SSH, never export matrices, never guess folders.
#[derive(Debug, Default)]
pub struct NonExfiltratingGateway;

impl RemoteComputeGateway for NonExfiltratingGateway {
    fn admit(
        &self,
        _kind: ComputeKind,
        knowledge_context_ready: bool,
        mcp_connected: bool,
    ) -> GatewayDecision {
        if !knowledge_context_ready {
            return GatewayDecision {
                state: "blocked",
                code: "configuration_blocked",
                evidence_status: Some("MODULE_UNAVAILABLE"),
                gated_run: false,
                ssh_used: false,
                matrix_export: false,
            };
        }
        if !mcp_connected {
            return GatewayDecision {
                state: "blocked",
                code: "mcp_unavailable",
                evidence_status: Some("MODULE_UNAVAILABLE"),
                gated_run: false,
                ssh_used: false,
                matrix_export: false,
            };
        }
        GatewayDecision {
            state: "gated",
            code: "approval_required_run",
            evidence_status: Some("NOT_COMPUTED"),
            gated_run: true,
            ssh_used: false,
            matrix_export: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeInspector {
        existing: HashSet<String>,
    }

    impl DatasetInspector for FakeInspector {
        fn exists(&self, relative_path: &str) -> bool {
            self.existing.contains(relative_path)
        }
    }

    struct FakeGateway {
        ready: bool,
        mcp: bool,
    }

    impl RemoteComputeGateway for FakeGateway {
        fn admit(
            &self,
            kind: ComputeKind,
            knowledge_context_ready: bool,
            mcp_connected: bool,
        ) -> GatewayDecision {
            NonExfiltratingGateway.admit(
                kind,
                knowledge_context_ready && self.ready,
                mcp_connected && self.mcp,
            )
        }
    }

    #[test]
    fn missing_knowledge_context_is_typed_blocked_not_folder_guessing() {
        let gateway = FakeGateway {
            ready: false,
            mcp: true,
        };
        let decision = gateway.admit(ComputeKind::RebuildRankings, true, true);
        assert_eq!(decision.code, "configuration_blocked");
        assert_eq!(decision.evidence_status, Some("MODULE_UNAVAILABLE"));
        assert!(!decision.ssh_used);
        assert!(!decision.gated_run);
        let payload = decision.to_json();
        assert_eq!(payload["exfiltrates"], false);
        assert_eq!(payload["new_analysis_started"], false);
    }

    #[test]
    fn mcp_dropout_is_module_unavailable() {
        let gateway = FakeGateway {
            ready: true,
            mcp: false,
        };
        let decision = gateway.admit(ComputeKind::RecomputeTfActivity, true, true);
        assert_eq!(decision.code, "mcp_unavailable");
        assert_eq!(decision.evidence_status, Some("MODULE_UNAVAILABLE"));
        assert!(!decision.ssh_used);
    }

    #[test]
    fn ready_context_admits_a_gated_run_without_ssh_or_export() {
        let gateway = FakeGateway {
            ready: true,
            mcp: true,
        };
        let decision = gateway.admit(ComputeKind::FillMissingColumn, true, true);
        assert!(decision.gated_run);
        assert!(!decision.ssh_used);
        assert!(!decision.matrix_export);
        assert_eq!(decision.evidence_status, Some("NOT_COMPUTED"));
    }

    #[test]
    fn coverage_gap_proposes_manifest_capability_without_starting_analysis() {
        let inspector = FakeInspector {
            existing: ["data/CRISPRGeneEffect.csv".to_string()]
                .into_iter()
                .collect(),
        };
        let proposal = new_analysis_proposal(
            &json!({"mode":"lineage_network","family":"effect_correlation"}),
            &json!({"status":"NOT_COMPUTED"}),
            &inspector,
        )
        .unwrap();
        assert_eq!(proposal["capability_id"], "co_dependency");
        assert_eq!(proposal["input_status"], "preprocessing_required");
        assert_eq!(proposal["requires_authorization"], true);
        assert_eq!(proposal["new_analysis_started"], false);
        assert_eq!(proposal["query_process_read_raw_data"], false);
    }

    #[test]
    fn missing_raw_input_points_to_public_data_without_download_or_scan() {
        let inspector = FakeInspector {
            existing: HashSet::new(),
        };
        let proposal = new_analysis_proposal(
            &json!({"mode":"top","module":"damaging_mutation_dependency"}),
            &json!({"status":"COVERAGE_GAP"}),
            &inspector,
        )
        .unwrap();
        assert_eq!(proposal["input_status"], "missing_inputs");
        assert_eq!(proposal["acquisition_skill"], "public-data-access");
        assert!(proposal["missing_inputs"]
            .as_array()
            .is_some_and(|ids| !ids.is_empty()));
        assert_eq!(proposal["portal_api_is_query_provider"], false);
    }

    #[test]
    fn found_evidence_does_not_propose_recomputation() {
        let inspector = FakeInspector {
            existing: HashSet::new(),
        };
        assert!(new_analysis_proposal(
            &json!({"mode":"lineage_network","family":"effect_correlation"}),
            &json!({"status":"FOUND"}),
            &inspector,
        )
        .is_none());
    }

    #[test]
    fn project_inspector_honors_configured_data_root() {
        let root = std::env::temp_dir().join(format!(
            "wisp-depmap-proposal-inspector-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(root.join(".wisp")).unwrap();
        std::fs::create_dir_all(root.join("release-files")).unwrap();
        std::fs::write(
            root.join("release-files/CRISPRGeneEffect.csv"),
            b"ModelID\n",
        )
        .unwrap();
        std::fs::write(
            root.join(".wisp/depmap-agent.json"),
            br#"{"data_root":"release-files"}"#,
        )
        .unwrap();
        let inspector = ProjectDatasetInspector::new(&root);
        assert!(inspector.exists("data/CRISPRGeneEffect.csv"));
        std::fs::remove_dir_all(root).ok();
    }
}

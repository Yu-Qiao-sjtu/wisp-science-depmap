//! Versioned Specialist/Agent manifest assembly.
//!
//! Host capabilities and user policy always restrict the manifest; the
//! manifest cannot grant authority, credentials, or plugins.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecialistManifest {
    pub schema_version: u32,
    pub manifest_version: String,
    pub id: String,
    pub display_name: String,
    pub instructions_resource: String,
    pub required_skills: Vec<String>,
    #[serde(default)]
    pub optional_skills: Vec<String>,
    #[serde(default)]
    pub native_tool_sets: Vec<String>,
    #[serde(default)]
    pub connector_capabilities: Vec<String>,
    #[serde(default)]
    pub supported_output_contracts: Vec<String>,
    #[serde(default)]
    pub evaluation_suites: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostPolicy {
    pub available_skills: BTreeSet<String>,
    pub available_connectors: BTreeSet<String>,
    pub available_tool_sets: BTreeSet<String>,
}

impl HostPolicy {
    pub fn bundled_depmap() -> Self {
        Self {
            available_skills: ["depmap-knowledge-query", "depmap-coding-agent"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            available_connectors: ["depmap_mcp".to_string()].into_iter().collect(),
            available_tool_sets: ["scientific_query", "runs"]
                .into_iter()
                .map(str::to_string)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedSpecialistSnapshot {
    pub id: String,
    pub manifest_version: String,
    pub display_name: String,
    pub instructions_digest: String,
    pub required_skills: Vec<String>,
    pub connectors: Vec<String>,
    pub native_tool_sets: Vec<String>,
    pub output_contracts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssemblyError {
    IncompatibleSchema { found: u32 },
    MissingSkill { skill: String },
    MissingConnector { connector: String },
    MissingToolSet { tool_set: String },
    AmbiguousConnector { connector: String },
}

impl std::fmt::Display for AssemblyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IncompatibleSchema { found } => {
                write!(
                    f,
                    "specialist manifest schema {found} is incompatible with runtime {MANIFEST_SCHEMA_VERSION}"
                )
            }
            Self::MissingSkill { skill } => {
                write!(f, "required skill `{skill}` is not available on this host")
            }
            Self::MissingConnector { connector } => {
                write!(
                    f,
                    "required connector `{connector}` is not available on this host"
                )
            }
            Self::MissingToolSet { tool_set } => {
                write!(
                    f,
                    "required native tool set `{tool_set}` is not available on this host"
                )
            }
            Self::AmbiguousConnector { connector } => {
                write!(f, "connector `{connector}` has an ambiguous host binding")
            }
        }
    }
}

pub fn load_depmap_manifest() -> SpecialistManifest {
    serde_json::from_str(include_str!("../../../specialists/depmap_r_agent.v1.json"))
        .expect("compiled DepMap specialist manifest must be valid JSON")
}

pub fn load_depmap_intent_catalog() -> crate::scientific_intent::IntentCatalog {
    crate::scientific_intent::IntentCatalog::bundled_depmap()
}

pub fn identity_digest(manifest: &SpecialistManifest) -> String {
    let mut hasher = Sha256::new();
    hasher.update(manifest.id.as_bytes());
    hasher.update(b"@");
    hasher.update(manifest.manifest_version.as_bytes());
    hasher.update(b":");
    hasher.update(manifest.instructions_resource.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn assemble(
    manifest: &SpecialistManifest,
    host: &HostPolicy,
) -> Result<ResolvedSpecialistSnapshot, AssemblyError> {
    if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
        return Err(AssemblyError::IncompatibleSchema {
            found: manifest.schema_version,
        });
    }
    for skill in &manifest.required_skills {
        if !host.available_skills.contains(skill) {
            return Err(AssemblyError::MissingSkill {
                skill: skill.clone(),
            });
        }
    }
    let mut connectors = Vec::new();
    for connector in &manifest.connector_capabilities {
        let matches: Vec<_> = host
            .available_connectors
            .iter()
            .filter(|item| *item == connector || item.ends_with(&format!("/{connector}")))
            .cloned()
            .collect();
        if matches.len() > 1 {
            return Err(AssemblyError::AmbiguousConnector {
                connector: connector.clone(),
            });
        }
        if matches.is_empty() {
            return Err(AssemblyError::MissingConnector {
                connector: connector.clone(),
            });
        }
        connectors.push(matches.into_iter().next().expect("checked length"));
    }
    let mut tool_sets = Vec::new();
    for tool_set in &manifest.native_tool_sets {
        if !host.available_tool_sets.contains(tool_set) {
            return Err(AssemblyError::MissingToolSet {
                tool_set: tool_set.clone(),
            });
        }
        tool_sets.push(tool_set.clone());
    }
    Ok(ResolvedSpecialistSnapshot {
        id: manifest.id.clone(),
        manifest_version: manifest.manifest_version.clone(),
        display_name: manifest.display_name.clone(),
        instructions_digest: identity_digest(manifest),
        required_skills: manifest.required_skills.clone(),
        connectors,
        native_tool_sets: tool_sets,
        output_contracts: manifest.supported_output_contracts.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_host_assembles_the_depmap_manifest() {
        let snapshot = assemble(&load_depmap_manifest(), &HostPolicy::bundled_depmap()).unwrap();
        assert_eq!(snapshot.id, "depmap_r_agent");
        assert_eq!(snapshot.manifest_version, "1.0.0");
        assert_eq!(snapshot.instructions_digest.len(), 64);
        assert_eq!(
            snapshot.required_skills,
            vec!["depmap-knowledge-query", "depmap-coding-agent"]
        );
    }

    #[test]
    fn bundled_host_loads_the_versioned_intent_catalog() {
        let catalog = load_depmap_intent_catalog();
        assert_eq!(
            catalog.schema_version,
            crate::scientific_intent::INTENT_SCHEMA_VERSION
        );
        assert_eq!(catalog.id, "depmap_r_agent.intent");
        assert!(catalog
            .capabilities
            .iter()
            .any(|spec| spec.id == "codependency_evidence"));
    }

    #[test]
    fn reduced_host_cannot_gain_authority_from_the_manifest() {
        let mut host = HostPolicy::bundled_depmap();
        host.available_skills.remove("depmap-coding-agent");
        match assemble(&load_depmap_manifest(), &host) {
            Err(AssemblyError::MissingSkill { skill }) => {
                assert_eq!(skill, "depmap-coding-agent")
            }
            other => panic!("{other:?}"),
        }
    }
}

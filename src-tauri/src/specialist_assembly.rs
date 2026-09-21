//! Versioned Specialist/Agent manifest assembly (#100).
//!
//! The manifest describes identity and required Skills. Host capabilities and
//! user policy always restrict it; the manifest cannot grant authority.

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
                write!(f, "specialist manifest schema {found} is incompatible with runtime {MANIFEST_SCHEMA_VERSION}")
            }
            Self::MissingSkill { skill } => {
                write!(f, "required skill `{skill}` is not available on this host")
            }
            Self::MissingConnector { connector } => {
                write!(f, "required connector `{connector}` is not available on this host")
            }
            Self::MissingToolSet { tool_set } => {
                write!(f, "required native tool set `{tool_set}` is not available on this host")
            }
            Self::AmbiguousConnector { connector } => {
                write!(f, "connector `{connector}` has an ambiguous host binding")
            }
        }
    }
}

pub fn load_depmap_manifest() -> SpecialistManifest {
    serde_json::from_str(include_str!("../../specialists/depmap_r_agent.v1.json"))
        .expect("compiled DepMap specialist manifest must be valid JSON")
}

pub fn instructions_digest(instructions: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(instructions.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn assemble(
    manifest: &SpecialistManifest,
    host: &HostPolicy,
    instructions: &str,
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
        instructions_digest: instructions_digest(instructions),
        required_skills: manifest.required_skills.clone(),
        connectors,
        native_tool_sets: tool_sets,
        output_contracts: manifest.supported_output_contracts.clone(),
    })
}

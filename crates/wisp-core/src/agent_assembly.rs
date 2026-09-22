//! Shared production/evaluation Agent assembly surface.
//!
//! Hosts still own concrete providers, tools, approvals, and persistence. This
//! module captures the policy-relevant surface after host wiring so desktop,
//! CLI, and evaluation use the same Specialist identity, instruction contract,
//! tool schemas, MCP projection, and context policy.

use crate::scientific_intent::validate_discovered_schema;
use crate::specialist_manifest::{
    assemble, load_depmap_manifest, AssemblyError, HostPolicy, ResolvedSpecialistSnapshot,
};
use crate::ContextManager;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use wisp_tools::Registry;

pub const AGENT_ASSEMBLY_CONTRACT: &str = "wisp.agent-assembly.v1";
pub const DEPMAP_ASSEMBLY_SECTION_MARKER: &str = "<!-- wisp:depmap-agent-assembly:v1:begin -->";
const DEPMAP_ASSEMBLY_SECTION_END: &str = "<!-- wisp:depmap-agent-assembly:v1:end -->";

/// The compact instruction subset that must be identical in desktop and eval.
/// The desktop Specialist may add richer scientific guidance around it.
pub const DEPMAP_PRODUCTION_INSTRUCTIONS: &str = "For every new DepMap request, use the typed host routing contract before scientific dispatch. Treat bounded tool results and durable Evidence/Run/Artifact records as evidence; never treat routing records, prompt text, Skill instructions, or model memory as scientific evidence. Preserve release, scope, metric direction, sample counts, coverage state, and approval boundaries. A coverage or bridge failure is not a biological negative, and interpretation or hypothesis must not masquerade as a measured claim.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentContextPolicy {
    pub max_context_tokens: usize,
    pub max_rounds: usize,
    pub auto_compact: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentApprovalPolicy {
    /// The host enforces approval immediately before dispatch.
    pub host_enforced: bool,
    pub plan_mode: bool,
    /// Every exact or virtual tool name that can reach the approval gate.
    pub approval_tools: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentAssemblySurface {
    pub contract: String,
    pub specialist: ResolvedSpecialistSnapshot,
    pub instruction_contract: String,
    pub instruction_digest: String,
    pub enabled_skills: Vec<String>,
    /// Exact eager schemas exposed to the model request.
    pub tool_schemas: BTreeMap<String, Value>,
    /// Exact deferred MCP targets hidden behind search_mcp_tools/use_mcp_tool.
    pub mcp_projection: Vec<String>,
    pub mcp_tool_schemas: BTreeMap<String, Value>,
    pub approvals: AgentApprovalPolicy,
    pub context: AgentContextPolicy,
}

impl AgentAssemblySurface {
    pub fn tool_schema(&self, name: &str) -> Option<&Value> {
        self.tool_schemas
            .get(name)
            .or_else(|| self.mcp_tool_schemas.get(name))
            .and_then(|schema| schema.pointer("/function/parameters"))
    }

    /// Use the same schema validator as the production guardrail path. Tests
    /// can therefore compare desktop/eval failures without dispatching a tool.
    pub fn validate_tool_arguments(&self, name: &str, arguments: &Value) -> Result<(), String> {
        let schema = self
            .tool_schema(name)
            .ok_or_else(|| format!("tool '{name}' is not exposed by this Agent assembly"))?;
        validate_discovered_schema(schema, arguments)
    }

    /// Stable digest of the exact schema advertised by this assembly. JSON
    /// object keys are sorted recursively so serialization order cannot make
    /// production/eval parity appear to drift.
    pub fn tool_schema_digest(&self, name: &str) -> Option<String> {
        self.tool_schemas
            .get(name)
            .or_else(|| self.mcp_tool_schemas.get(name))
            .map(canonical_json_digest)
    }
}

pub fn assemble_depmap_agent_surface(
    host: &HostPolicy,
    registry: &Registry,
    context: AgentContextPolicy,
    plan_mode: bool,
) -> Result<AgentAssemblySurface, AssemblyError> {
    let specialist = assemble(&load_depmap_manifest(), host)?;
    let tool_schemas = registry
        .schemas()
        .into_iter()
        .map(|schema| {
            let name = schema.function.name.clone();
            let value =
                serde_json::to_value(schema).expect("model-facing tool schema is serializable");
            (name, value)
        })
        .collect();
    let mut mcp_projection = registry
        .names()
        .into_iter()
        .filter(|name| registry.get(name).is_some_and(|tool| tool.defer_schema()))
        .map(str::to_string)
        .collect::<Vec<_>>();
    mcp_projection.sort();
    let mcp_tool_schemas = mcp_projection
        .iter()
        .filter_map(|name| {
            registry.get(name).map(|tool| {
                let value = serde_json::to_value(tool.schema())
                    .expect("deferred model-facing tool schema is serializable");
                (name.clone(), value)
            })
        })
        .collect();
    let mut approval_tools = registry.approval_names().into_iter().collect::<Vec<_>>();
    approval_tools.sort();
    let instruction_digest = sha256_hex(DEPMAP_PRODUCTION_INSTRUCTIONS.as_bytes());
    Ok(AgentAssemblySurface {
        contract: AGENT_ASSEMBLY_CONTRACT.into(),
        enabled_skills: specialist.required_skills.clone(),
        specialist,
        instruction_contract: DEPMAP_PRODUCTION_INSTRUCTIONS.into(),
        instruction_digest,
        tool_schemas,
        mcp_projection,
        mcp_tool_schemas,
        approvals: AgentApprovalPolicy {
            host_enforced: true,
            plan_mode,
            approval_tools,
        },
        context,
    })
}

/// Assemble and apply the DepMap Agent through one entry point shared by the
/// desktop host and evaluator. Returning the complete surface lets hosts audit
/// or persist every parity-relevant field instead of checking prompt text only.
pub fn assemble_and_apply_depmap_agent(
    ctx: &mut ContextManager,
    host: &HostPolicy,
    registry: &Registry,
    context: AgentContextPolicy,
    plan_mode: bool,
) -> Result<AgentAssemblySurface, AssemblyError> {
    let surface = assemble_depmap_agent_surface(host, registry, context, plan_mode)?;
    apply_agent_assembly(ctx, &surface);
    Ok(surface)
}

/// Apply the shared Specialist contract exactly once to the first system
/// message. Both production desktop turns and the evaluator call this helper.
pub fn apply_agent_assembly(ctx: &mut ContextManager, surface: &AgentAssemblySurface) {
    let Some(message) = ctx.messages.first_mut() else {
        return;
    };
    let wisp_llm::Content::Text(prompt) = &mut message.content else {
        return;
    };
    let section = format!(
        "{DEPMAP_ASSEMBLY_SECTION_MARKER}\nSpecialist: {}@{}\n{}\n{DEPMAP_ASSEMBLY_SECTION_END}",
        surface.specialist.id, surface.specialist.manifest_version, surface.instruction_contract,
    );
    // Project instructions may legitimately document the public marker. Only
    // a complete generated block at the end proves that assembly was applied.
    if prompt.ends_with(&section) {
        return;
    }
    remove_generated_assembly_sections(prompt);
    prompt.push_str("\n\n");
    prompt.push_str(&section);
}

fn remove_generated_assembly_sections(prompt: &mut String) {
    let mut cursor = 0;
    while let Some(relative_start) = prompt[cursor..].find(DEPMAP_ASSEMBLY_SECTION_MARKER) {
        let start = cursor + relative_start;
        let body_start = start + DEPMAP_ASSEMBLY_SECTION_MARKER.len();
        if !prompt[body_start..].starts_with("\nSpecialist: ") {
            // A lone marker can be project documentation, not generated state.
            cursor = body_start;
            continue;
        }
        let Some(relative_end) = prompt[body_start..].find(DEPMAP_ASSEMBLY_SECTION_END) else {
            break;
        };
        let end = body_start + relative_end + DEPMAP_ASSEMBLY_SECTION_END.len();
        prompt.replace_range(start..end, "");
        cursor = start;
    }
    let trimmed_len = prompt.trim_end().len();
    prompt.truncate(trimmed_len);
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn canonical_json_digest(value: &Value) -> String {
    fn canonicalize(value: &Value) -> Value {
        match value {
            Value::Array(values) => Value::Array(values.iter().map(canonicalize).collect()),
            Value::Object(values) => {
                let sorted = values
                    .iter()
                    .map(|(key, value)| (key.clone(), canonicalize(value)))
                    .collect::<BTreeMap<_, _>>();
                serde_json::to_value(sorted).expect("canonical JSON map is serializable")
            }
            value => value.clone(),
        }
    }

    let encoded = serde_json::to_vec(&canonicalize(value))
        .expect("Agent assembly schema is JSON serializable");
    sha256_hex(&encoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;
    use wisp_tools::{Tool, ToolEnv, ToolResult};

    struct FixtureMcp;

    #[async_trait::async_trait]
    impl Tool for FixtureMcp {
        fn name(&self) -> &str {
            "depmap_fixture"
        }

        fn schema(&self) -> wisp_llm::ToolSchema {
            wisp_llm::ToolSchema::new(
                self.name(),
                "fixture",
                json!({
                    "type": "object",
                    "properties": {"gene": {"type": "string"}},
                    "required": ["gene"],
                    "additionalProperties": false
                }),
            )
        }

        fn defer_schema(&self) -> bool {
            true
        }

        fn read_only(&self) -> bool {
            true
        }

        async fn run(&self, _args: &Value, _env: &dyn ToolEnv) -> ToolResult {
            ToolResult::ok("fixture")
        }
    }

    fn registry() -> Registry {
        let skills = Arc::new(wisp_skills::SkillIndex::default());
        let memory = Arc::new(crate::MemoryManager::new(std::path::Path::new(".")));
        let mut registry = crate::build_registry(skills, memory, false);
        registry.add(Box::new(FixtureMcp));
        crate::install_scientific_intent_planner_in(&mut registry, "project", "frame");
        registry
    }

    #[test]
    fn desktop_and_eval_surfaces_share_instructions_and_schema_failures() {
        let desktop = assemble_depmap_agent_surface(
            &HostPolicy::bundled_depmap(),
            &registry(),
            AgentContextPolicy {
                max_context_tokens: 128_000,
                max_rounds: 12,
                auto_compact: true,
            },
            false,
        )
        .unwrap();
        let eval = assemble_depmap_agent_surface(
            &HostPolicy::bundled_depmap(),
            &registry(),
            AgentContextPolicy {
                max_context_tokens: 128_000,
                max_rounds: 12,
                auto_compact: true,
            },
            false,
        )
        .unwrap();
        assert_eq!(desktop.specialist, eval.specialist);
        assert_eq!(desktop.instruction_contract, eval.instruction_contract);
        assert_eq!(desktop.tool_schemas, eval.tool_schemas);
        assert_eq!(desktop.mcp_projection, vec!["depmap_fixture"]);
        assert_eq!(desktop.mcp_tool_schemas, eval.mcp_tool_schemas);
        let invalid = json!({});
        assert_eq!(
            desktop.validate_tool_arguments("depmap_fixture", &invalid),
            eval.validate_tool_arguments("depmap_fixture", &invalid)
        );
        assert!(desktop
            .validate_tool_arguments("depmap_fixture", &invalid)
            .is_err());
        assert_eq!(
            desktop.tool_schema_digest("depmap_fixture"),
            eval.tool_schema_digest("depmap_fixture")
        );
    }

    #[test]
    fn schema_digest_is_stable_across_object_key_order() {
        let left = json!({"type": "object", "properties": {"b": {"type": "string"}, "a": {"type": "integer"}}});
        let right = json!({"properties": {"a": {"type": "integer"}, "b": {"type": "string"}}, "type": "object"});
        assert_eq!(canonical_json_digest(&left), canonical_json_digest(&right));
        let changed_description = json!({
            "type": "function",
            "function": {
                "name": "fixture",
                "description": "changed",
                "parameters": left,
            }
        });
        let original_description = json!({
            "type": "function",
            "function": {
                "name": "fixture",
                "description": "original",
                "parameters": right,
            }
        });
        assert_ne!(
            canonical_json_digest(&changed_description),
            canonical_json_digest(&original_description)
        );
    }

    #[test]
    fn assembly_section_is_idempotent() {
        let surface = assemble_depmap_agent_surface(
            &HostPolicy::bundled_depmap(),
            &registry(),
            AgentContextPolicy {
                max_context_tokens: 4_096,
                max_rounds: 4,
                auto_compact: false,
            },
            false,
        )
        .unwrap();
        let mut ctx = ContextManager::new(4_096);
        ctx.append_system("base");
        apply_agent_assembly(&mut ctx, &surface);
        apply_agent_assembly(&mut ctx, &surface);
        let prompt = ctx.messages[0].content.as_text();
        assert_eq!(prompt.matches(DEPMAP_ASSEMBLY_SECTION_MARKER).count(), 1);
    }

    #[test]
    fn documented_marker_does_not_suppress_generated_assembly() {
        let surface = assemble_depmap_agent_surface(
            &HostPolicy::bundled_depmap(),
            &registry(),
            AgentContextPolicy {
                max_context_tokens: 4_096,
                max_rounds: 4,
                auto_compact: false,
            },
            false,
        )
        .unwrap();
        let mut ctx = ContextManager::new(4_096);
        ctx.append_system(format!(
            "Project docs mention {DEPMAP_ASSEMBLY_SECTION_MARKER} without assembling anything."
        ));
        apply_agent_assembly(&mut ctx, &surface);
        let prompt = ctx.messages[0].content.as_text();
        assert_eq!(prompt.matches(DEPMAP_ASSEMBLY_SECTION_MARKER).count(), 2);
        assert!(prompt.ends_with(DEPMAP_ASSEMBLY_SECTION_END));
        assert!(prompt.contains(&surface.instruction_contract));
    }

    #[test]
    fn stale_generated_assembly_is_replaced() {
        let mut old = assemble_depmap_agent_surface(
            &HostPolicy::bundled_depmap(),
            &registry(),
            AgentContextPolicy {
                max_context_tokens: 4_096,
                max_rounds: 4,
                auto_compact: false,
            },
            false,
        )
        .unwrap();
        old.instruction_contract = "old assembly instructions".into();
        let mut current = old.clone();
        current.instruction_contract = "current assembly instructions".into();
        let mut ctx = ContextManager::new(4_096);
        ctx.append_system("base");
        apply_agent_assembly(&mut ctx, &old);
        apply_agent_assembly(&mut ctx, &current);
        let prompt = ctx.messages[0].content.as_text();
        assert_eq!(prompt.matches(DEPMAP_ASSEMBLY_SECTION_MARKER).count(), 1);
        assert!(!prompt.contains(&old.instruction_contract));
        assert!(prompt.contains(&current.instruction_contract));
        assert!(prompt.ends_with(DEPMAP_ASSEMBLY_SECTION_END));
    }
}

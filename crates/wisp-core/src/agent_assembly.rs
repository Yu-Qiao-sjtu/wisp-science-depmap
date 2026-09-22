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
        self.tool_schemas.get(name)
    }

    /// Use the same schema validator as the production guardrail path. Tests
    /// can therefore compare desktop/eval failures without dispatching a tool.
    pub fn validate_tool_arguments(&self, name: &str, arguments: &Value) -> Result<(), String> {
        let schema = self
            .tool_schema(name)
            .or_else(|| self.mcp_tool_schemas.get(name))
            .ok_or_else(|| format!("tool '{name}' is not exposed by this Agent assembly"))?;
        validate_discovered_schema(schema, arguments)
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
        .map(|schema| (schema.function.name, schema.function.parameters))
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
            registry
                .get(name)
                .map(|tool| (name.clone(), tool.schema().function.parameters))
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
    prompt.push_str("\n\n");
    prompt.push_str(&section);
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
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
}

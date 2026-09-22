//! The `Tool` trait every built-in or MCP-backed tool implements.

use crate::{
    env::{Approval, ToolEnv, ToolResult},
    execution::{CacheableToolResult, ToolExecutionPolicy},
};
use async_trait::async_trait;
use serde_json::Value;
use std::{future::Future, pin::Pin};
use wisp_llm::ToolSchema;

pub type ToolCompletion = Pin<Box<dyn Future<Output = ToolResult> + Send + 'static>>;

/// Result of a coordinated tool call. A detached completion keeps execution
/// capacity occupied after a caller stops waiting for remote work that cannot
/// be cancelled safely.
pub struct ToolRunOutcome {
    pub result: ToolResult,
    pub detached_completion: Option<ToolCompletion>,
}

impl ToolRunOutcome {
    pub fn complete(result: ToolResult) -> Self {
        Self {
            result,
            detached_completion: None,
        }
    }

    pub fn detached(result: ToolResult, completion: ToolCompletion) -> Self {
        Self {
            result,
            detached_completion: Some(completion),
        }
    }
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn schema(&self) -> ToolSchema;
    /// Keep this tool callable but omit its schema from ordinary model requests.
    /// Deferred tools are discovered through the registry's MCP search/dispatch pair.
    fn defer_schema(&self) -> bool {
        false
    }
    /// Minimum approval required even when the host has no persisted rule for
    /// this tool. Third-party plugin tools use `Ask`; built-ins default to
    /// `Allow`. An explicit host `Deny` always wins.
    fn minimum_approval(&self) -> Approval {
        Approval::Allow
    }
    /// Whether this tool only retrieves — no writes, no state changes, nothing
    /// to undo. Read-only tools stay callable in plan mode on top of
    /// [`crate::PLAN_MODE_READ_ONLY`], which cannot name tools it never sees
    /// (every MCP-backed retrieval tool). Default `false`: a tool nobody
    /// classified is refused while planning.
    fn read_only(&self) -> bool {
        false
    }
    /// Stable connector identity for MCP-backed tools. Native tools return
    /// `None`. Host policy uses this to grant an exact remote server rather
    /// than trusting a tool-name prefix that another connector could spoof.
    fn connector_id(&self) -> Option<&str> {
        None
    }
    /// Non-secret revision of the connector credential/account used by this
    /// tool. Connector-backed caching is disabled when this is absent.
    fn cache_authorization_revision(&self) -> Option<&str> {
        None
    }
    /// Complete, non-secret remote contract snapshot that defines cached
    /// result semantics. Connector implementations should include output
    /// schemas and all other server metadata that can change interpretation.
    fn cache_contract(&self) -> Option<Value> {
        None
    }
    /// Optional ingestion budget for this tool's textual result. The global
    /// `WISP_TOOL_RESULT_BUDGET` override still wins. Tools should use this
    /// only for intentionally bounded, self-contained contracts where spilling
    /// the result would cause a more expensive or less safe read-back loop.
    fn context_result_budget(&self) -> Option<usize> {
        None
    }
    /// Execution controls are fail-closed: tools must explicitly opt a
    /// read-only, certain result into caching. Concurrency limits still apply
    /// to uncached calls.
    fn execution_policy(&self, _args: &Value) -> ToolExecutionPolicy {
        ToolExecutionPolicy::default()
    }
    /// Return the bounded structured projection that may be persisted. The
    /// default deliberately refuses to infer safety from an arbitrary result.
    fn cacheable_result(&self, _result: &ToolResult) -> Option<CacheableToolResult> {
        None
    }
    /// Revalidate live remote metadata immediately before a cache hit is
    /// returned. Native tools accept their in-process contract by default.
    async fn validate_cache_hit(&self) -> Result<(), String> {
        Ok(())
    }
    /// Revalidate a cache hit while allowing remote implementations to detach
    /// provider work when this caller stops waiting. Detached validation keeps
    /// its execution permit until the provider request actually completes.
    async fn validate_cache_hit_coordinated(&self, _env: &dyn ToolEnv) -> ToolRunOutcome {
        match self.validate_cache_hit().await {
            Ok(()) => ToolRunOutcome::complete(ToolResult::ok("{}")),
            Err(error) => ToolRunOutcome::complete(ToolResult::fail(error)),
        }
    }
    /// One-line preview shown in the tool-call card (e.g. the file path).
    fn preview(&self, _args: &Value) -> String {
        String::new()
    }
    /// Hook fired before `run` (e.g. `edit` emits a unified diff here).
    async fn before(&self, _args: &Value, _env: &dyn ToolEnv) {}
    /// Coordinators use this hook so a remote operation may return promptly to
    /// its cancelled caller while retaining its concurrency lease until the
    /// non-cancellable provider work actually finishes.
    async fn run_coordinated(&self, args: &Value, env: &dyn ToolEnv) -> ToolRunOutcome {
        ToolRunOutcome::complete(self.run(args, env).await)
    }
    async fn run(&self, args: &Value, env: &dyn ToolEnv) -> ToolResult;
}

/// Pull a string argument, or fail with a clear message.
pub fn arg_str(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| format!("missing required argument '{}'", key))
}

/// Pull an optional string argument.
pub fn arg_str_opt(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(|v| v.as_str()).map(String::from)
}

/// Pull an optional integer argument.
pub fn arg_int_opt(args: &Value, key: &str) -> Option<i64> {
    args.get(key).and_then(|v| v.as_i64())
}

/// Pull an optional bool argument.
pub fn arg_bool_opt(args: &Value, key: &str) -> Option<bool> {
    args.get(key).and_then(|v| v.as_bool())
}

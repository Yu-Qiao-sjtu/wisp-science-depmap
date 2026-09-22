//! Shared execution coordination for tools.
//!
//! Caching is deliberately opt-in and stores only a tool-provided structured
//! projection. Backpressure is always active, including for mutating tools.

use crate::{
    Approval, ConfirmDecision, Tool, ToolControl, ToolEnv, ToolEvent, ToolResourceLease, ToolResult,
};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, VecDeque},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Notify;

const CACHE_FORMAT: u32 = 1;
const DEFAULT_MEMORY_ENTRIES: usize = 256;
const DEFAULT_MEMORY_BYTES: usize = 8 * 1024 * 1024;
const DEFAULT_DURABLE_ENTRIES: usize = 512;
const DEFAULT_DURABLE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ToolCacheMode {
    #[default]
    Disabled,
    Memory,
    MemoryAndProject,
}

#[derive(Debug, Clone)]
pub struct ToolCacheContract {
    pub mode: ToolCacheMode,
    pub capability_version: String,
    pub schema_version: String,
    pub release_digest: String,
    pub index_digest: String,
    pub ttl: Duration,
    pub max_result_bytes: usize,
    /// The tool asserts that calls within the supplied authorization scope may
    /// share this result. User-specific or uncertain calls leave this false.
    pub shared_authorization: bool,
    pub certain_outcome: bool,
}

impl Default for ToolCacheContract {
    fn default() -> Self {
        Self {
            mode: ToolCacheMode::Disabled,
            capability_version: String::new(),
            schema_version: String::new(),
            release_digest: String::new(),
            index_digest: String::new(),
            ttl: Duration::from_secs(0),
            max_result_bytes: 0,
            shared_authorization: false,
            certain_outcome: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolExecutionPolicy {
    pub cache: ToolCacheContract,
    pub max_concurrency: usize,
    pub max_queue: usize,
}

impl Default for ToolExecutionPolicy {
    fn default() -> Self {
        Self {
            cache: ToolCacheContract::default(),
            max_concurrency: 4,
            max_queue: 32,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolExecutionScope {
    pub agent_identity: String,
    pub authorization_scope: String,
    pub policy_projection: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolExecutionSignal {
    Hit,
    Miss,
    Stale,
    Bypass,
    Coalesced,
    Evicted,
    Queued,
    Backpressure,
    Cancelled,
}

impl ToolExecutionSignal {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hit => "hit",
            Self::Miss => "miss",
            Self::Stale => "stale",
            Self::Bypass => "bypass",
            Self::Coalesced => "coalesced",
            Self::Evicted => "evicted",
            Self::Queued => "queued",
            Self::Backpressure => "backpressure",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolExecutionDiagnostic {
    pub signal: ToolExecutionSignal,
    /// A one-way digest only; never raw arguments, identities, or paths.
    pub fingerprint: Option<String>,
    pub queue_depth: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheableToolResult {
    pub content: String,
}

impl CacheableToolResult {
    pub fn structured(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry {
    format: u32,
    fingerprint: String,
    stored_at_unix_ms: u128,
    result: CacheableToolResult,
}

impl CacheEntry {
    fn bytes(&self) -> usize {
        self.result.content.len()
    }
}

#[derive(Default)]
struct MemoryCache {
    entries: HashMap<String, CacheEntry>,
    order: VecDeque<String>,
    bytes: usize,
}

struct Flight {
    resolution: Mutex<Option<FlightResolution>>,
    notify: Notify,
    interested_callers: AtomicUsize,
}

#[derive(Clone)]
enum FlightResolution {
    Completed(ToolResult),
    Retry,
}

impl Flight {
    fn new() -> Self {
        Self {
            resolution: Mutex::new(None),
            notify: Notify::new(),
            interested_callers: AtomicUsize::new(1),
        }
    }
}

/// A single-flight leader executes for every still-interested authorized
/// caller, not just the turn that happened to arrive first.
struct SharedFlightEnv<'a> {
    inner: &'a dyn ToolEnv,
    flight: &'a Flight,
}

#[async_trait::async_trait]
impl ToolEnv for SharedFlightEnv<'_> {
    fn project_root(&self) -> &Path {
        self.inner.project_root()
    }
    fn restrict_read_paths_to_project(&self) -> bool {
        self.inner.restrict_read_paths_to_project()
    }
    fn resolve_read_path(&self, path: &str, allow_directory: bool) -> Result<PathBuf, String> {
        self.inner.resolve_read_path(path, allow_directory)
    }
    async fn confirm(&self, message: &str) -> bool {
        self.inner.confirm(message).await
    }
    async fn confirm_decision(&self, message: &str) -> ConfirmDecision {
        self.inner.confirm_decision(message).await
    }
    async fn approval_mode(&self, tool: &str) -> Approval {
        self.inner.approval_mode(tool).await
    }
    async fn acquire_tool_resources(
        &self,
        tool: &str,
        args: &Value,
    ) -> Result<Option<ToolResourceLease>, String> {
        self.inner.acquire_tool_resources(tool, args).await
    }
    fn approval_bypass(&self) -> bool {
        self.inner.approval_bypass()
    }
    fn force_ask_mutations(&self) -> bool {
        self.inner.force_ask_mutations()
    }
    fn plan_mode(&self) -> bool {
        self.inner.plan_mode()
    }
    fn project_write_locked(&self) -> bool {
        self.inner.project_write_locked()
    }
    fn artifact_requested(&self) -> bool {
        self.inner.artifact_requested()
    }
    fn danger_auto_approve(&self) -> bool {
        self.inner.danger_auto_approve()
    }
    async fn emit(&self, event: ToolEvent) {
        self.inner.emit(event).await;
    }
    fn is_cancelled(&self) -> bool {
        self.inner.is_cancelled() && self.flight.interested_callers.load(Ordering::SeqCst) <= 1
    }
    fn caller_cancelled(&self) -> bool {
        self.inner.is_cancelled()
    }
    fn guidance_pending(&self) -> bool {
        self.inner.guidance_pending()
    }
    fn cancel_flag(&self) -> Option<&std::sync::atomic::AtomicBool> {
        None
    }
    async fn preflight_local_execution(&self, source: &str) -> Result<(), String> {
        self.inner.preflight_local_execution(source).await
    }
    async fn preflight_shell(&self, cmd: &str) -> Result<(), String> {
        self.inner.preflight_shell(cmd).await
    }
    fn note_shell_outcome(&self, cmd: &str, success: bool, detail: &str) {
        self.inner.note_shell_outcome(cmd, success, detail);
    }
    fn report_written_paths(&self, paths: &[String]) {
        self.inner.report_written_paths(paths);
    }
    fn turn_id(&self) -> Option<&str> {
        self.inner.turn_id()
    }
    fn frame_id(&self) -> Option<&str> {
        self.inner.frame_id()
    }
    fn project_id(&self) -> Option<&str> {
        self.inner.project_id()
    }
    fn tool_execution_scope(&self) -> ToolExecutionScope {
        self.inner.tool_execution_scope()
    }
    fn note_tool_execution(&self, event: &ToolExecutionDiagnostic) {
        self.inner.note_tool_execution(event);
    }
}

#[derive(Default)]
struct GateCounts {
    active: usize,
    queued: usize,
}

struct Gate {
    counts: Mutex<GateCounts>,
    notify: Notify,
}

impl Gate {
    fn new() -> Self {
        Self {
            counts: Mutex::new(GateCounts::default()),
            notify: Notify::new(),
        }
    }
}

struct GatePermit {
    gate: Arc<Gate>,
}

enum PermitAcquireError {
    CallerCancelled(ToolResult),
    Rejected(ToolResult),
}

impl PermitAcquireError {
    fn into_result(self) -> ToolResult {
        match self {
            Self::CallerCancelled(result) | Self::Rejected(result) => result,
        }
    }
}

enum LimitedRunOutcome {
    Started(crate::ToolRunOutcome),
    CallerCancelled(ToolResult),
}

impl Drop for GatePermit {
    fn drop(&mut self) {
        let mut counts = lock(&self.gate.counts);
        counts.active = counts.active.saturating_sub(1);
        drop(counts);
        self.gate.notify.notify_one();
    }
}

pub struct ToolExecutionCoordinator {
    memory: Mutex<MemoryCache>,
    flights: Arc<Mutex<HashMap<String, Arc<Flight>>>>,
    gates: Mutex<HashMap<String, Arc<Gate>>>,
    max_memory_entries: usize,
    max_memory_bytes: usize,
}

impl Default for ToolExecutionCoordinator {
    fn default() -> Self {
        Self {
            memory: Mutex::new(MemoryCache::default()),
            flights: Arc::new(Mutex::new(HashMap::new())),
            gates: Mutex::new(HashMap::new()),
            max_memory_entries: DEFAULT_MEMORY_ENTRIES,
            max_memory_bytes: DEFAULT_MEMORY_BYTES,
        }
    }
}

pub(crate) fn coordinator() -> &'static ToolExecutionCoordinator {
    static COORDINATOR: OnceLock<ToolExecutionCoordinator> = OnceLock::new();
    COORDINATOR.get_or_init(ToolExecutionCoordinator::default)
}

impl ToolExecutionCoordinator {
    pub async fn execute(
        &self,
        tool: &dyn Tool,
        args: &Value,
        env: &dyn ToolEnv,
        approval_bearing: bool,
    ) -> ToolResult {
        let policy = tool.execution_policy(args);
        let scope = env.tool_execution_scope();
        let provider = tool.connector_id().unwrap_or("native");
        let cache_eligible = tool.read_only()
            && !approval_bearing
            && policy.cache.mode != ToolCacheMode::Disabled
            && policy.cache.shared_authorization
            && policy.cache.certain_outcome
            && policy.cache.ttl > Duration::ZERO
            && policy.cache.max_result_bytes > 0
            && !policy.cache.capability_version.is_empty()
            && !policy.cache.schema_version.is_empty()
            && !policy.cache.release_digest.is_empty()
            && !policy.cache.index_digest.is_empty()
            && !scope.agent_identity.is_empty()
            && !scope.authorization_scope.is_empty()
            && !scope.policy_projection.is_empty()
            && (tool.connector_id().is_none()
                || (tool
                    .cache_authorization_revision()
                    .is_some_and(|revision| !revision.is_empty())
                    && tool.cache_contract().is_some()));

        if !cache_eligible {
            emit(env, ToolExecutionSignal::Bypass, None, None);
            let outcome = match self
                .execute_limited(tool, args, env, provider, &policy)
                .await
            {
                LimitedRunOutcome::Started(outcome) => outcome,
                LimitedRunOutcome::CallerCancelled(result) => return result,
            };
            if let Some(completion) = outcome.detached_completion {
                tokio::spawn(async move {
                    let _ = completion.await;
                });
            }
            return outcome.result;
        }

        let identity = cache_identity(tool, args, provider, &policy.cache, &scope);
        match self
            .lookup_cache(env.project_root(), &identity, &policy.cache)
            .await
        {
            CacheLookup::Hit(entry) => {
                let validation_permit = match self
                    .acquire_permit(tool.name(), provider, &policy, env)
                    .await
                {
                    Ok(permit) => permit,
                    Err(error) => return error.into_result(),
                };
                let validation = tool.validate_cache_hit_coordinated(env).await;
                if let Some(completion) = validation.detached_completion {
                    tokio::spawn(async move {
                        let _ = completion.await;
                        drop(validation_permit);
                    });
                    emit(env, ToolExecutionSignal::Cancelled, None, None);
                    return typed_error("tool_wait_cancelled", tool.name(), provider, 0, 0);
                }
                drop(validation_permit);
                if !validation.result.success {
                    self.remove_memory(&identity.slot);
                    emit(
                        env,
                        ToolExecutionSignal::Stale,
                        Some(&identity.fingerprint),
                        None,
                    );
                    return ToolResult::fail(format!(
                        "cache contract revalidation failed before replay: {}",
                        validation.result.content
                    ))
                    .stop_batch();
                }
                if matches!(
                    classify_entry(entry.clone(), &identity, &policy.cache),
                    CacheLookup::Hit(_)
                ) {
                    emit(
                        env,
                        ToolExecutionSignal::Hit,
                        Some(&identity.fingerprint),
                        None,
                    );
                    return ToolResult::ok(entry.result.content);
                }
                self.remove_memory(&identity.slot);
                emit(
                    env,
                    ToolExecutionSignal::Stale,
                    Some(&identity.fingerprint),
                    None,
                );
            }
            CacheLookup::Stale => emit(
                env,
                ToolExecutionSignal::Stale,
                Some(&identity.fingerprint),
                None,
            ),
            CacheLookup::Miss => emit(
                env,
                ToolExecutionSignal::Miss,
                Some(&identity.fingerprint),
                None,
            ),
        }

        loop {
            let (flight, leader) = {
                let mut flights = lock(&self.flights);
                if let Some(existing) = flights.get(&identity.fingerprint) {
                    existing.interested_callers.fetch_add(1, Ordering::SeqCst);
                    (existing.clone(), false)
                } else {
                    let flight = Arc::new(Flight::new());
                    flights.insert(identity.fingerprint.clone(), flight.clone());
                    (flight, true)
                }
            };

            if !leader {
                emit(
                    env,
                    ToolExecutionSignal::Coalesced,
                    Some(&identity.fingerprint),
                    None,
                );
                match wait_for_flight(tool.name(), provider, env, flight).await {
                    FlightResolution::Completed(result) => return result,
                    FlightResolution::Retry => continue,
                }
            }

            let shared_env = SharedFlightEnv {
                inner: env,
                flight: flight.as_ref(),
            };
            let outcome = match self
                .execute_limited(tool, args, &shared_env, provider, &policy)
                .await
            {
                LimitedRunOutcome::Started(outcome) => outcome,
                LimitedRunOutcome::CallerCancelled(result) => {
                    let has_waiters = {
                        let mut flights = lock(&self.flights);
                        let has_waiters = flight.interested_callers.load(Ordering::SeqCst) > 1;
                        flights.remove(&identity.fingerprint);
                        has_waiters
                    };
                    let resolution = if has_waiters {
                        FlightResolution::Retry
                    } else {
                        FlightResolution::Completed(result.clone())
                    };
                    *lock(&flight.resolution) = Some(resolution);
                    flight.notify.notify_waiters();
                    return result;
                }
            };
            if let Some(completion) = outcome.detached_completion {
                let flights = Arc::clone(&self.flights);
                let fingerprint = identity.fingerprint.clone();
                let completed_flight = Arc::clone(&flight);
                tokio::spawn(async move {
                    let result = completion.await;
                    *lock(&completed_flight.resolution) = Some(FlightResolution::Completed(result));
                    lock(&flights).remove(&fingerprint);
                    completed_flight.notify.notify_waiters();
                });
                match wait_for_flight(tool.name(), provider, env, flight).await {
                    FlightResolution::Completed(result) => return result,
                    FlightResolution::Retry => continue,
                }
            }
            let result = outcome.result;
            let leader_cancelled = env.is_cancelled();
            if let Some(record) = valid_cache_projection(tool, &result, &policy.cache) {
                self.store_cache(env.project_root(), &identity, record, &policy.cache, env)
                    .await;
            } else {
                emit(
                    env,
                    ToolExecutionSignal::Bypass,
                    Some(&identity.fingerprint),
                    None,
                );
            }

            *lock(&flight.resolution) = Some(FlightResolution::Completed(result.clone()));
            lock(&self.flights).remove(&identity.fingerprint);
            flight.notify.notify_waiters();
            if leader_cancelled {
                emit(env, ToolExecutionSignal::Cancelled, None, None);
                return typed_error("tool_wait_cancelled", tool.name(), provider, 0, 0);
            }
            return result;
        }
    }

    async fn execute_limited(
        &self,
        tool: &dyn Tool,
        args: &Value,
        env: &dyn ToolEnv,
        provider: &str,
        policy: &ToolExecutionPolicy,
    ) -> LimitedRunOutcome {
        let permit = match self
            .acquire_permit(tool.name(), provider, policy, env)
            .await
        {
            Ok(permit) => permit,
            Err(PermitAcquireError::CallerCancelled(result)) => {
                return LimitedRunOutcome::CallerCancelled(result)
            }
            Err(PermitAcquireError::Rejected(result)) => {
                return LimitedRunOutcome::Started(crate::ToolRunOutcome::complete(result))
            }
        };
        let resource_lease = match env.acquire_tool_resources(tool.name(), args).await {
            Ok(lease) => lease,
            Err(error) => {
                return LimitedRunOutcome::Started(crate::ToolRunOutcome::complete(
                    ToolResult::fail(error).stop_batch(),
                ))
            }
        };
        tool.before(args, env).await;
        let outcome = tool.run_coordinated(args, env).await;
        if let Some(completion) = outcome.detached_completion {
            return LimitedRunOutcome::Started(crate::ToolRunOutcome::detached(
                outcome.result,
                Box::pin(async move {
                    let result = completion.await;
                    drop(resource_lease);
                    drop(permit);
                    result
                }),
            ));
        }
        drop(resource_lease);
        drop(permit);
        LimitedRunOutcome::Started(crate::ToolRunOutcome::complete(outcome.result))
    }

    async fn acquire_permit(
        &self,
        tool: &str,
        provider: &str,
        policy: &ToolExecutionPolicy,
        env: &dyn ToolEnv,
    ) -> Result<GatePermit, PermitAcquireError> {
        let max_concurrency = policy.max_concurrency.max(1);
        let key = format!("{provider}\0{tool}");
        let gate = {
            let mut gates = lock(&self.gates);
            gates
                .entry(key)
                .or_insert_with(|| Arc::new(Gate::new()))
                .clone()
        };
        let mut registered = false;
        loop {
            if env.caller_cancelled() {
                if registered {
                    let mut counts = lock(&gate.counts);
                    counts.queued = counts.queued.saturating_sub(1);
                }
                emit(env, ToolExecutionSignal::Cancelled, None, None);
                return Err(PermitAcquireError::CallerCancelled(typed_error(
                    "tool_execution_cancelled",
                    tool,
                    provider,
                    max_concurrency,
                    policy.max_queue,
                )));
            }
            {
                let mut counts = lock(&gate.counts);
                if counts.active < max_concurrency {
                    if registered {
                        counts.queued = counts.queued.saturating_sub(1);
                    }
                    counts.active += 1;
                    return Ok(GatePermit { gate: gate.clone() });
                }
                if !registered {
                    if counts.queued >= policy.max_queue {
                        let depth = counts.queued;
                        drop(counts);
                        emit(env, ToolExecutionSignal::Backpressure, None, Some(depth));
                        return Err(PermitAcquireError::Rejected(typed_error(
                            "tool_queue_overflow",
                            tool,
                            provider,
                            max_concurrency,
                            policy.max_queue,
                        )));
                    }
                    counts.queued += 1;
                    registered = true;
                    emit(env, ToolExecutionSignal::Queued, None, Some(counts.queued));
                }
            }
            let _ = tokio::time::timeout(Duration::from_millis(25), gate.notify.notified()).await;
        }
    }

    async fn lookup_cache(
        &self,
        project_root: &Path,
        identity: &CacheIdentity,
        contract: &ToolCacheContract,
    ) -> CacheLookup {
        if let Some(entry) = lock(&self.memory).entries.get(&identity.slot).cloned() {
            return classify_entry(entry, identity, contract);
        }
        if contract.mode != ToolCacheMode::MemoryAndProject {
            return CacheLookup::Miss;
        }
        let Ok(directory) = validated_durable_directory(project_root, false) else {
            return CacheLookup::Miss;
        };
        let file_name = format!("{}.json", identity.slot);
        let Ok(bytes) = read_durable_entry(&directory, &file_name, contract.max_result_bytes)
        else {
            return CacheLookup::Miss;
        };
        let Ok(entry) = serde_json::from_slice::<CacheEntry>(&bytes) else {
            return CacheLookup::Stale;
        };
        let outcome = classify_entry(entry.clone(), identity, contract);
        if matches!(outcome, CacheLookup::Hit(_)) {
            self.insert_memory(identity.slot.clone(), entry, None);
        }
        outcome
    }

    async fn store_cache(
        &self,
        project_root: &Path,
        identity: &CacheIdentity,
        result: CacheableToolResult,
        contract: &ToolCacheContract,
        env: &dyn ToolEnv,
    ) {
        let entry = CacheEntry {
            format: CACHE_FORMAT,
            fingerprint: identity.fingerprint.clone(),
            stored_at_unix_ms: now_unix_ms(),
            result,
        };
        if self.insert_memory(identity.slot.clone(), entry.clone(), Some(env))
            && contract.mode == ToolCacheMode::MemoryAndProject
        {
            if let (Ok(directory), Ok(bytes)) = (
                validated_durable_directory(project_root, true),
                serde_json::to_vec(&entry),
            ) {
                let file_name = format!("{}.json", identity.slot);
                if write_durable_entry(&directory, &file_name, &bytes).is_ok() {
                    enforce_durable_bounds(&directory, env);
                }
            }
        }
    }

    fn insert_memory(&self, slot: String, entry: CacheEntry, env: Option<&dyn ToolEnv>) -> bool {
        if entry.bytes() > self.max_memory_bytes {
            return false;
        }
        let mut cache = lock(&self.memory);
        if let Some(old) = cache.entries.remove(&slot) {
            cache.bytes = cache.bytes.saturating_sub(old.bytes());
            cache.order.retain(|item| item != &slot);
        }
        cache.bytes += entry.bytes();
        cache.order.push_back(slot.clone());
        cache.entries.insert(slot, entry);
        while cache.entries.len() > self.max_memory_entries || cache.bytes > self.max_memory_bytes {
            let Some(oldest) = cache.order.pop_front() else {
                break;
            };
            if let Some(old) = cache.entries.remove(&oldest) {
                cache.bytes = cache.bytes.saturating_sub(old.bytes());
                if let Some(env) = env {
                    emit(env, ToolExecutionSignal::Evicted, None, None);
                }
            }
        }
        true
    }

    fn remove_memory(&self, slot: &str) {
        let mut cache = lock(&self.memory);
        if let Some(entry) = cache.entries.remove(slot) {
            cache.bytes = cache.bytes.saturating_sub(entry.bytes());
        }
        cache.order.retain(|candidate| candidate != slot);
    }
}

enum CacheLookup {
    Hit(CacheEntry),
    Stale,
    Miss,
}

#[derive(Debug)]
struct CacheIdentity {
    slot: String,
    fingerprint: String,
}

fn cache_identity(
    tool: &dyn Tool,
    args: &Value,
    provider: &str,
    contract: &ToolCacheContract,
    scope: &ToolExecutionScope,
) -> CacheIdentity {
    let schema = tool.schema();
    let stable = json!({
        "agent": scope.agent_identity,
        "authorization": scope.authorization_scope,
        "connector_authorization": tool.cache_authorization_revision().unwrap_or_default(),
        "policy": scope.policy_projection,
        "tool": tool.name(),
        "provider": provider,
        "arguments": args,
    });
    let full = json!({
        "stable": stable.clone(),
        "capability_version": contract.capability_version,
        "schema_version": contract.schema_version,
        "schema": schema.function.parameters,
        "remote_contract": tool.cache_contract(),
        "release_digest": contract.release_digest,
        "index_digest": contract.index_digest,
    });
    CacheIdentity {
        slot: digest(&canonical_json(&stable)),
        fingerprint: digest(&canonical_json(&full)),
    }
}

fn classify_entry(
    entry: CacheEntry,
    identity: &CacheIdentity,
    contract: &ToolCacheContract,
) -> CacheLookup {
    let now = now_unix_ms();
    let elapsed = now.saturating_sub(entry.stored_at_unix_ms);
    let ttl_ms = contract.ttl.as_millis();
    if entry.format != CACHE_FORMAT
        || entry.fingerprint != identity.fingerprint
        || entry.stored_at_unix_ms > now
        || elapsed > ttl_ms
        || entry.bytes() > contract.max_result_bytes
        || serde_json::from_str::<Value>(&entry.result.content).is_err()
    {
        CacheLookup::Stale
    } else {
        CacheLookup::Hit(entry)
    }
}

fn valid_cache_projection(
    tool: &dyn Tool,
    result: &ToolResult,
    contract: &ToolCacheContract,
) -> Option<CacheableToolResult> {
    if !result.success
        || !result.images.is_empty()
        || result.control != ToolControl::Continue
        || result.allowed_next_tools.is_some()
    {
        return None;
    }
    let record = tool.cacheable_result(result)?;
    if record.content.len() > contract.max_result_bytes
        || serde_json::from_str::<Value>(&record.content).is_err()
    {
        return None;
    }
    Some(record)
}

async fn wait_for_flight(
    tool: &str,
    provider: &str,
    env: &dyn ToolEnv,
    flight: Arc<Flight>,
) -> FlightResolution {
    loop {
        if let Some(resolution) = lock(&flight.resolution).clone() {
            return resolution;
        }
        if env.is_cancelled() {
            flight.interested_callers.fetch_sub(1, Ordering::SeqCst);
            emit(env, ToolExecutionSignal::Cancelled, None, None);
            return FlightResolution::Completed(typed_error(
                "tool_wait_cancelled",
                tool,
                provider,
                0,
                0,
            ));
        }
        let _ = tokio::time::timeout(Duration::from_millis(25), flight.notify.notified()).await;
    }
}

fn typed_error(
    code: &str,
    tool: &str,
    provider: &str,
    max_concurrency: usize,
    max_queue: usize,
) -> ToolResult {
    ToolResult::fail(
        serde_json::to_string(&json!({
            "error": {
                "code": code,
                "tool": tool,
                "provider": provider,
                "max_concurrency": max_concurrency,
                "max_queue": max_queue,
            }
        }))
        .unwrap_or_else(|_| format!("tool execution error: {code}")),
    )
}

fn emit(
    env: &dyn ToolEnv,
    signal: ToolExecutionSignal,
    fingerprint: Option<&str>,
    queue_depth: Option<usize>,
) {
    env.note_tool_execution(&ToolExecutionDiagnostic {
        signal,
        fingerprint: fingerprint.map(ToOwned::to_owned),
        queue_depth,
    });
}

fn validated_durable_directory(project_root: &Path, create: bool) -> Result<Dir, String> {
    let mut directory = Dir::open_ambient_dir(project_root, ambient_authority())
        .map_err(|error| format!("project root is not resolvable: {error}"))?;
    for segment in [".wisp", "tool-cache", "v1"] {
        match directory.open_dir_nofollow(segment) {
            Ok(next) => directory = next,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && create => {
                match directory.create_dir(segment) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => {
                        return Err(format!(
                            "could not create durable cache directory '{segment}': {error}"
                        ));
                    }
                }
                directory = directory.open_dir_nofollow(segment).map_err(|error| {
                    format!("durable cache component '{segment}' is unsafe: {error}")
                })?;
            }
            Err(error) => return Err(format!("durable cache is unavailable: {error}")),
        }
    }
    Ok(directory)
}

fn read_durable_entry(
    directory: &Dir,
    file_name: &str,
    max_result_bytes: usize,
) -> std::io::Result<Vec<u8>> {
    let max_entry_bytes = max_result_bytes
        .saturating_mul(6)
        .saturating_add(64 * 1024)
        .min(DEFAULT_DURABLE_BYTES as usize);
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let file = directory.open_with(file_name, &options)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > max_entry_bytes as u64 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "durable cache entry exceeds its bounded result envelope",
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(max_entry_bytes as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > max_entry_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "durable cache entry grew beyond its bounded result envelope",
        ));
    }
    Ok(bytes)
}

fn write_durable_entry(directory: &Dir, file_name: &str, bytes: &[u8]) -> std::io::Result<()> {
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);
    for _ in 0..32 {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let temporary = format!(".{file_name}.tmp-{}-{sequence}", std::process::id());
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        let mut file = match directory.open_with(&temporary, &options) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        if let Err(error) = file.write_all(bytes) {
            drop(file);
            let _ = directory.remove_file(&temporary);
            return Err(error);
        }
        drop(file);
        if let Err(error) = directory.rename(&temporary, directory, file_name) {
            let _ = directory.remove_file(&temporary);
            return Err(error);
        }
        return Ok(());
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not allocate a unique durable-cache temporary file",
    ))
}

fn enforce_durable_bounds(directory: &Dir, env: &dyn ToolEnv) {
    let Ok(reader) = directory.entries() else {
        return;
    };
    let mut files = Vec::new();
    let mut total_bytes = 0u64;
    for item in reader.flatten() {
        let file_name = item.file_name();
        if Path::new(&file_name)
            .extension()
            .and_then(|ext| ext.to_str())
            != Some("json")
        {
            continue;
        }
        let Ok(file_type) = item.file_type() else {
            continue;
        };
        if !file_type.is_file() {
            continue;
        }
        let Ok(metadata) = item.metadata() else {
            continue;
        };
        let bytes = metadata.len();
        let modified = metadata
            .modified()
            .map(cap_std::time::SystemTime::into_std)
            .unwrap_or(UNIX_EPOCH);
        total_bytes = total_bytes.saturating_add(bytes);
        files.push((modified, file_name, bytes));
    }
    files.sort_by_key(|(modified, _, _)| *modified);
    while files.len() > DEFAULT_DURABLE_ENTRIES || total_bytes > DEFAULT_DURABLE_BYTES {
        let (_, file_name, bytes) = files.remove(0);
        if directory.remove_file(file_name).is_ok() {
            total_bytes = total_bytes.saturating_sub(bytes);
            emit(env, ToolExecutionSignal::Evicted, None, None);
        }
    }
}

fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn canonical_json(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => serde_json::to_string(value).unwrap_or_default(),
        Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(canonical_json)
                .collect::<Vec<_>>()
                .join(",")
        ),
        Value::Object(values) => {
            let mut keys: Vec<_> = values.keys().collect();
            keys.sort_unstable();
            format!(
                "{{{}}}",
                keys.into_iter()
                    .map(|key| format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap_or_default(),
                        canonical_json(&values[key])
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
    use wisp_llm::ToolSchema;

    static NEXT_ROOT: AtomicU64 = AtomicU64::new(1);

    struct FakeTool {
        name: &'static str,
        connector: &'static str,
        authorization_revision: &'static str,
        remote_contract_revision: &'static str,
        cache_contract_valid: Arc<AtomicBool>,
        calls: Arc<AtomicUsize>,
        policy: ToolExecutionPolicy,
        read_only: bool,
        project_result: bool,
        delay_ms: u64,
        cancel_aware: bool,
    }

    #[async_trait]
    impl Tool for FakeTool {
        fn name(&self) -> &str {
            self.name
        }

        fn schema(&self) -> ToolSchema {
            ToolSchema::new(
                self.name,
                "fake read",
                json!({
                    "type": "object",
                    "properties": {"gene": {"type": "string"}}
                }),
            )
        }

        fn read_only(&self) -> bool {
            self.read_only
        }

        fn connector_id(&self) -> Option<&str> {
            Some(self.connector)
        }

        fn cache_authorization_revision(&self) -> Option<&str> {
            Some(self.authorization_revision)
        }

        fn cache_contract(&self) -> Option<Value> {
            (!self.remote_contract_revision.is_empty())
                .then(|| json!({"revision": self.remote_contract_revision}))
        }

        fn execution_policy(&self, _args: &Value) -> ToolExecutionPolicy {
            self.policy.clone()
        }

        fn cacheable_result(&self, result: &ToolResult) -> Option<CacheableToolResult> {
            self.project_result
                .then(|| CacheableToolResult::structured(result.content.clone()))
        }

        async fn validate_cache_hit(&self) -> Result<(), String> {
            self.cache_contract_valid
                .load(Ordering::SeqCst)
                .then_some(())
                .ok_or_else(|| "remote catalog changed".into())
        }

        async fn run_coordinated(&self, _args: &Value, env: &dyn ToolEnv) -> crate::ToolRunOutcome {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let delay = self.delay_ms;
            let mut work = Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(delay)).await;
                ToolResult::ok(r#"{"evidence":[{"id":"bounded-ref"}]}"#)
            });
            if !self.cancel_aware {
                return crate::ToolRunOutcome::complete(work.await);
            }
            tokio::select! {
                result = &mut work => crate::ToolRunOutcome::complete(result),
                _ = async {
                    loop {
                        if env.caller_cancelled() {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                } => crate::ToolRunOutcome::detached(
                    ToolResult::fail("caller stopped waiting"),
                    Box::pin(async move { work.await }),
                ),
            }
        }

        async fn run(&self, _args: &Value, env: &dyn ToolEnv) -> ToolResult {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let mut elapsed = 0;
            while elapsed < self.delay_ms {
                if self.cancel_aware && env.is_cancelled() {
                    return ToolResult::fail("cancelled by all callers");
                }
                let step = (self.delay_ms - elapsed).min(5);
                tokio::time::sleep(Duration::from_millis(step)).await;
                elapsed += step;
            }
            ToolResult::ok(r#"{"evidence":[{"id":"bounded-ref"}]}"#)
        }
    }

    struct TestEnv {
        root: PathBuf,
        scope: ToolExecutionScope,
        cancelled: Arc<AtomicBool>,
        diagnostics: Mutex<Vec<ToolExecutionDiagnostic>>,
    }

    struct DetachedTool {
        calls: Arc<AtomicUsize>,
        completion_ms: u64,
    }

    struct GatedValidationTool {
        calls: Arc<AtomicUsize>,
        validations: Arc<AtomicUsize>,
        validation_delay_ms: u64,
        policy: ToolExecutionPolicy,
    }

    #[async_trait]
    impl Tool for GatedValidationTool {
        fn name(&self) -> &str {
            "gated_validation"
        }

        fn schema(&self) -> ToolSchema {
            ToolSchema::new(self.name(), "gated validation", json!({"type": "object"}))
        }

        fn read_only(&self) -> bool {
            true
        }

        fn connector_id(&self) -> Option<&str> {
            Some("provider")
        }

        fn cache_authorization_revision(&self) -> Option<&str> {
            Some("credential-v1")
        }

        fn cache_contract(&self) -> Option<Value> {
            Some(json!({"revision": "remote-contract-v1"}))
        }

        fn execution_policy(&self, _args: &Value) -> ToolExecutionPolicy {
            self.policy.clone()
        }

        fn cacheable_result(&self, result: &ToolResult) -> Option<CacheableToolResult> {
            Some(CacheableToolResult::structured(result.content.clone()))
        }

        async fn validate_cache_hit(&self) -> Result<(), String> {
            self.validations.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(self.validation_delay_ms)).await;
            Ok(())
        }

        async fn validate_cache_hit_coordinated(&self, env: &dyn ToolEnv) -> crate::ToolRunOutcome {
            self.validations.fetch_add(1, Ordering::SeqCst);
            let delay = self.validation_delay_ms;
            let mut validation = Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(delay)).await;
                ToolResult::ok("{}")
            });
            tokio::select! {
                result = &mut validation => crate::ToolRunOutcome::complete(result),
                _ = async {
                    loop {
                        if env.caller_cancelled() {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                } => crate::ToolRunOutcome::detached(
                    ToolResult::fail("cache validation caller stopped waiting"),
                    Box::pin(async move { validation.await }),
                ),
            }
        }

        async fn run(&self, _args: &Value, _env: &dyn ToolEnv) -> ToolResult {
            self.calls.fetch_add(1, Ordering::SeqCst);
            ToolResult::ok(r#"{"evidence":[{"id":"bounded-ref"}]}"#)
        }
    }

    #[async_trait]
    impl Tool for DetachedTool {
        fn name(&self) -> &str {
            "detached_remote"
        }

        fn schema(&self) -> ToolSchema {
            ToolSchema::new(self.name(), "detached remote", json!({"type": "object"}))
        }

        fn execution_policy(&self, _args: &Value) -> ToolExecutionPolicy {
            ToolExecutionPolicy {
                cache: ToolCacheContract::default(),
                max_concurrency: 1,
                max_queue: 0,
            }
        }

        async fn run_coordinated(
            &self,
            _args: &Value,
            _env: &dyn ToolEnv,
        ) -> crate::ToolRunOutcome {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let delay = self.completion_ms;
            crate::ToolRunOutcome::detached(
                ToolResult::fail("caller stopped waiting"),
                Box::pin(async move {
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                    ToolResult::fail("detached completion finished")
                }),
            )
        }

        async fn run(&self, _args: &Value, _env: &dyn ToolEnv) -> ToolResult {
            unreachable!("the coordinator must use run_coordinated")
        }
    }

    impl TestEnv {
        fn new(root: PathBuf, authorization: &str) -> Self {
            Self {
                root,
                scope: ToolExecutionScope {
                    agent_identity: "agent:reader".into(),
                    authorization_scope: authorization.into(),
                    policy_projection: "read-only:v1".into(),
                },
                cancelled: Arc::new(AtomicBool::new(false)),
                diagnostics: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl ToolEnv for TestEnv {
        fn project_root(&self) -> &Path {
            &self.root
        }

        async fn confirm(&self, _message: &str) -> bool {
            true
        }

        async fn emit(&self, _event: crate::ToolEvent) {}

        fn is_cancelled(&self) -> bool {
            self.cancelled.load(Ordering::SeqCst)
        }

        fn tool_execution_scope(&self) -> ToolExecutionScope {
            self.scope.clone()
        }

        fn note_tool_execution(&self, event: &ToolExecutionDiagnostic) {
            lock(&self.diagnostics).push(event.clone());
        }
    }

    fn root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "wisp-tool-execution-{label}-{}-{}",
            std::process::id(),
            NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn cache_policy(release: &str, mode: ToolCacheMode) -> ToolExecutionPolicy {
        ToolExecutionPolicy {
            cache: ToolCacheContract {
                mode,
                capability_version: "cap-v1".into(),
                schema_version: "schema-v1".into(),
                release_digest: release.into(),
                index_digest: "index-v1".into(),
                ttl: Duration::from_secs(60),
                max_result_bytes: 4 * 1024,
                shared_authorization: true,
                certain_outcome: true,
            },
            max_concurrency: 2,
            max_queue: 4,
        }
    }

    fn fake(
        name: &'static str,
        connector: &'static str,
        calls: Arc<AtomicUsize>,
        policy: ToolExecutionPolicy,
    ) -> Arc<FakeTool> {
        Arc::new(FakeTool {
            name,
            connector,
            authorization_revision: "credential-v1",
            remote_contract_revision: "remote-contract-v1",
            cache_contract_valid: Arc::new(AtomicBool::new(true)),
            calls,
            policy,
            read_only: true,
            project_result: true,
            delay_ms: 60,
            cancel_aware: false,
        })
    }

    #[test]
    fn cache_entries_from_the_future_are_stale() {
        let identity = CacheIdentity {
            slot: "future-slot".into(),
            fingerprint: "future-fingerprint".into(),
        };
        let entry = CacheEntry {
            format: CACHE_FORMAT,
            fingerprint: identity.fingerprint.clone(),
            stored_at_unix_ms: now_unix_ms() + 60_000,
            result: CacheableToolResult::structured(r#"{"evidence":[]}"#),
        };

        assert!(matches!(
            classify_entry(
                entry,
                &identity,
                &cache_policy("release-v1", ToolCacheMode::Memory).cache,
            ),
            CacheLookup::Stale
        ));
    }

    #[tokio::test]
    async fn identical_normalized_reads_are_single_flight_and_cached() {
        let coordinator = Arc::new(ToolExecutionCoordinator::default());
        let calls = Arc::new(AtomicUsize::new(0));
        let tool = fake(
            "single_flight_read",
            "fake-provider",
            calls.clone(),
            cache_policy("release-v1", ToolCacheMode::Memory),
        );
        let env = Arc::new(TestEnv::new(root("single-flight"), "scope-a"));
        let left: Value = serde_json::from_str(r#"{"gene":"KRAS","limit":5}"#).unwrap();
        let right: Value = serde_json::from_str(r#"{"limit":5,"gene":"KRAS"}"#).unwrap();

        let first = {
            let coordinator = coordinator.clone();
            let tool = tool.clone();
            let env = env.clone();
            tokio::spawn(async move {
                coordinator
                    .execute(tool.as_ref(), &left, env.as_ref(), false)
                    .await
            })
        };
        tokio::time::sleep(Duration::from_millis(5)).await;
        let second = coordinator
            .execute(tool.as_ref(), &right, env.as_ref(), false)
            .await;
        let first = first.await.unwrap();
        let third = coordinator
            .execute(tool.as_ref(), &right, env.as_ref(), false)
            .await;

        assert!(first.success && second.success && third.success);
        assert_eq!(first.content, second.content);
        assert_eq!(second.content, third.content);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let signals: Vec<_> = lock(&env.diagnostics)
            .iter()
            .map(|event| event.signal)
            .collect();
        assert!(signals.contains(&ToolExecutionSignal::Coalesced));
        assert!(signals.contains(&ToolExecutionSignal::Hit));
    }

    #[tokio::test]
    async fn release_connector_authorization_and_arguments_never_collide() {
        let coordinator = ToolExecutionCoordinator::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let project = root("isolation");
        let env_a = TestEnv::new(project.clone(), "scope-a");
        let env_b = TestEnv::new(project, "scope-b");
        let v1 = fake(
            "isolated_read",
            "provider-a",
            calls.clone(),
            cache_policy("release-v1", ToolCacheMode::Memory),
        );
        let args = json!({"gene": "KRAS"});
        coordinator.execute(v1.as_ref(), &args, &env_a, false).await;
        coordinator.execute(v1.as_ref(), &args, &env_a, false).await;
        coordinator
            .execute(v1.as_ref(), &json!({"gene": "NRAS"}), &env_a, false)
            .await;
        coordinator.execute(v1.as_ref(), &args, &env_b, false).await;
        let credential_v2 = Arc::new(FakeTool {
            name: "isolated_read",
            connector: "provider-a",
            authorization_revision: "credential-v2",
            remote_contract_revision: "remote-contract-v1",
            cache_contract_valid: Arc::new(AtomicBool::new(true)),
            calls: calls.clone(),
            policy: cache_policy("release-v1", ToolCacheMode::Memory),
            read_only: true,
            project_result: true,
            delay_ms: 60,
            cancel_aware: false,
        });
        coordinator
            .execute(credential_v2.as_ref(), &args, &env_a, false)
            .await;
        let provider_b = fake(
            "isolated_read",
            "provider-b",
            calls.clone(),
            cache_policy("release-v1", ToolCacheMode::Memory),
        );
        coordinator
            .execute(provider_b.as_ref(), &args, &env_a, false)
            .await;
        let release_v2 = fake(
            "isolated_read",
            "provider-a",
            calls.clone(),
            cache_policy("release-v2", ToolCacheMode::Memory),
        );
        coordinator
            .execute(release_v2.as_ref(), &args, &env_a, false)
            .await;
        let mut index_v2_policy = cache_policy("release-v1", ToolCacheMode::Memory);
        index_v2_policy.cache.index_digest = "index-v2".into();
        let index_v2 = fake(
            "isolated_read",
            "provider-a",
            calls.clone(),
            index_v2_policy,
        );
        coordinator
            .execute(index_v2.as_ref(), &args, &env_a, false)
            .await;

        let mut schema_v2_policy = cache_policy("release-v1", ToolCacheMode::Memory);
        schema_v2_policy.cache.schema_version = "schema-v2".into();
        let schema_v2 = fake(
            "isolated_read",
            "provider-a",
            calls.clone(),
            schema_v2_policy,
        );
        coordinator
            .execute(schema_v2.as_ref(), &args, &env_a, false)
            .await;

        let mut capability_v2_policy = cache_policy("release-v1", ToolCacheMode::Memory);
        capability_v2_policy.cache.capability_version = "cap-v2".into();
        let capability_v2 = fake(
            "isolated_read",
            "provider-a",
            calls.clone(),
            capability_v2_policy,
        );
        coordinator
            .execute(capability_v2.as_ref(), &args, &env_a, false)
            .await;

        let remote_contract_v2 = Arc::new(FakeTool {
            name: "isolated_read",
            connector: "provider-a",
            authorization_revision: "credential-v1",
            remote_contract_revision: "remote-contract-v2",
            cache_contract_valid: Arc::new(AtomicBool::new(true)),
            calls: calls.clone(),
            policy: cache_policy("release-v1", ToolCacheMode::Memory),
            read_only: true,
            project_result: true,
            delay_ms: 60,
            cancel_aware: false,
        });
        coordinator
            .execute(remote_contract_v2.as_ref(), &args, &env_a, false)
            .await;

        assert_eq!(calls.load(Ordering::SeqCst), 10);
        assert!(lock(&env_a.diagnostics)
            .iter()
            .any(|event| event.signal == ToolExecutionSignal::Stale));
    }

    #[tokio::test]
    async fn cache_hit_revalidates_live_connector_contract() {
        let coordinator = ToolExecutionCoordinator::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let valid = Arc::new(AtomicBool::new(true));
        let tool = Arc::new(FakeTool {
            name: "revalidated_read",
            connector: "provider",
            authorization_revision: "credential-v1",
            remote_contract_revision: "remote-contract-v1",
            cache_contract_valid: valid.clone(),
            calls: calls.clone(),
            policy: cache_policy("release-v1", ToolCacheMode::Memory),
            read_only: true,
            project_result: true,
            delay_ms: 0,
            cancel_aware: false,
        });
        let env = TestEnv::new(root("revalidate"), "scope-a");
        let args = json!({"gene": "KRAS"});
        assert!(
            coordinator
                .execute(tool.as_ref(), &args, &env, false)
                .await
                .success
        );
        valid.store(false, Ordering::SeqCst);
        let rejected = coordinator.execute(tool.as_ref(), &args, &env, false).await;

        assert!(!rejected.success);
        assert!(rejected.content.contains("remote catalog changed"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(lock(&env.diagnostics)
            .iter()
            .any(|event| event.signal == ToolExecutionSignal::Stale));
    }

    #[tokio::test]
    async fn cache_hit_validation_obeys_provider_backpressure() {
        let coordinator = Arc::new(ToolExecutionCoordinator::default());
        let calls = Arc::new(AtomicUsize::new(0));
        let validations = Arc::new(AtomicUsize::new(0));
        let mut policy = cache_policy("release-v1", ToolCacheMode::Memory);
        policy.max_concurrency = 1;
        policy.max_queue = 0;
        let tool = Arc::new(GatedValidationTool {
            calls: calls.clone(),
            validations: validations.clone(),
            validation_delay_ms: 80,
            policy,
        });
        let env = Arc::new(TestEnv::new(root("gated-validation"), "scope-a"));
        let args = json!({"gene": "KRAS"});
        assert!(
            coordinator
                .execute(tool.as_ref(), &args, env.as_ref(), false)
                .await
                .success
        );

        let first_hit = {
            let coordinator = coordinator.clone();
            let tool = tool.clone();
            let env = env.clone();
            let args = args.clone();
            tokio::spawn(async move {
                coordinator
                    .execute(tool.as_ref(), &args, env.as_ref(), false)
                    .await
            })
        };
        tokio::time::sleep(Duration::from_millis(10)).await;
        let overflow = coordinator
            .execute(tool.as_ref(), &args, env.as_ref(), false)
            .await;

        assert!(!overflow.success);
        assert!(overflow.content.contains("tool_queue_overflow"));
        assert!(first_hit.await.unwrap().success);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(validations.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cache_hit_expiring_during_validation_is_not_replayed() {
        let coordinator = ToolExecutionCoordinator::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let validations = Arc::new(AtomicUsize::new(0));
        let mut policy = cache_policy("release-v1", ToolCacheMode::Memory);
        policy.cache.ttl = Duration::from_millis(20);
        let tool = GatedValidationTool {
            calls: calls.clone(),
            validations: validations.clone(),
            validation_delay_ms: 60,
            policy,
        };
        let env = TestEnv::new(root("expired-during-validation"), "scope-a");
        let args = json!({"gene": "KRAS"});
        assert!(coordinator.execute(&tool, &args, &env, false).await.success);

        let refreshed = coordinator.execute(&tool, &args, &env, false).await;

        assert!(refreshed.success);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(validations.load(Ordering::SeqCst), 1);
        assert!(lock(&env.diagnostics)
            .iter()
            .any(|event| event.signal == ToolExecutionSignal::Stale));
    }

    #[tokio::test]
    async fn cancelling_cache_hit_validation_releases_caller_but_retains_capacity() {
        let coordinator = Arc::new(ToolExecutionCoordinator::default());
        let calls = Arc::new(AtomicUsize::new(0));
        let validations = Arc::new(AtomicUsize::new(0));
        let mut policy = cache_policy("release-v1", ToolCacheMode::Memory);
        policy.max_concurrency = 1;
        policy.max_queue = 0;
        let tool = Arc::new(GatedValidationTool {
            calls: calls.clone(),
            validations: validations.clone(),
            validation_delay_ms: 300,
            policy,
        });
        let project = root("cancelled-cache-validation");
        let seed_env = TestEnv::new(project.clone(), "scope-a");
        let args = json!({"gene": "KRAS"});
        assert!(
            coordinator
                .execute(tool.as_ref(), &args, &seed_env, false)
                .await
                .success
        );

        let cancelled_env = Arc::new(TestEnv::new(project.clone(), "scope-a"));
        let cancelled_hit = {
            let coordinator = coordinator.clone();
            let tool = tool.clone();
            let env = cancelled_env.clone();
            let args = args.clone();
            tokio::spawn(async move {
                coordinator
                    .execute(tool.as_ref(), &args, env.as_ref(), false)
                    .await
            })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        cancelled_env.cancelled.store(true, Ordering::SeqCst);
        let cancelled = tokio::time::timeout(Duration::from_millis(100), cancelled_hit)
            .await
            .expect("cancelled cache validation must release its caller promptly")
            .unwrap();
        assert!(!cancelled.success);
        assert!(cancelled.content.contains("tool_wait_cancelled"));

        let active_env = TestEnv::new(project, "scope-a");
        let overflow = coordinator
            .execute(tool.as_ref(), &args, &active_env, false)
            .await;
        assert!(!overflow.success);
        assert!(overflow.content.contains("tool_queue_overflow"));

        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            coordinator
                .execute(tool.as_ref(), &args, &active_env, false)
                .await
                .success
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(validations.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn mutating_and_uncertain_results_bypass_cache() {
        let coordinator = ToolExecutionCoordinator::default();
        let env = TestEnv::new(root("bypass"), "scope-a");
        let args = json!({"gene": "KRAS"});
        let mutating_calls = Arc::new(AtomicUsize::new(0));
        let mutating = Arc::new(FakeTool {
            name: "mutating",
            connector: "provider",
            authorization_revision: "credential-v1",
            remote_contract_revision: "remote-contract-v1",
            cache_contract_valid: Arc::new(AtomicBool::new(true)),
            calls: mutating_calls.clone(),
            policy: cache_policy("release-v1", ToolCacheMode::Memory),
            read_only: false,
            project_result: true,
            delay_ms: 60,
            cancel_aware: false,
        });
        coordinator
            .execute(mutating.as_ref(), &args, &env, false)
            .await;
        coordinator
            .execute(mutating.as_ref(), &args, &env, false)
            .await;
        assert_eq!(mutating_calls.load(Ordering::SeqCst), 2);

        let uncertain_calls = Arc::new(AtomicUsize::new(0));
        let mut uncertain_policy = cache_policy("release-v1", ToolCacheMode::Memory);
        uncertain_policy.cache.certain_outcome = false;
        let uncertain = fake(
            "uncertain",
            "provider",
            uncertain_calls.clone(),
            uncertain_policy,
        );
        coordinator
            .execute(uncertain.as_ref(), &args, &env, false)
            .await;
        coordinator
            .execute(uncertain.as_ref(), &args, &env, false)
            .await;
        assert_eq!(uncertain_calls.load(Ordering::SeqCst), 2);

        let approval_calls = Arc::new(AtomicUsize::new(0));
        let approval_bearing = fake(
            "approval_bearing",
            "provider",
            approval_calls.clone(),
            cache_policy("release-v1", ToolCacheMode::Memory),
        );
        coordinator
            .execute(approval_bearing.as_ref(), &args, &env, true)
            .await;
        coordinator
            .execute(approval_bearing.as_ref(), &args, &env, true)
            .await;
        assert_eq!(approval_calls.load(Ordering::SeqCst), 2);

        let missing_contract_calls = Arc::new(AtomicUsize::new(0));
        let missing_contract = Arc::new(FakeTool {
            name: "missing_remote_contract",
            connector: "provider",
            authorization_revision: "credential-v1",
            remote_contract_revision: "",
            cache_contract_valid: Arc::new(AtomicBool::new(true)),
            calls: missing_contract_calls.clone(),
            policy: cache_policy("release-v1", ToolCacheMode::Memory),
            read_only: true,
            project_result: true,
            delay_ms: 0,
            cancel_aware: false,
        });
        coordinator
            .execute(missing_contract.as_ref(), &args, &env, false)
            .await;
        coordinator
            .execute(missing_contract.as_ref(), &args, &env, false)
            .await;
        assert_eq!(missing_contract_calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn durable_cache_survives_coordinator_recreation() {
        let calls = Arc::new(AtomicUsize::new(0));
        let tool = fake(
            "durable_read",
            "provider",
            calls.clone(),
            cache_policy("release-v1", ToolCacheMode::MemoryAndProject),
        );
        let project = root("durable");
        std::fs::create_dir_all(&project).unwrap();
        let env = TestEnv::new(project.clone(), "scope-a");
        let args = json!({"gene": "KRAS"});
        ToolExecutionCoordinator::default()
            .execute(tool.as_ref(), &args, &env, false)
            .await;
        ToolExecutionCoordinator::default()
            .execute(tool.as_ref(), &args, &env, false)
            .await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let _ = std::fs::remove_dir_all(project);
    }

    #[test]
    fn durable_cache_rejects_oversized_serialized_entries_before_reading() {
        let project = root("durable-oversized");
        std::fs::create_dir_all(&project).unwrap();
        let directory = validated_durable_directory(&project, true).unwrap();
        let oversized = vec![b'x'; 70 * 1024];
        write_durable_entry(&directory, "oversized.json", &oversized).unwrap();

        let error = read_durable_entry(&directory, "oversized.json", 512).unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        let _ = std::fs::remove_dir_all(project);
    }

    #[test]
    fn durable_cache_replaces_hard_link_without_truncating_its_target() {
        let project = root("durable-hard-link-project");
        let outside = root("durable-hard-link-outside");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let outside_file = outside.join("keep.json");
        std::fs::write(&outside_file, b"keep").unwrap();
        let directory = validated_durable_directory(&project, true).unwrap();
        let cache_file = project
            .join(".wisp")
            .join("tool-cache")
            .join("v1")
            .join("linked.json");
        std::fs::hard_link(&outside_file, &cache_file).unwrap();

        write_durable_entry(&directory, "linked.json", b"replacement").unwrap();

        assert_eq!(std::fs::read(&outside_file).unwrap(), b"keep");
        assert_eq!(std::fs::read(&cache_file).unwrap(), b"replacement");
        let _ = std::fs::remove_dir_all(project);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn durable_cache_refuses_symlinked_storage_components() {
        use std::os::unix::fs::symlink;

        let calls = Arc::new(AtomicUsize::new(0));
        let tool = fake(
            "symlink_safe_read",
            "provider",
            calls.clone(),
            cache_policy("release-v1", ToolCacheMode::MemoryAndProject),
        );
        let project = root("symlink-project");
        let outside = root("symlink-outside");
        std::fs::create_dir_all(project.join(".wisp")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("keep.json"), b"keep").unwrap();
        symlink(&outside, project.join(".wisp").join("tool-cache")).unwrap();
        let env = TestEnv::new(project.clone(), "scope-a");

        let result = ToolExecutionCoordinator::default()
            .execute(tool.as_ref(), &json!({"gene": "KRAS"}), &env, false)
            .await;

        assert!(result.success);
        assert_eq!(std::fs::read(outside.join("keep.json")).unwrap(), b"keep");
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(project);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[tokio::test]
    async fn cancelling_waiter_does_not_cancel_leader() {
        let coordinator = Arc::new(ToolExecutionCoordinator::default());
        let calls = Arc::new(AtomicUsize::new(0));
        let tool = fake(
            "cancel_waiter",
            "provider",
            calls.clone(),
            cache_policy("release-v1", ToolCacheMode::Memory),
        );
        let project = root("cancel-waiter");
        let leader_env = Arc::new(TestEnv::new(project.clone(), "scope-a"));
        let waiter_env = Arc::new(TestEnv::new(project, "scope-a"));
        let args = json!({"gene": "KRAS"});
        let leader = {
            let coordinator = coordinator.clone();
            let tool = tool.clone();
            let env = leader_env.clone();
            let args = args.clone();
            tokio::spawn(async move {
                coordinator
                    .execute(tool.as_ref(), &args, env.as_ref(), false)
                    .await
            })
        };
        tokio::time::sleep(Duration::from_millis(5)).await;
        let waiter = {
            let coordinator = coordinator.clone();
            let tool = tool.clone();
            let env = waiter_env.clone();
            let args = args.clone();
            tokio::spawn(async move {
                coordinator
                    .execute(tool.as_ref(), &args, env.as_ref(), false)
                    .await
            })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        waiter_env.cancelled.store(true, Ordering::SeqCst);

        let waiter_result = waiter.await.unwrap();
        let leader_result = leader.await.unwrap();
        assert!(leader_result.success);
        assert!(!waiter_result.success);
        assert!(waiter_result.content.contains("tool_wait_cancelled"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cancelling_original_leader_does_not_fail_an_active_waiter() {
        let coordinator = Arc::new(ToolExecutionCoordinator::default());
        let calls = Arc::new(AtomicUsize::new(0));
        let tool = Arc::new(FakeTool {
            name: "cancel_leader",
            connector: "provider",
            authorization_revision: "credential-v1",
            remote_contract_revision: "remote-contract-v1",
            cache_contract_valid: Arc::new(AtomicBool::new(true)),
            calls: calls.clone(),
            policy: cache_policy("release-v1", ToolCacheMode::Memory),
            read_only: true,
            project_result: true,
            delay_ms: 300,
            cancel_aware: true,
        });
        let project = root("cancel-leader");
        let leader_env = Arc::new(TestEnv::new(project.clone(), "scope-a"));
        let waiter_env = Arc::new(TestEnv::new(project, "scope-a"));
        let args = json!({"gene": "KRAS"});
        let leader = {
            let coordinator = coordinator.clone();
            let tool = tool.clone();
            let env = leader_env.clone();
            let args = args.clone();
            tokio::spawn(async move {
                coordinator
                    .execute(tool.as_ref(), &args, env.as_ref(), false)
                    .await
            })
        };
        tokio::time::sleep(Duration::from_millis(10)).await;
        let waiter = {
            let coordinator = coordinator.clone();
            let tool = tool.clone();
            let env = waiter_env.clone();
            let args = args.clone();
            tokio::spawn(async move {
                coordinator
                    .execute(tool.as_ref(), &args, env.as_ref(), false)
                    .await
            })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        leader_env.cancelled.store(true, Ordering::SeqCst);

        let leader_result = tokio::time::timeout(Duration::from_millis(100), leader)
            .await
            .expect("cancelled single-flight leader must release its turn promptly")
            .unwrap();
        let waiter_result = waiter.await.unwrap();
        assert!(!leader_result.success);
        assert!(leader_result.content.contains("tool_wait_cancelled"));
        assert!(waiter_result.success, "{}", waiter_result.content);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cancelling_queued_flight_leader_hands_execution_to_waiter() {
        let coordinator = Arc::new(ToolExecutionCoordinator::default());
        let calls = Arc::new(AtomicUsize::new(0));
        let mut policy = cache_policy("release-v1", ToolCacheMode::Memory);
        policy.max_concurrency = 1;
        let tool = Arc::new(FakeTool {
            name: "queued_cancel_leader",
            connector: "provider",
            authorization_revision: "credential-v1",
            remote_contract_revision: "remote-contract-v1",
            cache_contract_valid: Arc::new(AtomicBool::new(true)),
            calls: calls.clone(),
            policy,
            read_only: true,
            project_result: true,
            delay_ms: 300,
            cancel_aware: true,
        });
        let project = root("queued-cancel-leader");
        let blocker_env = Arc::new(TestEnv::new(project.clone(), "scope-a"));
        let leader_env = Arc::new(TestEnv::new(project.clone(), "scope-a"));
        let waiter_env = Arc::new(TestEnv::new(project, "scope-a"));
        let blocker = {
            let coordinator = coordinator.clone();
            let tool = tool.clone();
            let env = blocker_env.clone();
            tokio::spawn(async move {
                coordinator
                    .execute(
                        tool.as_ref(),
                        &json!({"gene": "BLOCKER"}),
                        env.as_ref(),
                        false,
                    )
                    .await
            })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        let shared_args = json!({"gene": "SHARED"});
        let leader = {
            let coordinator = coordinator.clone();
            let tool = tool.clone();
            let env = leader_env.clone();
            let args = shared_args.clone();
            tokio::spawn(async move {
                coordinator
                    .execute(tool.as_ref(), &args, env.as_ref(), false)
                    .await
            })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        let waiter = {
            let coordinator = coordinator.clone();
            let tool = tool.clone();
            let env = waiter_env.clone();
            let args = shared_args.clone();
            tokio::spawn(async move {
                coordinator
                    .execute(tool.as_ref(), &args, env.as_ref(), false)
                    .await
            })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        leader_env.cancelled.store(true, Ordering::SeqCst);

        let leader_result = tokio::time::timeout(Duration::from_millis(100), leader)
            .await
            .expect("queued flight leader must release its cancelled caller promptly")
            .unwrap();
        assert!(!leader_result.success);
        assert!(leader_result.content.contains("tool_execution_cancelled"));
        assert!(blocker.await.unwrap().success);
        let waiter_result = waiter.await.unwrap();
        assert!(waiter_result.success, "{}", waiter_result.content);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn queue_overflow_is_bounded_and_typed() {
        let coordinator = Arc::new(ToolExecutionCoordinator::default());
        let calls = Arc::new(AtomicUsize::new(0));
        let tool = Arc::new(FakeTool {
            name: "bounded_queue",
            connector: "provider",
            authorization_revision: "credential-v1",
            remote_contract_revision: "remote-contract-v1",
            cache_contract_valid: Arc::new(AtomicBool::new(true)),
            calls: calls.clone(),
            policy: ToolExecutionPolicy {
                cache: ToolCacheContract::default(),
                max_concurrency: 1,
                max_queue: 0,
            },
            read_only: true,
            project_result: false,
            delay_ms: 80,
            cancel_aware: false,
        });
        let env = Arc::new(TestEnv::new(root("queue"), "scope-a"));
        let first = {
            let coordinator = coordinator.clone();
            let tool = tool.clone();
            let env = env.clone();
            tokio::spawn(async move {
                coordinator
                    .execute(tool.as_ref(), &json!({}), env.as_ref(), false)
                    .await
            })
        };
        tokio::time::sleep(Duration::from_millis(10)).await;
        let overflow = coordinator
            .execute(tool.as_ref(), &json!({}), env.as_ref(), false)
            .await;
        assert!(!overflow.success);
        assert!(overflow.content.contains("tool_queue_overflow"));
        assert!(first.await.unwrap().success);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn detached_remote_work_retains_capacity_until_completion() {
        let coordinator = ToolExecutionCoordinator::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let tool = DetachedTool {
            calls: calls.clone(),
            completion_ms: 80,
        };
        let env = TestEnv::new(root("detached-capacity"), "scope-a");

        let cancelled = coordinator.execute(&tool, &json!({}), &env, false).await;
        assert!(!cancelled.success);
        let overflow = coordinator.execute(&tool, &json!({}), &env, false).await;
        assert!(!overflow.success);
        assert!(overflow.content.contains("tool_queue_overflow"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        tokio::time::sleep(Duration::from_millis(100)).await;
        let next = coordinator.execute(&tool, &json!({}), &env, false).await;
        assert!(!next.success);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}

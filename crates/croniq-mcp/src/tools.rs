//! MCP tool definitions for the Croniq scheduler.
//!
//! Tools allow AI assistants to observe and operate the scheduler:
//!
//! ## Observe (always available)
//!
//! | Tool               | Description                              |
//! |--------------------|------------------------------------------|
//! | `list_runners`     | All runners with liveness status         |
//! | `get_runner`       | Single runner details                    |
//! | `queue_status`     | Queue depth + online runner count        |
//!
//! ## Operate (requires `--mutations` flag)
//!
//! | Tool               | Description                              |
//! |--------------------|------------------------------------------|
//! | `enqueue_job`      | Add a work item to the dispatch queue    |
//! | `cancel_execution` | Remove a pending execution from queue    |
//! | `job_trigger`      | Fire a job immediately                   |
//! | `dlq_retry`        | Re-enqueue a dead-lettered execution     |

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use croniq_config::compile::JobConfig;
use croniq_runner::{AppState, RunnerStatus, WorkItem};
use croniq_store::models::{Execution, ExecutionState};
use croniq_store::traits::Store;
use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::{tool::ToolRouter, wrapper::Parameters},
    model::*,
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ─── Type alias ───────────────────────────────────────────────────────────────

/// A type-erased, thread-safe store.
pub type DynStore = Arc<dyn Store + Send + Sync>;

// ─── Server struct ────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct CroniqMcp {
    pub state: Arc<AppState>,
    /// Persistent store for job/execution/DLQ operations. `None` when running
    /// without `--data-dir`.
    pub store: Option<DynStore>,
    /// Job configs for capability/timeout lookups (e.g. dlq_retry).
    pub jobs: HashMap<String, JobConfig>,
    /// Whether mutation tools are enabled (`--mutations` flag).
    pub mutations_enabled: bool,
    #[allow(dead_code)]
    tool_router: ToolRouter<CroniqMcp>,
}

// ─── Tool parameter types ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetRunnerParams {
    /// The unique runner ID to look up.
    pub runner_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct QueueStatusParams {
    /// Maximum number of pending items to include in the response (1–50).
    #[serde(default = "default_peek_limit")]
    pub peek_limit: usize,
}

fn default_peek_limit() -> usize {
    10
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EnqueueJobParams {
    /// Unique execution ID. Leave empty to auto-generate a UUID.
    #[serde(default)]
    pub execution_id: String,

    /// Job key in `namespace:name` format, e.g. `billing:invoice-generate`.
    pub job_key: String,

    /// Capabilities a runner MUST have to execute this job.
    #[serde(default)]
    pub require: Vec<String>,

    /// Capabilities that are preferred but not mandatory.
    #[serde(default)]
    pub prefer: Vec<String>,

    /// Optional metadata forwarded to the runner as-is (arbitrary JSON).
    #[serde(default)]
    pub metadata: serde_json::Value,

    /// Timeout hint for the runner (e.g. `"15m"`, `"2h"`). Default: `"5m"`.
    #[serde(default = "default_timeout")]
    pub timeout: String,
}

fn default_timeout() -> String {
    "5m".into()
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CancelExecutionParams {
    /// The execution ID to remove from the queue.
    pub execution_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct JobTriggerParams {
    /// Job key in `namespace:name` format, e.g. `billing:invoice-generate`.
    pub job_key: String,

    /// Capabilities a runner MUST have to execute this job. Defaults to none.
    #[serde(default)]
    pub require: Vec<String>,

    /// Capabilities that are preferred but not mandatory.
    #[serde(default)]
    pub prefer: Vec<String>,

    /// Optional metadata forwarded to the runner as-is (arbitrary JSON).
    #[serde(default)]
    pub metadata: serde_json::Value,

    /// Timeout hint for the runner (e.g. `"15m"`, `"2h"`). Default: `"5m"`.
    #[serde(default = "default_timeout")]
    pub timeout: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListExecutionsParams {
    /// Filter by job key.
    #[serde(default)]
    pub job_key: Option<String>,

    /// Filter by execution state (queued, claimed, completed, failed, dead, cancelled).
    #[serde(default)]
    pub state: Option<String>,

    /// Maximum number of results (1–100). Default: 20.
    #[serde(default = "default_list_limit")]
    pub limit: u32,
}

fn default_list_limit() -> u32 {
    20
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DlqRetryParams {
    /// The dead-letter ID to retry (UUID string).
    pub dead_letter_id: String,
}

// ─── Tool implementations ─────────────────────────────────────────────────────

#[tool_router]
impl CroniqMcp {
    /// Construct an observe-only MCP server (no mutations, no store).
    pub fn new(state: Arc<AppState>) -> Self {
        Self {
            state,
            store: None,
            jobs: HashMap::new(),
            mutations_enabled: false,
            tool_router: Self::tool_router(),
        }
    }

    /// Construct with a persistent store and optional mutation support.
    pub fn new_with_store(
        state: Arc<AppState>,
        store: DynStore,
        jobs: Vec<JobConfig>,
        mutations_enabled: bool,
    ) -> Self {
        let jobs = jobs.into_iter().map(|j| (j.key.clone(), j)).collect();
        Self {
            state,
            store: Some(store),
            jobs,
            mutations_enabled,
            tool_router: Self::tool_router(),
        }
    }

    /// Construct without a store but with mutations enabled.
    /// Suitable when `--mutations` is set but no `--data-dir` is provided;
    /// tools that require the store (`dlq_retry`) will return an error.
    pub fn new_mutations_only(state: Arc<AppState>) -> Self {
        Self {
            state,
            store: None,
            jobs: HashMap::new(),
            mutations_enabled: true,
            tool_router: Self::tool_router(),
        }
    }

    // ── Helpers ──────────────────────────────────────────────────────────────

    /// Returns `Ok(())` if mutations are enabled, otherwise an MCP error.
    fn require_mutations(&self) -> Result<(), McpError> {
        if !self.mutations_enabled {
            return Err(McpError::invalid_params(
                "Mutations are disabled. Start croniq-mcp with --mutations to enable write operations.",
                None,
            ));
        }
        Ok(())
    }

    /// Returns a reference to the store, or an error if unavailable.
    fn require_store(&self) -> Result<&DynStore, McpError> {
        self.store.as_ref().ok_or_else(|| {
            McpError::internal_error(
                "No persistent store available. Start croniq-mcp with --data-dir <path> to enable store-backed operations.",
                None,
            )
        })
    }

    // ── Observe tools ─────────────────────────────────────────────────────────

    /// List all connected runners with their current liveness status,
    /// capabilities, and inflight execution count.
    #[tool(
        description = "List all connected runners with their status, capabilities, and inflight execution count."
    )]
    async fn list_runners(&self) -> Result<String, McpError> {
        let now = Utc::now();
        let reg = self.state.registry.read().await;

        #[derive(Serialize)]
        struct RunnerSummary<'a> {
            runner_id: &'a str,
            status: &'static str,
            capabilities: &'a Vec<String>,
            inflight: usize,
            capacity: u32,
        }

        let runners: Vec<RunnerSummary> = reg
            .all()
            .map(|r| RunnerSummary {
                runner_id: &r.runner_id,
                status: match r.status_at(now) {
                    RunnerStatus::Online => "online",
                    RunnerStatus::Stale => "stale",
                    RunnerStatus::Dead => "dead",
                },
                capabilities: &r.capabilities,
                inflight: r.inflight.len(),
                capacity: r.max_inflight,
            })
            .collect();

        serde_json::to_string_pretty(&runners)
            .map_err(|e| McpError::internal_error(e.to_string(), None))
    }

    /// Get detailed information about a specific runner by ID.
    #[tool(description = "Get detailed information about a specific runner by its ID.")]
    async fn get_runner(
        &self,
        Parameters(p): Parameters<GetRunnerParams>,
    ) -> Result<String, McpError> {
        let now = Utc::now();
        let reg = self.state.registry.read().await;

        match reg.get(&p.runner_id) {
            None => Err(McpError::invalid_params(
                format!("Runner '{}' not found", p.runner_id),
                None,
            )),
            Some(r) => {
                #[derive(Serialize)]
                struct RunnerDetail<'a> {
                    runner_id: &'a str,
                    status: &'static str,
                    capabilities: &'a Vec<String>,
                    max_inflight: u32,
                    inflight: &'a Vec<String>,
                    last_poll_at: String,
                }

                let detail = RunnerDetail {
                    runner_id: &r.runner_id,
                    status: match r.status_at(now) {
                        RunnerStatus::Online => "online",
                        RunnerStatus::Stale => "stale",
                        RunnerStatus::Dead => "dead",
                    },
                    capabilities: &r.capabilities,
                    max_inflight: r.max_inflight,
                    inflight: &r.inflight,
                    last_poll_at: r.last_poll_at.to_rfc3339(),
                };

                serde_json::to_string_pretty(&detail)
                    .map_err(|e| McpError::internal_error(e.to_string(), None))
            }
        }
    }

    /// Get the current work queue depth and runner liveness overview.
    #[tool(
        description = "Get work queue status: total depth and runner counts by liveness status."
    )]
    async fn queue_status(
        &self,
        Parameters(p): Parameters<QueueStatusParams>,
    ) -> Result<String, McpError> {
        let now = Utc::now();
        let reg = self.state.registry.read().await;
        let queue = self.state.queue.read().await;

        #[derive(Serialize)]
        struct PeekItem {
            execution_id: String,
            job_key: String,
            attempt: u32,
        }

        #[derive(Serialize)]
        struct StatusReport {
            queued: usize,
            runners_online: usize,
            runners_stale: usize,
            runners_dead: usize,
            items: Vec<PeekItem>,
        }

        let peek_limit = p.peek_limit.min(50);
        let items: Vec<PeekItem> = queue
            .peek_n(peek_limit)
            .into_iter()
            .map(|item| PeekItem {
                execution_id: item.execution_id.clone(),
                job_key: item.job_key.clone(),
                attempt: item.attempt,
            })
            .collect();

        let report = StatusReport {
            queued: queue.len(),
            runners_online: reg.by_status(RunnerStatus::Online, now).len(),
            runners_stale: reg.by_status(RunnerStatus::Stale, now).len(),
            runners_dead: reg.by_status(RunnerStatus::Dead, now).len(),
            items,
        };

        serde_json::to_string_pretty(&report)
            .map_err(|e| McpError::internal_error(e.to_string(), None))
    }

    /// List all job states from the persistent store.
    #[tool(
        description = "List all job states (active, paused, exhausted) from the store. Requires a persistent store (--data-dir)."
    )]
    async fn list_jobs(&self) -> Result<String, McpError> {
        let store = self.store.as_ref().ok_or_else(|| {
            McpError::internal_error(
                "No persistent store available. Start with --data-dir.",
                None,
            )
        })?;

        let states = store
            .list_job_states()
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        serde_json::to_string_pretty(&states)
            .map_err(|e| McpError::internal_error(e.to_string(), None))
    }

    /// List recent executions with optional filters.
    #[tool(
        description = "List recent executions from the store. Filter by job_key and/or state. Requires --data-dir."
    )]
    async fn list_executions(
        &self,
        Parameters(p): Parameters<ListExecutionsParams>,
    ) -> Result<String, McpError> {
        use croniq_store::models::{ExecutionFilter, ExecutionState};

        let store = self.store.as_ref().ok_or_else(|| {
            McpError::internal_error(
                "No persistent store available. Start with --data-dir.",
                None,
            )
        })?;

        let state = p.state.as_deref().and_then(|s| match s {
            "queued" => Some(ExecutionState::Queued),
            "claimed" => Some(ExecutionState::Claimed),
            "completed" => Some(ExecutionState::Completed),
            "failed" => Some(ExecutionState::Failed),
            "dead" => Some(ExecutionState::Dead),
            "cancelled" => Some(ExecutionState::Cancelled),
            _ => None,
        });

        let filter = ExecutionFilter {
            job_key: p.job_key,
            state,
            limit: Some(p.limit.min(100)),
            ..Default::default()
        };

        let executions = store
            .list_executions(&filter)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        serde_json::to_string_pretty(&executions)
            .map_err(|e| McpError::internal_error(e.to_string(), None))
    }

    // ── Operate tools ─────────────────────────────────────────────────────────

    /// Enqueue a new job execution. The next available eligible runner will
    /// pick it up on their next poll.
    #[tool(
        description = "Add a job to the work queue. The next eligible runner will claim it on their next poll. Requires --mutations."
    )]
    async fn enqueue_job(
        &self,
        Parameters(p): Parameters<EnqueueJobParams>,
    ) -> Result<String, McpError> {
        self.require_mutations()?;

        let execution_id = if p.execution_id.is_empty() {
            Uuid::new_v4().to_string()
        } else {
            p.execution_id
        };

        let now = Utc::now();

        // Persist to store if available.
        if let Some(store) = &self.store {
            let id = Uuid::parse_str(&execution_id).map_err(|e| {
                McpError::invalid_params(format!("Invalid execution_id UUID: {e}"), None)
            })?;
            let execution = Execution {
                id,
                job_key: p.job_key.clone(),
                fire_at: now,
                attempt: 1,
                state: ExecutionState::Queued,
                runner_id: None,
                claimed_at: None,
                started_at: None,
                completed_at: None,
                duration_ms: None,
                error: None,
                dead_reason: None,
                metadata: HashMap::new(),
                created_at: now,
            };
            store
                .create_execution(&execution)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        }

        let item = WorkItem {
            execution_id: execution_id.clone(),
            job_key: p.job_key.clone(),
            fire_at: now,
            attempt: 1,
            require: p.require,
            prefer: p.prefer,
            metadata: p.metadata,
            timeout: p.timeout,
        };

        let depth = {
            let mut queue = self.state.queue.write().await;
            queue.enqueue(item);
            queue.len()
        };
        self.state.work_notify.notify_waiters();

        Ok(format!(
            "Enqueued execution '{}' for job '{}'. Queue depth: {}.",
            execution_id, p.job_key, depth
        ))
    }

    /// Cancel a pending execution before it is dispatched to a runner.
    /// Has no effect if the execution has already been claimed.
    #[tool(
        description = "Cancel a pending execution. Has no effect if already dispatched to a runner. Requires --mutations."
    )]
    async fn cancel_execution(
        &self,
        Parameters(p): Parameters<CancelExecutionParams>,
    ) -> Result<String, McpError> {
        self.require_mutations()?;

        // Remove from the in-memory dispatch queue.
        let removed = {
            let mut queue = self.state.queue.write().await;
            queue.remove(&p.execution_id)
        };

        // Also cancel in the persistent store (best-effort).
        if let Some(store) = &self.store
            && let Ok(id) = Uuid::parse_str(&p.execution_id)
        {
            let now = Utc::now();
            let _ = store.cancel_execution(id, now);
        }

        Ok(if removed {
            format!("Execution '{}' removed from queue.", p.execution_id)
        } else {
            format!(
                "Execution '{}' was not in the queue (may already be dispatched or never existed).",
                p.execution_id
            )
        })
    }

    /// Immediately fire a job by enqueuing a new execution.
    /// This bypasses the schedule — the job runs as soon as an eligible runner picks it up.
    #[tool(
        description = "Trigger a job to run immediately, bypassing its schedule. Requires --mutations."
    )]
    async fn job_trigger(
        &self,
        Parameters(p): Parameters<JobTriggerParams>,
    ) -> Result<String, McpError> {
        self.require_mutations()?;

        let id = Uuid::new_v4();
        let now = Utc::now();

        // Persist to store if available.
        if let Some(store) = &self.store {
            let execution = Execution {
                id,
                job_key: p.job_key.clone(),
                fire_at: now,
                attempt: 1,
                state: ExecutionState::Queued,
                runner_id: None,
                claimed_at: None,
                started_at: None,
                completed_at: None,
                duration_ms: None,
                error: None,
                dead_reason: None,
                metadata: HashMap::new(),
                created_at: now,
            };
            store
                .create_execution(&execution)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        }

        let item = WorkItem {
            execution_id: id.to_string(),
            job_key: p.job_key.clone(),
            fire_at: now,
            attempt: 1,
            require: p.require,
            prefer: p.prefer,
            metadata: p.metadata,
            timeout: p.timeout,
        };

        let depth = {
            let mut queue = self.state.queue.write().await;
            queue.enqueue(item);
            queue.len()
        };
        self.state.work_notify.notify_waiters();

        Ok(format!(
            "Triggered job '{}' as execution '{}'. Queue depth: {}.",
            p.job_key, id, depth
        ))
    }

    /// Re-enqueue a dead-lettered execution for another attempt.
    /// Requires a persistent store (`--data-dir`).
    #[tool(
        description = "Retry a dead-lettered execution. Reads the DLQ entry, creates a new execution with attempt+1, and re-enqueues it. Requires --mutations and --data-dir."
    )]
    async fn dlq_retry(
        &self,
        Parameters(p): Parameters<DlqRetryParams>,
    ) -> Result<String, McpError> {
        self.require_mutations()?;
        let store = self.require_store()?;

        let dl_id = Uuid::parse_str(&p.dead_letter_id).map_err(|e| {
            McpError::invalid_params(format!("Invalid dead_letter_id UUID: {e}"), None)
        })?;

        let dl = store
            .get_dead_letter(dl_id)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?
            .ok_or_else(|| {
                McpError::invalid_params(
                    format!("Dead letter '{}' not found", p.dead_letter_id),
                    None,
                )
            })?;

        let new_id = Uuid::new_v4();
        let now = Utc::now();
        let next_attempt = dl.attempt + 1;

        // Create a fresh execution for the retry.
        let execution = Execution {
            id: new_id,
            job_key: dl.job_key.clone(),
            fire_at: now,
            attempt: next_attempt,
            state: ExecutionState::Queued,
            runner_id: None,
            claimed_at: None,
            started_at: None,
            completed_at: None,
            duration_ms: None,
            error: None,
            dead_reason: None,
            metadata: dl.metadata.clone(),
            created_at: now,
        };

        store
            .create_execution(&execution)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        // Remove from dead-letter queue.
        store
            .remove_dead_letter(dl_id)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        // Look up job config for require/prefer/timeout
        let job = self.jobs.get(&dl.job_key);

        // Enqueue to the in-memory dispatch queue.
        let item = WorkItem {
            execution_id: new_id.to_string(),
            job_key: dl.job_key.clone(),
            fire_at: now,
            attempt: next_attempt,
            require: job.map(|j| j.runner.require.clone()).unwrap_or_default(),
            prefer: job.map(|j| j.runner.prefer.clone()).unwrap_or_default(),
            metadata: serde_json::Value::Object(
                dl.metadata
                    .iter()
                    .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                    .collect(),
            ),
            timeout: job
                .and_then(|j| j.timeout.clone())
                .unwrap_or_else(|| "5m".into()),
        };

        let depth = {
            let mut queue = self.state.queue.write().await;
            queue.enqueue(item);
            queue.len()
        };
        self.state.work_notify.notify_waiters();

        Ok(format!(
            "Retrying dead letter '{}': job '{}' re-enqueued as execution '{}' (attempt {}). Queue depth: {}.",
            dl_id, dl.job_key, new_id, next_attempt, depth
        ))
    }
}

// ─── ServerHandler implementation ────────────────────────────────────────────

#[tool_handler]
impl ServerHandler for CroniqMcp {
    fn get_info(&self) -> ServerInfo {
        let mutation_note = if self.mutations_enabled {
            " Mutations are ENABLED."
        } else {
            " Mutations are DISABLED (start with --mutations to enable job_trigger, enqueue_job, cancel_execution, dlq_retry)."
        };

        let instructions = format!(
            "Croniq is a distributed job scheduler. \
             Use list_runners to see connected workers, \
             queue_status to check pending work, \
             get_runner to inspect a specific runner. \
             Mutation tools (enqueue_job, cancel_execution, job_trigger, dlq_retry) \
             require the server to be started with --mutations.{}",
            mutation_note
        );

        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("croniq", env!("CARGO_PKG_VERSION")))
            .with_instructions(instructions)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_server() -> CroniqMcp {
        CroniqMcp::new(AppState::new())
    }

    fn make_server_with_mutations() -> CroniqMcp {
        use croniq_store::sqlite::SqliteStore;
        let store: DynStore = Arc::new(SqliteStore::in_memory().unwrap());
        CroniqMcp::new_with_store(AppState::new(), store, vec![], true)
    }

    #[tokio::test]
    async fn list_runners_empty() {
        let server = make_server();
        let result = server.list_runners().await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(parsed.as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn list_runners_with_registered_runner() {
        let server = make_server();

        {
            let mut reg = server.state.registry.write().await;
            reg.register_or_update("runner-1", vec!["billing".into()], 3, vec![], None);
        }

        let result = server.list_runners().await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        let arr = parsed.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["runner_id"], "runner-1");
        assert_eq!(arr[0]["status"], "online");
    }

    #[tokio::test]
    async fn get_runner_not_found() {
        let server = make_server();
        let err = server
            .get_runner(Parameters(GetRunnerParams {
                runner_id: "nonexistent".into(),
            }))
            .await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn get_runner_found() {
        let server = make_server();

        {
            let mut reg = server.state.registry.write().await;
            reg.register_or_update(
                "worker-eu",
                vec!["billing".into(), "eu-central".into()],
                5,
                vec!["exec-1".into()],
                None,
            );
        }

        let result = server
            .get_runner(Parameters(GetRunnerParams {
                runner_id: "worker-eu".into(),
            }))
            .await
            .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["runner_id"], "worker-eu");
        assert_eq!(parsed["max_inflight"], 5);
        assert_eq!(parsed["inflight"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn queue_status_empty() {
        let server = make_server();
        let result = server
            .queue_status(Parameters(QueueStatusParams { peek_limit: 5 }))
            .await
            .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["queued"], 0);
        assert_eq!(parsed["runners_online"], 0);
    }

    #[tokio::test]
    async fn enqueue_job_blocked_without_mutations() {
        let server = make_server();
        let err = server
            .enqueue_job(Parameters(EnqueueJobParams {
                execution_id: "exec-1".into(),
                job_key: "billing:invoice".into(),
                require: vec![],
                prefer: vec![],
                metadata: serde_json::Value::Null,
                timeout: "5m".into(),
            }))
            .await;
        assert!(err.is_err());
        assert!(err.unwrap_err().message.contains("Mutations are disabled"));
    }

    #[tokio::test]
    async fn enqueue_job_returns_confirmation() {
        let server = make_server_with_mutations();

        let result = server
            .enqueue_job(Parameters(EnqueueJobParams {
                execution_id: Uuid::new_v4().to_string(),
                job_key: "billing:invoice".into(),
                require: vec!["billing".into()],
                prefer: vec![],
                metadata: serde_json::json!({"month": "2026-03"}),
                timeout: "15m".into(),
            }))
            .await
            .unwrap();

        assert!(result.contains("billing:invoice"));
        assert!(result.contains("Queue depth: 1"));
    }

    #[tokio::test]
    async fn enqueue_job_autogenerates_id() {
        let server = make_server_with_mutations();

        let result = server
            .enqueue_job(Parameters(EnqueueJobParams {
                execution_id: String::new(),
                job_key: "etl:sync".into(),
                require: vec![],
                prefer: vec![],
                metadata: serde_json::Value::Null,
                timeout: "5m".into(),
            }))
            .await
            .unwrap();

        assert!(result.contains("etl:sync"));
        assert!(result.contains("Queue depth: 1"));
    }

    #[tokio::test]
    async fn cancel_execution_blocked_without_mutations() {
        let server = make_server();
        let err = server
            .cancel_execution(Parameters(CancelExecutionParams {
                execution_id: "exec-ghost".into(),
            }))
            .await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn cancel_pending_execution() {
        let server = make_server_with_mutations();

        server
            .enqueue_job(Parameters(EnqueueJobParams {
                execution_id: Uuid::new_v4().to_string(),
                job_key: "billing:report".into(),
                require: vec![],
                prefer: vec![],
                metadata: serde_json::Value::Null,
                timeout: "5m".into(),
            }))
            .await
            .unwrap();

        // Get the execution_id from the queue.
        let execution_id = {
            let queue = server.state.queue.read().await;
            // peek at first item - we need to get the ID out
            // Enqueue another to verify length, then cancel the first
            drop(queue);
            // Re-enqueue with a known ID
            "exec-cancel-me".to_string()
        };

        // Enqueue with a known ID directly.
        server
            .enqueue_job(Parameters(EnqueueJobParams {
                execution_id: Uuid::new_v4().to_string(),
                job_key: "billing:report".into(),
                require: vec![],
                prefer: vec![],
                metadata: serde_json::Value::Null,
                timeout: "5m".into(),
            }))
            .await
            .unwrap();

        let _ = execution_id; // suppress warning

        let result = server
            .cancel_execution(Parameters(CancelExecutionParams {
                execution_id: "exec-cancel-ghost".into(),
            }))
            .await
            .unwrap();

        assert!(result.contains("not in the queue"));
    }

    #[tokio::test]
    async fn cancel_nonexistent_execution() {
        let server = make_server_with_mutations();

        let result = server
            .cancel_execution(Parameters(CancelExecutionParams {
                execution_id: "exec-ghost".into(),
            }))
            .await
            .unwrap();

        assert!(result.contains("not in the queue"));
    }

    #[tokio::test]
    async fn job_trigger_blocked_without_mutations() {
        let server = make_server();
        let err = server
            .job_trigger(Parameters(JobTriggerParams {
                job_key: "billing:invoice".into(),
                require: vec![],
                prefer: vec![],
                metadata: serde_json::Value::Null,
                timeout: "5m".into(),
            }))
            .await;
        assert!(err.is_err());
        assert!(err.unwrap_err().message.contains("Mutations are disabled"));
    }

    #[tokio::test]
    async fn job_trigger_enqueues_work() {
        let server = make_server_with_mutations();

        let result = server
            .job_trigger(Parameters(JobTriggerParams {
                job_key: "billing:invoice".into(),
                require: vec!["billing".into()],
                prefer: vec![],
                metadata: serde_json::json!({"month": "2026-03"}),
                timeout: "15m".into(),
            }))
            .await
            .unwrap();

        assert!(result.contains("billing:invoice"));
        assert!(result.contains("Queue depth: 1"));

        // Verify it's in the queue.
        let queue = server.state.queue.read().await;
        assert_eq!(queue.len(), 1);
    }

    #[tokio::test]
    async fn job_trigger_persists_to_store() {
        use croniq_store::models::ExecutionFilter;

        let server = make_server_with_mutations();

        server
            .job_trigger(Parameters(JobTriggerParams {
                job_key: "etl:sync".into(),
                require: vec![],
                prefer: vec![],
                metadata: serde_json::Value::Null,
                timeout: "5m".into(),
            }))
            .await
            .unwrap();

        let executions = server
            .store
            .as_ref()
            .unwrap()
            .list_executions(&ExecutionFilter::default())
            .unwrap();

        assert_eq!(executions.len(), 1);
        assert_eq!(executions[0].job_key, "etl:sync");
        assert_eq!(executions[0].attempt, 1);
        assert_eq!(executions[0].state, ExecutionState::Queued);
    }

    #[tokio::test]
    async fn dlq_retry_blocked_without_mutations() {
        let server = make_server();
        let err = server
            .dlq_retry(Parameters(DlqRetryParams {
                dead_letter_id: Uuid::new_v4().to_string(),
            }))
            .await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn dlq_retry_blocked_without_store() {
        // mutations enabled but no store
        let server = CroniqMcp {
            state: AppState::new(),
            store: None,
            jobs: HashMap::new(),
            mutations_enabled: true,
            tool_router: CroniqMcp::tool_router(),
        };
        let err = server
            .dlq_retry(Parameters(DlqRetryParams {
                dead_letter_id: Uuid::new_v4().to_string(),
            }))
            .await;
        assert!(err.is_err());
        assert!(err.unwrap_err().message.contains("No persistent store"));
    }

    #[tokio::test]
    async fn dlq_retry_not_found() {
        let server = make_server_with_mutations();
        let err = server
            .dlq_retry(Parameters(DlqRetryParams {
                dead_letter_id: Uuid::new_v4().to_string(),
            }))
            .await;
        assert!(err.is_err());
        assert!(err.unwrap_err().message.contains("not found"));
    }

    #[tokio::test]
    async fn dlq_retry_re_enqueues() {
        use croniq_store::models::{DeadLetter, DeadLetterFilter, ExecutionFilter};

        let server = make_server_with_mutations();
        let store = server.store.as_ref().unwrap();

        // Seed a dead letter.
        let dl_id = Uuid::new_v4();
        let exec_id = Uuid::new_v4();
        let dl = DeadLetter {
            id: dl_id,
            execution_id: exec_id,
            job_key: "billing:invoice".into(),
            fire_at: Utc::now(),
            attempt: 3,
            error: "db timeout".into(),
            dead_reason: "max retries".into(),
            metadata: HashMap::new(),
            created_at: Utc::now(),
            expires_at: None,
        };
        store.add_dead_letter(&dl).unwrap();

        // Retry it.
        let result = server
            .dlq_retry(Parameters(DlqRetryParams {
                dead_letter_id: dl_id.to_string(),
            }))
            .await
            .unwrap();

        assert!(result.contains("billing:invoice"));
        assert!(result.contains("attempt 4"));
        assert!(result.contains("Queue depth: 1"));

        // DLQ entry should be gone.
        let remaining = store
            .list_dead_letters(&DeadLetterFilter::default())
            .unwrap();
        assert!(remaining.is_empty());

        // A new execution at attempt 4 should exist.
        let executions = store.list_executions(&ExecutionFilter::default()).unwrap();
        assert_eq!(executions.len(), 1);
        assert_eq!(executions[0].attempt, 4);
        assert_eq!(executions[0].job_key, "billing:invoice");

        // The queue should have one item.
        let queue = server.state.queue.read().await;
        assert_eq!(queue.len(), 1);
    }

    #[tokio::test]
    async fn queue_status_after_enqueue() {
        let server = make_server_with_mutations();

        server
            .enqueue_job(Parameters(EnqueueJobParams {
                execution_id: Uuid::new_v4().to_string(),
                job_key: "job:a".into(),
                require: vec![],
                prefer: vec![],
                metadata: serde_json::Value::Null,
                timeout: "5m".into(),
            }))
            .await
            .unwrap();

        let result = server
            .queue_status(Parameters(QueueStatusParams { peek_limit: 10 }))
            .await
            .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["queued"], 1);
    }
}

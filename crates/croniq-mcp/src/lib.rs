//! croniq-mcp: MCP Server for the Croniq distributed job scheduler.
//!
//! Exposes Croniq's work queue and runner registry to AI assistants via the
//! [Model Context Protocol](https://modelcontextprotocol.io).
//!
//! # Tools
//!
//! | Tool               | Description                                    |
//! |--------------------|------------------------------------------------|
//! | `list_runners`     | All runners with liveness + capabilities       |
//! | `get_runner`       | Single runner detail                           |
//! | `queue_status`     | Queue depth + online runner count              |
//! | `list_jobs`        | List job states from the store (requires --data-dir) |
//! | `get_job`          | Fetch one job (DSL fallback) (requires --data-dir) |
//! | `create_job`       | Create a store-managed job (requires --mutations + --data-dir) |
//! | `update_job`       | Patch mutable job metadata (requires --mutations + --data-dir) |
//! | `delete_job`       | Delete a store-managed job (requires --mutations + --data-dir) |
//! | `activate_job`     | Mark a job active (requires --mutations + --data-dir) |
//! | `deactivate_job`   | Mark a job inactive (requires --mutations + --data-dir) |
//! | `list_executions`  | List recent executions (requires --data-dir) |
//! | `get_execution_logs` | Read captured logs for an execution (requires --data-dir) |
//! | `enqueue_job`      | Schedule a new execution (requires --mutations) |
//! | `cancel_execution` | Remove a pending execution (requires --mutations) |
//! | `job_trigger`      | Fire a job immediately (requires --mutations) |
//! | `list_schedules`   | List store-managed trigger definitions (requires --data-dir) |
//! | `get_schedule`     | Fetch one schedule by trigger UUID (requires --data-dir) |
//! | `create_schedule`  | Create a schedule (requires --mutations + --data-dir) |
//! | `update_schedule`  | Patch a schedule (requires --mutations + --data-dir) |
//! | `delete_schedule`  | Delete a schedule (requires --mutations + --data-dir) |
//! | `list_calendars`   | List persisted calendar definitions (requires --data-dir) |
//! | `get_calendar`     | Fetch one calendar by UUID (requires --data-dir) |
//! | `create_calendar`  | Create a calendar (requires --mutations + --data-dir) |
//! | `update_calendar`  | Patch a calendar (requires --mutations + --data-dir) |
//! | `delete_calendar`  | Delete a calendar (requires --mutations + --data-dir) |
//! | `delete_runner`    | Remove a runner from the registry (requires --mutations) |
//! | `dashboard_forecast` | Project upcoming fires into time buckets (HTTP transport only) |
//! | `list_dead_letters` | List dead-lettered executions (requires --data-dir) |
//! | `get_dead_letter`  | Fetch one dead-letter entry (requires --data-dir) |
//! | `delete_dead_letter` | Drop a dead-letter entry (requires --mutations + --data-dir) |
//! | `dlq_retry`        | Retry a dead-lettered execution (requires --mutations + --data-dir) |
//!
//! # Usage
//!
//! ```rust,no_run
//! use std::sync::Arc;
//! use croniq_mcp::tools::CroniqMcp;
//! use croniq_runner::AppState;
//! use rmcp::{ServiceExt, transport::stdio};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let state = AppState::new();
//!     let server = CroniqMcp::new(Arc::clone(&state));
//!     let service = server.serve(stdio()).await?;
//!     service.waiting().await?;
//!     Ok(())
//! }
//! ```

pub mod tools;

pub use tools::{CroniqMcp, DynStore};

// ─── HTTP transport (Streamable HTTP for in-process embedding) ────────────────

use std::collections::HashMap;
use std::sync::Arc;

use croniq_config::compile::JobConfig;
use croniq_runner::AppState;
use croniq_scheduler::trigger::Trigger;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};

/// Names of the mutation tools, in the order they're registered. Server-side
/// auth middleware uses this list to require an `admin` JWT scope on
/// `tools/call` for any of these — keeping the gating list authoritative in
/// this crate so it stays in sync with [`tools::CroniqMcp`].
pub const MUTATION_TOOL_NAMES: &[&str] = &[
    // Execution / queue mutations
    "enqueue_job",
    "cancel_execution",
    "job_trigger",
    // Job CRUD
    "create_job",
    "update_job",
    "delete_job",
    "activate_job",
    "deactivate_job",
    // Schedule (TriggerDefinition) CRUD
    "create_schedule",
    "update_schedule",
    "delete_schedule",
    // Calendar CRUD
    "create_calendar",
    "update_calendar",
    "delete_calendar",
    // Runner cleanup
    "delete_runner",
    // Dead-letter operations
    "delete_dead_letter",
    "dlq_retry",
];

/// Build a Streamable-HTTP MCP service ready to be `.nest_service`'d under an
/// axum route (e.g. `/mcp`). Each session creates its own [`CroniqMcp`]
/// instance sharing the given `state`/`store`/`jobs` — mutations are wired in
/// at the factory level; per-call scope gating is enforced by the embedding
/// server's auth middleware (see [`MUTATION_TOOL_NAMES`]).
///
/// The `jobs` snapshot is captured at build time. Croniqfile reloads do not
/// propagate to in-flight MCP sessions; restart `croniq-server` to refresh.
pub fn streamable_http_service(
    state: Arc<AppState>,
    store: Option<DynStore>,
    jobs: Vec<JobConfig>,
    triggers: Option<Arc<tokio::sync::RwLock<HashMap<String, Trigger>>>>,
) -> StreamableHttpService<CroniqMcp, LocalSessionManager> {
    let factory = move || {
        let mut server = match store.as_ref() {
            Some(s) => {
                CroniqMcp::new_with_store(Arc::clone(&state), Arc::clone(s), jobs.clone(), true)
            }
            None => CroniqMcp::new_mutations_only(Arc::clone(&state)),
        };
        if let Some(ref t) = triggers {
            server = server.with_triggers(Arc::clone(t));
        }
        Ok::<_, std::io::Error>(server)
    };

    StreamableHttpService::new(
        factory,
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    )
}

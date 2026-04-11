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
//! | `enqueue_job`      | Schedule a new execution (requires --mutations) |
//! | `cancel_execution` | Remove a pending execution (requires --mutations) |
//! | `job_trigger`      | Fire a job immediately (requires --mutations)  |
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

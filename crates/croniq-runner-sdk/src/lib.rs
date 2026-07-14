//! Croniq Runner SDK — build job execution runners in Rust.
//!
//! # Quick start
//!
//! ```rust,no_run
//! use croniq_runner_sdk::{CroniqRunner, ExecutionContext};
//!
//! #[tokio::main]
//! async fn main() {
//!     let runner = CroniqRunner::builder("http://localhost:4000", "my-runner")
//!         .api_key("croniq_abc123")
//!         .max_inflight(5)
//!         .build();
//!
//!     runner.register("billing:invoice", |ctx: ExecutionContext| async move {
//!         tracing::info!(execution_id = %ctx.execution_id, "processing invoice");
//!         Ok(())
//!     }).await;
//!
//!     runner.start().await.unwrap();
//! }
//! ```
//!
//! # Catch-all handler
//!
//! Register a fallback that handles any job key for which no specific handler
//! is registered — see [`examples/catch_all.rs`](https://github.com/nuetzliches/croniq/blob/main/crates/croniq-runner-sdk/examples/catch_all.rs):
//!
//! ```rust,no_run
//! # use croniq_runner_sdk::{CroniqRunner, ExecutionContext};
//! # async fn demo() {
//! # let runner = CroniqRunner::builder("http://localhost:4000", "r").build();
//! runner.set_default_handler(|ctx: ExecutionContext| async move {
//!     tracing::info!(job_key = %ctx.job_key, "handling job");
//!     Ok(())
//! }).await;
//! # }
//! ```
//!
//! # Triggering jobs on demand (producer)
//!
//! The runner above is the *consumer* side. To *fire* a job on demand — e.g. in
//! response to an application event, in addition to the Croniqfile schedule —
//! use the producer-side [`TriggerClient`] (`POST /v1/trigger`). It carries its
//! own `jobs:trigger`-scoped credentials, independent of any runner. See the
//! [`trigger`] module for details.
//!
//! ```rust,no_run
//! use croniq_runner_sdk::TriggerClient;
//!
//! # async fn demo() -> Result<(), Box<dyn std::error::Error>> {
//! let client = TriggerClient::builder("http://localhost:4000")
//!     .api_key("croniq_trigger_key")
//!     .build();
//! let result = client.trigger("billing:invoice-generate").send().await?;
//! # let _ = result;
//! # Ok(())
//! # }
//! ```

pub mod client;
pub(crate) mod enrichment;
pub mod handler;
pub mod identity;
pub mod log_writer;
pub mod runner;
pub mod trigger;

pub use client::{ClientError, WorkEvent};
pub use handler::{ExecutionContext, HandlerError};
pub use identity::resolve_runner_id;
pub use log_writer::LogWriter;
pub use runner::CroniqRunner;
pub use trigger::{TriggerClient, TriggerError, TriggerResult};

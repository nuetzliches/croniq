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

pub mod client;
pub mod handler;
pub mod runner;

pub use client::{ClientError, WorkEvent};
pub use handler::{ExecutionContext, HandlerError};
pub use runner::CroniqRunner;

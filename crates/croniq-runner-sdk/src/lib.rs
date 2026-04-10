//! Croniq Runner SDK — build job execution runners in Rust.
//!
//! # Quick start
//!
//! ```rust,no_run
//! use croniq_runner_sdk::{CroniqRunner, ExecutionContext};
//!
//! #[tokio::main]
//! async fn main() {
//!     let mut runner = CroniqRunner::builder("http://localhost:9090", "my-runner")
//!         .api_key("croniq_abc123")
//!         .max_inflight(5)
//!         .build();
//!
//!     runner.register("billing:invoice", |ctx: ExecutionContext| async move {
//!         tracing::info!(execution_id = %ctx.execution_id, "processing invoice");
//!         Ok(())
//!     });
//!
//!     runner.start().await.unwrap();
//! }
//! ```

pub mod client;
pub mod handler;
pub mod runner;

pub use handler::ExecutionContext;
pub use runner::CroniqRunner;

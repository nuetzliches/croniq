//! croniq-runner: HTTP Pull-API, Runner Registry, Work Queue, Capability-Routing.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │                    croniq-runner                     │
//! │                                                     │
//! │  WorkQueue ──→ CapabilityRouter ──→ RunnerRegistry  │
//! │       ↑                                    ↑        │
//! │       │           HTTP API                 │        │
//! │  POST /v1/poll ──────────────────────────→ │        │
//! │  POST /v1/complete ─────────────────────→  │        │
//! │  GET  /health                               │        │
//! └─────────────────────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```rust,no_run
//! use std::sync::Arc;
//! use croniq_runner::api::{AppState, router};
//!
//! #[tokio::main]
//! async fn main() {
//!     let state = AppState::new();
//!     let app = router(Arc::clone(&state));
//!
//!     let listener = tokio::net::TcpListener::bind("0.0.0.0:9443").await.unwrap();
//!     axum::serve(listener, app).await.unwrap();
//! }
//! ```

pub mod api;
pub mod queue;
pub mod registry;
pub mod router;
pub mod types;

// Convenient re-exports for the most common types.
pub use api::{AppState, router as pull_api_router};
pub use queue::WorkQueue;
pub use registry::RunnerRegistry;
pub use router::CapabilityRouter;
pub use types::{
    CompleteRequest, CompleteResponse, CompletionStatus, HealthResponse, PollRequest, PollResponse,
    Runner, RunnerStatus, RunnerSummary, TriggerRequest, TriggerResponse, WorkAssignment, WorkItem,
};

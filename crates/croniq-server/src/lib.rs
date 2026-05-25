//! croniq-server: orchestrates the full Croniq scheduler runtime.
//!
//! # Architecture
//!
//! ```text
//!                          ┌──────────────────────────────────────────┐
//!    Croniqfile ──parse──► │              croniq-server               │
//!                          │                                          │
//!                          │  ┌────────────┐   ┌──────────────────┐  │
//!                          │  │ Scheduler  │   │   Completion     │  │
//!                          │  │   Loop     │──►│   Processor      │  │
//!                          │  │ (tick/1s)  │   │ (retry / DL)     │  │
//!                          │  └─────┬──────┘   └──────────────────┘  │
//!                          │        │              ┌───────────────┐  │
//!                          │        │              │   Watchdog    │  │
//!                          │        │              │  (sweep/30s)  │  │
//!                          │        │              └───────────────┘  │
//!                          │        │ enqueue             ▲           │
//!                          │        ▼                     │           │
//!                          │  ┌─────────────────────────────────────┐ │
//!                          │  │       HTTP Pull-API (axum)          │ │
//!                          │  │  POST /v1/poll  POST /v1/complete   │ │
//!                          │  │  GET  /health                       │ │
//!                          │  └─────────────────────────────────────┘ │
//!                          │                     ▲                    │
//!                          └─────────────────────┼────────────────────┘
//!                                                │
//!                                          Runner agents
//! ```

pub mod alerts;
pub mod api;
pub mod completion;
pub mod dashboard;
pub mod email;
pub mod loader;
pub mod mcp;
pub mod metrics;
pub mod notify;
pub mod oidc;
pub mod quota;
pub mod reload;
pub mod scheduler;
pub mod store;
pub mod telemetry;
pub mod watchdog;
pub mod watcher;

pub use api::ServerState;
pub use completion::{CompletionEvent, CompletionProcessor, ProcessedOutcome};
pub use loader::{
    LoadError, LoadedConfig, load_file, load_str, restore_queued_executions, restore_trigger_states,
};
pub use reload::{
    ApplyError, ReloadCounters, ReloadDiff, ReloadError, ReloadPlan, apply_plan, apply_plan_direct,
    build_plan,
};
pub use scheduler::{FiredExecution, SchedulerLoop, TickResult};
pub use store::{DynStore, sqlite_store};
pub use watchdog::{WatchdogLoop, WatchdogResult};

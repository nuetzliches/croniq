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

pub mod api;
pub mod completion;
pub mod dashboard;
pub mod loader;
pub mod metrics;
pub mod notify;
pub mod quota;
pub mod scheduler;
pub mod store;
pub mod watcher;
pub mod watchdog;

pub use api::ServerState;
pub use completion::{CompletionEvent, CompletionProcessor, ProcessedOutcome};
pub use loader::{LoadError, LoadedConfig, load_file, load_str, restore_trigger_states, restore_queued_executions};
pub use scheduler::{FiredExecution, SchedulerLoop, TickResult};
pub use store::{DynStore, StoreExt, sqlite_store};
pub use watchdog::{WatchdogLoop, WatchdogResult};

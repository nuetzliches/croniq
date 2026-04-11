//! croniq-bridge: connects the DSL to the runtime.
//!
//! This crate is the translation layer between what the operator *declared*
//! (a `Croniqfile`) and what the runtime *needs* to execute and retry jobs.
//!
//! ```text
//!   Croniqfile (DSL)
//!       │
//!       ▼  croniq-config::compile()
//!   RuntimeConfig / JobConfig
//!       │
//!       ▼  croniq-bridge
//!   ┌───────────────────────────────┐
//!   │  WorkItem  (→ croniq-runner)  │  dispatched to runners via HTTP Pull-API
//!   │  ExecutionPolicy (→ croniq-  │  governs retry / timeout / dead-letter
//!   │               execution)      │
//!   └───────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```rust
//! use croniq_config::{compile::compile, parser::Parser};
//! use croniq_bridge::{job_to_work_item, job_to_execution_policy};
//! use chrono::Utc;
//!
//! let src = r#"
//!     job billing:invoice {
//!         every day at 02:00
//!         timeout 15m
//!         runner { require billing }
//!         retry exponential { max_attempts 5; base 2s; cap 60s }
//!     }
//! "#;
//!
//! let ast = Parser::parse(src).unwrap();
//! let config = compile(&ast);
//!
//! let job = &config.jobs[0];
//! let item = job_to_work_item(job, "exec-001", Utc::now(), 1);
//! let policy = job_to_execution_policy(job);
//!
//! assert_eq!(item.job_key, "billing:invoice");
//! assert_eq!(item.attempt, 1);
//! assert_eq!(item.require, vec!["billing"]);
//! assert_eq!(item.timeout, "15m");
//! assert_eq!(policy.retry.max_attempts, 5);
//! ```

pub mod dispatch;
pub mod policy;

// Convenient re-exports
pub use dispatch::{job_to_execution_policy, job_to_work_item};
pub use policy::{dead_letter_to_policy, retry_config_to_policy, timeout_to_policy};

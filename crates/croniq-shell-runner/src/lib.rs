//! Croniq generic shell / exec runner.
//!
//! Picks up the `__runner_exec` payload stamped by the Croniqfile compiler
//! ([`croniq_config::compile::RunnerExec`]) from the work-assignment metadata
//! and dispatches it to a local subprocess.
//!
//! # Modes
//!
//! - `runner shell { command "<sh-string>" }` — runs as `sh -c "<sh-string>"`,
//!   so users get pipes, redirects, env-substitution, the lot.
//! - `runner exec { args <argv0> <argv1> … }` — direct exec, no shell, no
//!   quoting hazards.
//!
//! # Trust model
//!
//! Anyone with write access to the Croniqfile (or to the JobConfig metadata
//! via the API) can run arbitrary commands as the runner process. Treat the
//! runner pool's filesystem and network namespace as exposed to whoever can
//! ship a Croniqfile change.

pub mod exec;

pub use exec::{Outcome, run};

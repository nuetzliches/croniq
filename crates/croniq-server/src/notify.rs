//! Deprecated.
//!
//! The old `notify_failure()` env-var-only shell hook lived here. As
//! of issue #140 PR-1 the failure-notification path runs through
//! [`crate::alerts`] — a rule-driven evaluator with named channels,
//! throttling, and a persistent delivery log.
//!
//! Back-compat for `CRONIQ_ON_FAILURE_CMD` is preserved: at boot,
//! [`crate::alerts::merge_legacy_env_hook`] synthesises a catch-all
//! shell-channel rule from the env var when no DSL `alerts {}` block
//! is present. Operators see a one-shot deprecation notice on first
//! boot pointing them at the DSL migration.
//!
//! This file is intentionally left in the crate (with no public items)
//! so external callers that referenced `croniq_server::notify` get a
//! clean module-not-empty signal rather than a missing-module error
//! during incremental upgrades.

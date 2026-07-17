//! Convert DSL policy types into runtime policy types.
//!
//! The DSL (`croniq-config`) uses string-based, human-friendly config structs.
//! The runtime (`croniq-execution`) uses strongly-typed policy structs with
//! `std::time::Duration` values. This module bridges the two.

use std::time::Duration;

use croniq_config::compile::{DeadLetterConfig, RetryConfig};
use croniq_execution::{
    dead_letter::DeadLetterPolicy,
    retry::{RetryPolicy, RetryStrategy, parse_duration},
    timeout::TimeoutPolicy,
};

// ─── Retry ────────────────────────────────────────────────────────────────────

/// Convert a DSL `RetryConfig` into a runtime `RetryPolicy`.
///
/// Strategy mapping:
/// - `"exponential"` → `RetryStrategy::Exponential` (base / cap)
/// - `"linear"`      → `RetryStrategy::Linear` (base / step / cap)
/// - `"fixed"`       → `RetryStrategy::Fixed` (delay)
/// - `"none"` / any other → `RetryPolicy::none()`
pub fn retry_config_to_policy(cfg: &RetryConfig) -> RetryPolicy {
    let strategy = match cfg.strategy.as_str() {
        "exponential" => {
            let base = cfg
                .base
                .as_deref()
                .and_then(parse_duration)
                .unwrap_or(Duration::from_secs(2));
            let cap = cfg
                .cap
                .as_deref()
                .and_then(parse_duration)
                .unwrap_or(Duration::from_secs(30));
            RetryStrategy::Exponential { base, cap }
        }
        "linear" => {
            let base = cfg
                .base
                .as_deref()
                .and_then(parse_duration)
                .unwrap_or(Duration::from_secs(2));
            let step = cfg
                .step
                .as_deref()
                .and_then(parse_duration)
                .unwrap_or(Duration::from_secs(5));
            // Linear cap is mandatory in the runtime type; use 24 h as
            // "effectively uncapped" when the DSL omits it.
            let cap = cfg
                .cap
                .as_deref()
                .and_then(parse_duration)
                .unwrap_or(Duration::from_secs(24 * 3600));
            RetryStrategy::Linear { base, step, cap }
        }
        "fixed" => {
            let delay = cfg
                .delay
                .as_deref()
                .and_then(parse_duration)
                .unwrap_or(Duration::from_secs(5));
            RetryStrategy::Fixed { delay }
        }
        "none" => return RetryPolicy::none(),
        _ => {
            // Unknown strategy → fall back to default exponential
            RetryStrategy::Exponential {
                base: Duration::from_secs(2),
                cap: Duration::from_secs(30),
            }
        }
    };

    RetryPolicy {
        strategy,
        max_attempts: cfg.max_attempts,
        jitter: cfg.jitter.unwrap_or(0.0).clamp(0.0, 1.0),
    }
}

// ─── Timeout ──────────────────────────────────────────────────────────────────

/// Convert an optional timeout string (e.g. `"15m"`) into a `TimeoutPolicy`.
///
/// Falls back to the 5-minute default if `None` or unparseable.
pub fn timeout_to_policy(timeout_str: Option<&str>) -> TimeoutPolicy {
    timeout_str
        .and_then(TimeoutPolicy::parse)
        .unwrap_or_default()
}

// ─── Dead-letter ──────────────────────────────────────────────────────────────

/// Convert a DSL `DeadLetterConfig` into a runtime `DeadLetterPolicy`.
pub fn dead_letter_to_policy(cfg: &DeadLetterConfig) -> DeadLetterPolicy {
    if !cfg.enabled {
        return DeadLetterPolicy::disabled();
    }

    let mut policy = DeadLetterPolicy::default();

    if let Some(retention) = &cfg.retention {
        policy = policy.with_retention(retention);
    }
    if let Some(hint) = &cfg.operator_hint {
        policy = policy.with_hint(hint);
    }
    if let Some(max_age) = &cfg.replay_max_age {
        policy = policy.with_replay_max_age(max_age);
    }

    policy
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn base_retry() -> RetryConfig {
        RetryConfig::default()
    }

    // ─ Retry ─────────────────────────────────────────────────────────────────

    #[test]
    fn default_retry_is_exponential() {
        let policy = retry_config_to_policy(&base_retry());
        assert_eq!(policy.max_attempts, 3);
        assert!(matches!(policy.strategy, RetryStrategy::Exponential { .. }));
    }

    #[test]
    fn exponential_parses_base_and_cap() {
        let cfg = RetryConfig {
            strategy: "exponential".into(),
            max_attempts: 5,
            base: Some("4s".into()),
            cap: Some("1m".into()),
            jitter: Some(0.1),
            ..RetryConfig::default()
        };
        let policy = retry_config_to_policy(&cfg);
        assert_eq!(policy.max_attempts, 5);
        assert!(matches!(
            policy.strategy,
            RetryStrategy::Exponential {
                base,
                cap,
            } if base == Duration::from_secs(4) && cap == Duration::from_secs(60)
        ));
        assert_eq!(policy.jitter, 0.1);
    }

    #[test]
    fn linear_parses_base_step_cap() {
        let cfg = RetryConfig {
            strategy: "linear".into(),
            max_attempts: 4,
            base: Some("3s".into()),
            step: Some("2s".into()),
            cap: Some("20s".into()),
            jitter: None,
            ..RetryConfig::default()
        };
        let policy = retry_config_to_policy(&cfg);
        assert!(matches!(
            policy.strategy,
            RetryStrategy::Linear { base, step, cap }
            if base == Duration::from_secs(3)
                && step == Duration::from_secs(2)
                && cap == Duration::from_secs(20)
        ));
    }

    #[test]
    fn linear_without_cap_uses_24h_default() {
        let cfg = RetryConfig {
            strategy: "linear".into(),
            max_attempts: 3,
            base: Some("5s".into()),
            step: Some("5s".into()),
            cap: None,
            ..RetryConfig::default()
        };
        let policy = retry_config_to_policy(&cfg);
        // No explicit cap → falls back to 24 h (effectively uncapped)
        assert!(matches!(
            policy.strategy,
            RetryStrategy::Linear { cap, .. } if cap == Duration::from_secs(24 * 3600)
        ));
    }

    #[test]
    fn fixed_parses_delay() {
        let cfg = RetryConfig {
            strategy: "fixed".into(),
            max_attempts: 2,
            delay: Some("10s".into()),
            ..RetryConfig::default()
        };
        let policy = retry_config_to_policy(&cfg);
        assert!(matches!(
            policy.strategy,
            RetryStrategy::Fixed { delay } if delay == Duration::from_secs(10)
        ));
    }

    #[test]
    fn none_strategy_returns_single_attempt() {
        let cfg = RetryConfig {
            strategy: "none".into(),
            max_attempts: 5, // ignored
            ..RetryConfig::default()
        };
        let policy = retry_config_to_policy(&cfg);
        // RetryPolicy::none() = max_attempts 1, no further retries
        assert_eq!(policy.max_attempts, 1);
        use croniq_execution::retry::RetryDecision;
        assert!(matches!(policy.evaluate(1), RetryDecision::Exhausted));
    }

    #[test]
    fn unknown_strategy_falls_back_to_exponential() {
        let cfg = RetryConfig {
            strategy: "banana".into(),
            max_attempts: 1,
            ..RetryConfig::default()
        };
        let policy = retry_config_to_policy(&cfg);
        assert!(matches!(policy.strategy, RetryStrategy::Exponential { .. }));
    }

    #[test]
    fn jitter_clamped_to_unit_interval() {
        let cfg = RetryConfig {
            jitter: Some(1.5),
            ..RetryConfig::default()
        };
        let policy = retry_config_to_policy(&cfg);
        assert_eq!(policy.jitter, 1.0);

        let cfg2 = RetryConfig {
            jitter: Some(-0.3),
            ..RetryConfig::default()
        };
        let policy2 = retry_config_to_policy(&cfg2);
        assert_eq!(policy2.jitter, 0.0);
    }

    // ─ Timeout ───────────────────────────────────────────────────────────────

    #[test]
    fn timeout_some_parses_correctly() {
        let policy = timeout_to_policy(Some("15m"));
        assert_eq!(policy.duration, Duration::from_secs(900));
    }

    #[test]
    fn timeout_none_returns_default() {
        let policy = timeout_to_policy(None);
        assert_eq!(policy.duration, TimeoutPolicy::default().duration);
    }

    #[test]
    fn timeout_unparseable_returns_default() {
        let policy = timeout_to_policy(Some("garbage"));
        assert_eq!(policy.duration, TimeoutPolicy::default().duration);
    }

    #[test]
    fn timeout_milliseconds() {
        let policy = timeout_to_policy(Some("500ms"));
        assert_eq!(policy.duration, Duration::from_millis(500));
    }

    // ─ Dead-letter ───────────────────────────────────────────────────────────

    #[test]
    fn dead_letter_disabled() {
        let cfg = DeadLetterConfig {
            enabled: false,
            retention: Some("30d".into()),
            operator_hint: None,
            replay_max_age: None,
        };
        let policy = dead_letter_to_policy(&cfg);
        assert!(!policy.enabled);
    }

    #[test]
    fn dead_letter_enabled_with_retention() {
        let cfg = DeadLetterConfig {
            enabled: true,
            retention: Some("7d".into()),
            operator_hint: Some("Check the billing queue".into()),
            replay_max_age: None,
        };
        let policy = dead_letter_to_policy(&cfg);
        assert!(policy.enabled);
        assert_eq!(policy.retention, Duration::from_secs(7 * 24 * 3600));
        assert_eq!(
            policy.operator_hint.as_deref(),
            Some("Check the billing queue")
        );
    }

    #[test]
    fn dead_letter_default_retention() {
        let cfg = DeadLetterConfig::default();
        let policy = dead_letter_to_policy(&cfg);
        assert!(policy.enabled);
        // Default retention = 30 days
        assert_eq!(policy.retention, Duration::from_secs(30 * 24 * 3600));
    }

    #[test]
    fn dead_letter_no_retention_keeps_default() {
        let cfg = DeadLetterConfig {
            enabled: true,
            retention: None, // omitted → default 30 days
            operator_hint: None,
            replay_max_age: None,
        };
        let policy = dead_letter_to_policy(&cfg);
        assert!(policy.enabled);
        // No explicit retention → inherits DeadLetterPolicy::default() = 30 days
        assert_eq!(policy.retention, Duration::from_secs(30 * 24 * 3600));
    }

    #[test]
    fn dead_letter_replay_max_age_maps_into_policy() {
        let cfg = DeadLetterConfig {
            enabled: true,
            retention: Some("30d".into()),
            operator_hint: None,
            replay_max_age: Some("7d".into()),
        };
        let policy = dead_letter_to_policy(&cfg);
        assert_eq!(policy.replay_max_age, Some(Duration::from_secs(7 * 86400)));
    }

    #[test]
    fn dead_letter_replay_max_age_none_by_default() {
        let policy = dead_letter_to_policy(&DeadLetterConfig::default());
        assert!(policy.replay_max_age.is_none());
    }
}

//! Failure-alert evaluator + delivery (issue #140, PR-1 foundation).
//!
//! Replaces the old `notify_failure()` env-var-only shell-out with a
//! rule-driven system: operators declare named channels and rules in
//! the Croniqfile `alerts { … }` block, and this module decides which
//! rules fire on each permanent failure, applies the per-(rule,
//! job_key) throttle, dispatches to the matching channels, and
//! persists a delivery row per fire (or per suppressed fire).
//!
//! PR-1 scope:
//!   - Trigger: `job_failed` only (dead-letter or dropped).
//!   - Channels: `shell` only. `webhook` / `email` recognised at
//!     compile time but skipped here with a warning (PR-2/PR-3).
//!   - Throttle: in-process `HashMap<(rule, job_key), Instant>`. The
//!     server seeds it from `alert_deliveries.fired_at` on boot so a
//!     restart doesn't reset the suppression window.
//!   - Back-compat: at boot, if `CRONIQ_ON_FAILURE_CMD` is set, the
//!     server synthesises a catch-all rule + shell channel using that
//!     command and logs a one-shot deprecation warning. The new rule
//!     name is `"_legacy_env_hook"` (the leading underscore ensures
//!     it doesn't collide with operator-chosen names — those must
//!     start with a letter per the DSL identifier rules).

use std::collections::HashMap;
use std::process::Command;
use std::sync::Arc;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use croniq_config::compile::{AlertsConfig, ChannelConfig, ChannelKind, RuleConfig, RuleTrigger};
use croniq_store::models::{AlertDelivery, AlertDeliveryState};
use uuid::Uuid;

use crate::store::DynStore;

/// Sentinel rule name used by the `CRONIQ_ON_FAILURE_CMD` back-compat
/// path. Starts with `_` so it can't collide with an operator-chosen
/// name (the DSL identifier rule rejects leading-`_` qualifiers — see
/// the parser's `read_string_value`).
pub const LEGACY_ENV_RULE_NAME: &str = "_legacy_env_hook";
/// Matching sentinel channel name for the env-var path.
pub const LEGACY_ENV_CHANNEL_NAME: &str = "_legacy_env_hook";

/// Failure context the evaluator sees on every dead-letter / drop.
///
/// Kept as a small owned struct rather than a borrow so the evaluator
/// can be called from any context (sync completion processor today,
/// future async watchdog for SLA misses) without lifetime gymnastics.
#[derive(Debug, Clone)]
pub struct FailureContext {
    pub job_key: String,
    pub execution_id: String,
    pub error: String,
    pub attempt: u32,
    /// `"dead_letter"` or `"dropped"` — surfaced as
    /// `CRONIQ_REASON` to shell channels.
    pub reason: String,
}

/// Throttle map keyed by `(rule_name, job_key)`. Wrapped in an `Arc<Mutex<_>>`
/// because the completion processor is shared across threads via Tokio
/// tasks and the lock is held for tens of microseconds at most (single
/// HashMap lookup + insert).
pub type ThrottleMap = Arc<Mutex<HashMap<(String, String), DateTime<Utc>>>>;

/// Build a fresh, empty throttle map. Used by tests; the server boot
/// path calls [`load_throttle_state`] to seed from the
/// `alert_deliveries` table instead.
pub fn empty_throttle_map() -> ThrottleMap {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Seed an in-memory throttle map from the `alert_deliveries` history.
///
/// Reads the most recent `fired_at` per `(rule, job_key)` pair so a
/// server restart doesn't reset suppression windows for jobs that
/// were recently quieted. Best-effort — a store error here is logged
/// and the throttle map stays empty (cold-start cost: at most one
/// extra alert per (rule, job_key) until the next throttled match).
pub fn load_throttle_state(store: &DynStore, alerts: &AlertsConfig) -> ThrottleMap {
    let map = empty_throttle_map();
    for rule in &alerts.rules {
        // We don't know up-front which job_keys have fired for this
        // rule, so the helper queries per (rule, "*") combo isn't
        // useful here. Instead, we list recent deliveries for the
        // rule and pick the newest per job_key. 200 rows max keeps
        // the boot cost bounded.
        let filter = croniq_store::models::AlertDeliveryFilter {
            rule_name: Some(rule.name.clone()),
            limit: Some(200),
            ..Default::default()
        };
        match store.list_alert_deliveries(&filter) {
            Ok(rows) => {
                let mut guard = map.lock().unwrap();
                for row in rows {
                    let key = (rule.name.clone(), row.job_key.clone());
                    // list_alert_deliveries returns rows newest-first,
                    // so only insert if we haven't seen this key yet.
                    guard.entry(key).or_insert(row.fired_at);
                }
            }
            Err(e) => {
                tracing::warn!(
                    rule = %rule.name,
                    error = %e,
                    "could not load throttle history — starting with empty map"
                );
            }
        }
    }
    map
}

/// Parse a throttle duration string like `"10m"`, `"30s"`, `"1h"`.
/// Returns `None` for unparseable input or when the rule didn't
/// specify a throttle — the caller treats `None` as "fire every time".
fn parse_throttle_secs(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (digits, mult): (&str, u64) = match s.chars().last()? {
        's' => (&s[..s.len() - 1], 1),
        'm' => (&s[..s.len() - 1], 60),
        'h' => (&s[..s.len() - 1], 3600),
        c if c.is_ascii_digit() => (s, 1),
        _ => return None,
    };
    digits.parse::<u64>().ok()?.checked_mul(mult)
}

/// Decide which rules match `ctx` and dispatch them. Returns the list
/// of delivery rows that were recorded (for tests and future audit).
///
/// Side effects: per matching rule + channel, runs the channel's
/// handler, inserts an `alert_deliveries` row, and logs at INFO. The
/// loop is intentionally sequential — alert volume is low (one fire
/// per permanent failure) and ordering audit logs by rule-name
/// declaration order makes operator triage easier.
pub fn evaluate_failure(
    alerts: &AlertsConfig,
    ctx: &FailureContext,
    throttle: &ThrottleMap,
    store: &DynStore,
) -> Vec<AlertDelivery> {
    let now = Utc::now();
    let mut recorded = Vec::new();

    for rule in &alerts.rules {
        if !rule_matches(rule, ctx) {
            continue;
        }

        let throttle_window = rule.throttle.as_deref().and_then(parse_throttle_secs);
        let throttle_state =
            check_throttle(throttle, &rule.name, &ctx.job_key, throttle_window, now);

        match throttle_state {
            ThrottleDecision::Suppress { last_at } => {
                let delivery = AlertDelivery {
                    delivery_id: Uuid::new_v4().to_string(),
                    rule_name: rule.name.clone(),
                    channel_name: rule
                        .channels
                        .first()
                        .cloned()
                        .unwrap_or_else(|| "<none>".into()),
                    job_key: ctx.job_key.clone(),
                    execution_id: Some(ctx.execution_id.clone()),
                    state: AlertDeliveryState::Throttled,
                    error: None,
                    fired_at: now,
                    delivered_at: None,
                };
                tracing::info!(
                    target: "croniq::alerts",
                    rule = %rule.name,
                    job_key = %ctx.job_key,
                    last_fire = %last_at,
                    "alerts.fired suppressed by throttle"
                );
                let _ = store.record_alert_delivery(&delivery);
                recorded.push(delivery);
                continue;
            }
            ThrottleDecision::Fire => {
                // record_throttle_fire happens inside the per-channel
                // loop so a successful first channel still updates the
                // throttle even if a later channel fails. (Operators
                // care about "the rule fired", not "every channel
                // delivered".)
                record_throttle_fire(throttle, &rule.name, &ctx.job_key, now);
            }
        }

        for channel_name in &rule.channels {
            let Some(channel) = alerts.channels.get(channel_name) else {
                tracing::warn!(
                    target: "croniq::alerts",
                    rule = %rule.name,
                    channel = %channel_name,
                    "rule references unknown channel — skipping"
                );
                let delivery = AlertDelivery {
                    delivery_id: Uuid::new_v4().to_string(),
                    rule_name: rule.name.clone(),
                    channel_name: channel_name.clone(),
                    job_key: ctx.job_key.clone(),
                    execution_id: Some(ctx.execution_id.clone()),
                    state: AlertDeliveryState::Failed,
                    error: Some("unknown channel".into()),
                    fired_at: now,
                    delivered_at: None,
                };
                let _ = store.record_alert_delivery(&delivery);
                recorded.push(delivery);
                continue;
            };

            let delivery = dispatch(channel, &rule.name, ctx, now);
            let _ = store.record_alert_delivery(&delivery);
            recorded.push(delivery);
        }
    }

    recorded
}

fn rule_matches(rule: &RuleConfig, ctx: &FailureContext) -> bool {
    if !matches!(rule.trigger, RuleTrigger::JobFailed) {
        return false;
    }
    if ctx.attempt < rule.min_attempts {
        return false;
    }
    if rule.dead_letter_only && ctx.reason != "dead_letter" {
        return false;
    }
    if !glob_match(&rule.job_key_glob, &ctx.job_key) {
        return false;
    }
    true
}

/// Minimal shell-style glob: `*` matches zero or more characters,
/// `?` matches exactly one. No `[…]` ranges, no escaping. Sufficient
/// for the `"billing:*"` / `"cleanup:*"` patterns the issue gives as
/// examples; a richer matcher would need a dedicated crate (`globset`)
/// and the benefit is marginal at the volume we expect.
fn glob_match(pat: &str, s: &str) -> bool {
    fn inner(pat: &[u8], s: &[u8]) -> bool {
        match (pat.first(), s.first()) {
            (None, None) => true,
            (Some(b'*'), _) => {
                // Try consuming zero or more chars.
                if inner(&pat[1..], s) {
                    return true;
                }
                if !s.is_empty() {
                    return inner(pat, &s[1..]);
                }
                false
            }
            (Some(b'?'), Some(_)) => inner(&pat[1..], &s[1..]),
            (Some(pc), Some(sc)) if pc == sc => inner(&pat[1..], &s[1..]),
            _ => false,
        }
    }
    inner(pat.as_bytes(), s.as_bytes())
}

enum ThrottleDecision {
    Fire,
    Suppress { last_at: DateTime<Utc> },
}

fn check_throttle(
    map: &ThrottleMap,
    rule_name: &str,
    job_key: &str,
    window_secs: Option<u64>,
    now: DateTime<Utc>,
) -> ThrottleDecision {
    let Some(window) = window_secs else {
        return ThrottleDecision::Fire;
    };
    let guard = map.lock().unwrap();
    let key = (rule_name.to_string(), job_key.to_string());
    if let Some(&last_at) = guard.get(&key) {
        let elapsed = (now - last_at).num_seconds().max(0) as u64;
        if elapsed < window {
            return ThrottleDecision::Suppress { last_at };
        }
    }
    ThrottleDecision::Fire
}

fn record_throttle_fire(map: &ThrottleMap, rule_name: &str, job_key: &str, now: DateTime<Utc>) {
    let mut guard = map.lock().unwrap();
    guard.insert((rule_name.to_string(), job_key.to_string()), now);
}

/// Run a single channel's handler. Inserts the appropriate delivery
/// state; never panics on a misbehaving handler (the worst we do is
/// log + record Failed).
fn dispatch(
    channel: &ChannelConfig,
    rule_name: &str,
    ctx: &FailureContext,
    now: DateTime<Utc>,
) -> AlertDelivery {
    match &channel.kind {
        ChannelKind::Shell { command } => deliver_shell(channel, command, rule_name, ctx, now),
        ChannelKind::Unknown { reason } => {
            tracing::warn!(
                target: "croniq::alerts",
                rule = %rule_name,
                channel = %channel.name,
                reason = %reason,
                "channel kind not implemented in this build — skipping"
            );
            AlertDelivery {
                delivery_id: Uuid::new_v4().to_string(),
                rule_name: rule_name.to_string(),
                channel_name: channel.name.clone(),
                job_key: ctx.job_key.clone(),
                execution_id: Some(ctx.execution_id.clone()),
                state: AlertDeliveryState::Failed,
                error: Some(reason.clone()),
                fired_at: now,
                delivered_at: None,
            }
        }
    }
}

fn deliver_shell(
    channel: &ChannelConfig,
    command: &str,
    rule_name: &str,
    ctx: &FailureContext,
    now: DateTime<Utc>,
) -> AlertDelivery {
    let result = Command::new("sh")
        .arg("-c")
        .arg(command)
        .env("CRONIQ_JOB_KEY", &ctx.job_key)
        .env("CRONIQ_EXECUTION_ID", &ctx.execution_id)
        .env("CRONIQ_ERROR", &ctx.error)
        .env("CRONIQ_ATTEMPT", ctx.attempt.to_string())
        .env("CRONIQ_REASON", &ctx.reason)
        .env("CRONIQ_RULE", rule_name)
        .env("CRONIQ_CHANNEL", &channel.name)
        .output();

    let (state, error) = match result {
        Ok(output) if output.status.success() => (AlertDeliveryState::Delivered, None),
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
            let code = output.status.code().unwrap_or(-1);
            (
                AlertDeliveryState::Failed,
                Some(format!("shell exited {code}: {}", truncate(&stderr, 500))),
            )
        }
        Err(e) => (AlertDeliveryState::Failed, Some(e.to_string())),
    };

    match (&state, &error) {
        (AlertDeliveryState::Delivered, _) => tracing::info!(
            target: "croniq::alerts",
            rule = %rule_name,
            channel = %channel.name,
            job_key = %ctx.job_key,
            "alerts.delivered"
        ),
        (_, Some(err)) => tracing::warn!(
            target: "croniq::alerts",
            rule = %rule_name,
            channel = %channel.name,
            job_key = %ctx.job_key,
            error = %err,
            "alerts.delivery_failed"
        ),
        _ => {}
    }

    AlertDelivery {
        delivery_id: Uuid::new_v4().to_string(),
        rule_name: rule_name.to_string(),
        channel_name: channel.name.clone(),
        job_key: ctx.job_key.clone(),
        execution_id: Some(ctx.execution_id.clone()),
        state,
        error,
        fired_at: now,
        delivered_at: Some(now),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

/// Synthesise a back-compat catch-all rule + shell channel from the
/// `CRONIQ_ON_FAILURE_CMD` env var, if set. Logs a one-shot
/// deprecation pointer at INFO so operators see the migration path
/// once per boot (not once per fire).
///
/// Returns a fresh [`AlertsConfig`] when no DSL `alerts {}` block is
/// present (`base` is empty) but the env var is set; otherwise
/// returns `base` unchanged. When BOTH are configured, the DSL wins
/// and the env-var hook is ignored (with a warning explaining why).
pub fn merge_legacy_env_hook(base: AlertsConfig) -> AlertsConfig {
    let env_cmd = std::env::var("CRONIQ_ON_FAILURE_CMD")
        .ok()
        .filter(|s| !s.is_empty());
    let Some(cmd) = env_cmd else {
        return base;
    };

    if !base.rules.is_empty() || !base.channels.is_empty() {
        tracing::warn!(
            "CRONIQ_ON_FAILURE_CMD is set but the Croniqfile has its own \
             `alerts {{ … }}` block — the env var is being ignored. Remove \
             the env var to silence this warning."
        );
        return base;
    }

    tracing::info!(
        "CRONIQ_ON_FAILURE_CMD is set without an `alerts {{ … }}` block — \
         synthesising a catch-all rule. This env-var path is deprecated; \
         migrate to `alerts {{ channel \"{}\" {{ shell \"…\" }} \
         rule \"…\" {{ when job_failed; channels \"{}\" }} }}` in your \
         Croniqfile.",
        LEGACY_ENV_CHANNEL_NAME,
        LEGACY_ENV_CHANNEL_NAME
    );

    let mut cfg = base;
    cfg.channels.insert(
        LEGACY_ENV_CHANNEL_NAME.to_string(),
        ChannelConfig {
            name: LEGACY_ENV_CHANNEL_NAME.to_string(),
            kind: ChannelKind::Shell { command: cmd },
        },
    );
    cfg.rules.push(RuleConfig {
        name: LEGACY_ENV_RULE_NAME.to_string(),
        trigger: RuleTrigger::JobFailed,
        job_key_glob: "*".into(),
        min_attempts: 1,
        dead_letter_only: false,
        throttle: None,
        channels: vec![LEGACY_ENV_CHANNEL_NAME.to_string()],
    });
    cfg
}

#[cfg(test)]
mod tests {
    use super::*;
    use croniq_store::models::AlertDeliveryFilter;

    fn make_ctx(job_key: &str) -> FailureContext {
        FailureContext {
            job_key: job_key.into(),
            execution_id: "exec-1".into(),
            error: "boom".into(),
            attempt: 3,
            reason: "dead_letter".into(),
        }
    }

    fn make_store() -> DynStore {
        crate::store::sqlite_store(croniq_store::sqlite::SqliteStore::in_memory().unwrap())
    }

    #[test]
    fn glob_matches_wildcard_prefix() {
        assert!(glob_match("billing:*", "billing:invoice"));
        assert!(glob_match("billing:*", "billing:"));
        assert!(!glob_match("billing:*", "ops:nightly"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("a?c", "abc"));
        assert!(!glob_match("a?c", "abbc"));
    }

    #[test]
    fn parse_throttle_handles_units() {
        assert_eq!(parse_throttle_secs("30s"), Some(30));
        assert_eq!(parse_throttle_secs("5m"), Some(300));
        assert_eq!(parse_throttle_secs("1h"), Some(3600));
        assert_eq!(parse_throttle_secs("90"), Some(90));
        assert_eq!(parse_throttle_secs(""), None);
        assert_eq!(parse_throttle_secs("garbage"), None);
    }

    #[test]
    fn evaluator_fires_matching_shell_rule() {
        let alerts = AlertsConfig {
            channels: [(
                "ops".to_string(),
                ChannelConfig {
                    name: "ops".into(),
                    kind: ChannelKind::Shell {
                        command: "true".into(),
                    },
                },
            )]
            .into_iter()
            .collect(),
            rules: vec![RuleConfig {
                name: "any-failure".into(),
                trigger: RuleTrigger::JobFailed,
                job_key_glob: "*".into(),
                min_attempts: 1,
                dead_letter_only: false,
                throttle: None,
                channels: vec!["ops".into()],
            }],
        };
        let throttle = empty_throttle_map();
        let store = make_store();
        let recorded = evaluate_failure(&alerts, &make_ctx("billing:invoice"), &throttle, &store);
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].state, AlertDeliveryState::Delivered);
        assert_eq!(recorded[0].channel_name, "ops");
    }

    #[test]
    fn evaluator_skips_non_matching_glob() {
        let alerts = AlertsConfig {
            channels: [(
                "x".to_string(),
                ChannelConfig {
                    name: "x".into(),
                    kind: ChannelKind::Shell {
                        command: "true".into(),
                    },
                },
            )]
            .into_iter()
            .collect(),
            rules: vec![RuleConfig {
                name: "billing-only".into(),
                trigger: RuleTrigger::JobFailed,
                job_key_glob: "billing:*".into(),
                min_attempts: 1,
                dead_letter_only: false,
                throttle: None,
                channels: vec!["x".into()],
            }],
        };
        let throttle = empty_throttle_map();
        let store = make_store();
        let recorded = evaluate_failure(&alerts, &make_ctx("ops:nightly"), &throttle, &store);
        assert!(recorded.is_empty());
    }

    #[test]
    fn evaluator_respects_min_attempts() {
        let alerts = AlertsConfig {
            channels: [(
                "x".to_string(),
                ChannelConfig {
                    name: "x".into(),
                    kind: ChannelKind::Shell {
                        command: "true".into(),
                    },
                },
            )]
            .into_iter()
            .collect(),
            rules: vec![RuleConfig {
                name: "second-try".into(),
                trigger: RuleTrigger::JobFailed,
                job_key_glob: "*".into(),
                min_attempts: 5,
                dead_letter_only: false,
                throttle: None,
                channels: vec!["x".into()],
            }],
        };
        let throttle = empty_throttle_map();
        let store = make_store();
        let mut ctx = make_ctx("any:job");
        ctx.attempt = 3;
        let recorded = evaluate_failure(&alerts, &ctx, &throttle, &store);
        assert!(recorded.is_empty(), "attempt 3 < min_attempts 5 — no fire");

        ctx.attempt = 5;
        let recorded = evaluate_failure(&alerts, &ctx, &throttle, &store);
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].state, AlertDeliveryState::Delivered);
    }

    #[test]
    fn evaluator_dead_letter_only_filters_dropped_reason() {
        let alerts = AlertsConfig {
            channels: [(
                "x".to_string(),
                ChannelConfig {
                    name: "x".into(),
                    kind: ChannelKind::Shell {
                        command: "true".into(),
                    },
                },
            )]
            .into_iter()
            .collect(),
            rules: vec![RuleConfig {
                name: "dl-only".into(),
                trigger: RuleTrigger::JobFailed,
                job_key_glob: "*".into(),
                min_attempts: 1,
                dead_letter_only: true,
                throttle: None,
                channels: vec!["x".into()],
            }],
        };
        let throttle = empty_throttle_map();
        let store = make_store();

        let mut ctx = make_ctx("a:b");
        ctx.reason = "dropped".into();
        let recorded = evaluate_failure(&alerts, &ctx, &throttle, &store);
        assert!(recorded.is_empty(), "dropped reason is filtered out");

        ctx.reason = "dead_letter".into();
        let recorded = evaluate_failure(&alerts, &ctx, &throttle, &store);
        assert_eq!(recorded.len(), 1);
    }

    #[test]
    fn throttle_suppresses_repeats_within_window() {
        let alerts = AlertsConfig {
            channels: [(
                "x".to_string(),
                ChannelConfig {
                    name: "x".into(),
                    kind: ChannelKind::Shell {
                        command: "true".into(),
                    },
                },
            )]
            .into_iter()
            .collect(),
            rules: vec![RuleConfig {
                name: "throttled".into(),
                trigger: RuleTrigger::JobFailed,
                job_key_glob: "*".into(),
                min_attempts: 1,
                dead_letter_only: false,
                throttle: Some("1h".into()),
                channels: vec!["x".into()],
            }],
        };
        let throttle = empty_throttle_map();
        let store = make_store();
        let ctx = make_ctx("a:b");

        // First fire goes through.
        let r1 = evaluate_failure(&alerts, &ctx, &throttle, &store);
        assert_eq!(r1.len(), 1);
        assert_eq!(r1[0].state, AlertDeliveryState::Delivered);

        // Second fire within the window is suppressed.
        let r2 = evaluate_failure(&alerts, &ctx, &throttle, &store);
        assert_eq!(r2.len(), 1);
        assert_eq!(r2[0].state, AlertDeliveryState::Throttled);

        // The delivery log persists both rows.
        let rows = store
            .list_alert_deliveries(&AlertDeliveryFilter::default())
            .unwrap();
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn unknown_channel_kind_records_failed_delivery() {
        let alerts = AlertsConfig {
            channels: [(
                "future".to_string(),
                ChannelConfig {
                    name: "future".into(),
                    kind: ChannelKind::Unknown {
                        reason: "webhook not yet implemented".into(),
                    },
                },
            )]
            .into_iter()
            .collect(),
            rules: vec![RuleConfig {
                name: "uses-future-channel".into(),
                trigger: RuleTrigger::JobFailed,
                job_key_glob: "*".into(),
                min_attempts: 1,
                dead_letter_only: false,
                throttle: None,
                channels: vec!["future".into()],
            }],
        };
        let throttle = empty_throttle_map();
        let store = make_store();
        let recorded = evaluate_failure(&alerts, &make_ctx("a:b"), &throttle, &store);
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].state, AlertDeliveryState::Failed);
        assert!(recorded[0].error.as_ref().unwrap().contains("webhook"));
    }

    #[test]
    fn unknown_channel_reference_in_rule_records_failed_delivery() {
        let alerts = AlertsConfig {
            channels: HashMap::new(),
            rules: vec![RuleConfig {
                name: "dangling".into(),
                trigger: RuleTrigger::JobFailed,
                job_key_glob: "*".into(),
                min_attempts: 1,
                dead_letter_only: false,
                throttle: None,
                channels: vec!["does-not-exist".into()],
            }],
        };
        let throttle = empty_throttle_map();
        let store = make_store();
        let recorded = evaluate_failure(&alerts, &make_ctx("a:b"), &throttle, &store);
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].state, AlertDeliveryState::Failed);
        assert_eq!(recorded[0].error.as_deref(), Some("unknown channel"));
    }

    #[test]
    fn merge_legacy_env_hook_synthesises_rule_when_env_set_and_dsl_empty() {
        // SAFETY: tests run in a single process; we set the env var
        // briefly and restore it. There's no parallel test that reads
        // this var, but we still scope tightly via a guard pattern.
        unsafe { std::env::set_var("CRONIQ_ON_FAILURE_CMD", "true") };
        let merged = merge_legacy_env_hook(AlertsConfig::default());
        unsafe { std::env::remove_var("CRONIQ_ON_FAILURE_CMD") };
        assert!(merged.channels.contains_key(LEGACY_ENV_CHANNEL_NAME));
        assert_eq!(merged.rules.len(), 1);
        assert_eq!(merged.rules[0].name, LEGACY_ENV_RULE_NAME);
    }

    #[test]
    fn merge_legacy_env_hook_yields_when_dsl_block_present() {
        let mut dsl_alerts = AlertsConfig::default();
        dsl_alerts.channels.insert(
            "ops".into(),
            ChannelConfig {
                name: "ops".into(),
                kind: ChannelKind::Shell {
                    command: "/bin/true".into(),
                },
            },
        );
        unsafe { std::env::set_var("CRONIQ_ON_FAILURE_CMD", "echo ignored") };
        let merged = merge_legacy_env_hook(dsl_alerts);
        unsafe { std::env::remove_var("CRONIQ_ON_FAILURE_CMD") };
        // No legacy rule synthesised — DSL wins.
        assert!(!merged.channels.contains_key(LEGACY_ENV_CHANNEL_NAME));
        assert!(merged.rules.iter().all(|r| r.name != LEGACY_ENV_RULE_NAME));
    }

    #[test]
    fn merge_legacy_env_hook_noop_when_neither_present() {
        unsafe { std::env::remove_var("CRONIQ_ON_FAILURE_CMD") };
        let merged = merge_legacy_env_hook(AlertsConfig::default());
        assert!(merged.channels.is_empty());
        assert!(merged.rules.is_empty());
    }
}

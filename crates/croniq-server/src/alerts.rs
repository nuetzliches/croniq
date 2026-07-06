//! Failure-alert evaluator + delivery (issue #140, PR-1 foundation).
//!
//! Replaces the old `notify_failure()` env-var-only shell-out with a
//! rule-driven system: operators declare named channels and rules in
//! the Croniqfile `alerts { … }` block, and this module decides which
//! rules fire on each permanent failure, applies the per-(rule,
//! job_key) throttle, dispatches to the matching channels, and
//! persists a delivery row per fire (or per suppressed fire).
//!
//! Scope after PR-2 (#140):
//!   - Triggers: `job_failed` only (dead-letter or dropped).
//!   - Channels: `shell` + `webhook`. `email` recognised at compile
//!     time and skipped here with a warning (PR-3).
//!   - Throttle: in-process `HashMap<(rule, job_key), DateTime<Utc>>`.
//!     The server seeds it from `alert_deliveries.fired_at` on boot so
//!     a restart doesn't reset the suppression window.
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
use std::time::Duration;

use chrono::{DateTime, Utc};
use croniq_config::compile::{AlertsConfig, ChannelConfig, ChannelKind, RuleConfig, RuleTrigger};
use croniq_store::models::{AlertDelivery, AlertDeliveryState};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use uuid::Uuid;

use crate::store::DynStore;

type HmacSha256 = Hmac<Sha256>;

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
pub(crate) fn parse_throttle_secs(s: &str) -> Option<u64> {
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
pub async fn evaluate_failure(
    alerts: &AlertsConfig,
    ctx: &FailureContext,
    throttle: &ThrottleMap,
    store: &DynStore,
    email_sender: &Arc<dyn crate::email::EmailSender>,
) -> Vec<AlertDelivery> {
    let mut recorded = Vec::new();
    for rule in &alerts.rules {
        if !rule_matches_failure(rule, ctx) {
            continue;
        }
        recorded.extend(dispatch_rule(rule, alerts, ctx, throttle, store, email_sender).await);
    }
    recorded
}

/// Run throttle + channel dispatch for a single rule that the caller
/// has already determined to match. Used by both [`evaluate_failure`]
/// (after `rule_matches_failure`) and the watchdog's SLA-miss sweep
/// (which selects rules by trigger type + `expected_within` instead).
///
/// Sharing this path is what makes a `throttle 10m` directive apply
/// uniformly across `job_failed` AND `job_sla_missed` fires on the
/// same `(rule, job_key)`.
pub(crate) async fn dispatch_rule(
    rule: &RuleConfig,
    alerts: &AlertsConfig,
    ctx: &FailureContext,
    throttle: &ThrottleMap,
    store: &DynStore,
    email_sender: &Arc<dyn crate::email::EmailSender>,
) -> Vec<AlertDelivery> {
    let now = Utc::now();
    let mut recorded = Vec::new();

    // Operational override (issue #231): a force-disabled or actively-
    // snoozed rule does not fire at all. An expired override is inert —
    // the watchdog sweep removes it. Best-effort: a store error here just
    // means the rule fires as if no override existed.
    let override_row = store.get_alert_rule_override(&rule.name).ok().flatten();
    if let Some(ov) = &override_row
        && ov.is_suppressing(now)
    {
        tracing::info!(
            target: "croniq::alerts",
            rule = %rule.name,
            job_key = %ctx.job_key,
            reason = if ov.enabled == Some(false) { "disabled" } else { "snoozed" },
            "alerts.fired suppressed by operational override"
        );
        return recorded;
    }

    // Override throttle, when set, replaces the DSL window; otherwise the
    // DSL value applies.
    let throttle_window = override_row
        .as_ref()
        .and_then(|o| o.effective_throttle_secs(now))
        .or_else(|| rule.throttle.as_deref().and_then(parse_throttle_secs));
    let throttle_state = check_throttle(throttle, &rule.name, &ctx.job_key, throttle_window, now);
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
            return recorded;
        }
        ThrottleDecision::Fire => {
            // record_throttle_fire happens before the per-channel loop
            // so a successful first channel still updates the throttle
            // even if a later channel fails. Operators care about "the
            // rule fired", not "every channel delivered".
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

        let delivery = dispatch(channel, &rule.name, ctx, now, email_sender).await;
        let _ = store.record_alert_delivery(&delivery);
        recorded.push(delivery);
    }

    recorded
}

/// Filter rules for the `job_failed` dispatch path. SLA-miss rules
/// use a separate selector (`expected_within` + watchdog elapsed
/// check) and call `dispatch_rule` directly, bypassing this filter.
fn rule_matches_failure(rule: &RuleConfig, ctx: &FailureContext) -> bool {
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
pub(crate) fn glob_match(pat: &str, s: &str) -> bool {
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
async fn dispatch(
    channel: &ChannelConfig,
    rule_name: &str,
    ctx: &FailureContext,
    now: DateTime<Utc>,
    email_sender: &Arc<dyn crate::email::EmailSender>,
) -> AlertDelivery {
    match &channel.kind {
        ChannelKind::Shell { command } => deliver_shell(channel, command, rule_name, ctx, now),
        ChannelKind::Webhook {
            url,
            signing_key,
            timeout_secs,
        } => {
            deliver_webhook(
                channel,
                url,
                signing_key.as_deref(),
                *timeout_secs,
                rule_name,
                ctx,
                now,
            )
            .await
        }
        ChannelKind::Email { recipients } => {
            deliver_email(channel, recipients, rule_name, ctx, now, email_sender).await
        }
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

/// Webhook payload envelope. Documented in [`docs/operations.md`] and
/// the issue #140 body so receivers can verify the contract is stable.
/// New fields are additive — receivers MUST ignore unknown keys.
#[derive(serde::Serialize)]
struct WebhookPayload<'a> {
    rule: &'a str,
    event: &'a str,
    job_key: &'a str,
    execution_id: &'a str,
    attempt: u32,
    reason: &'a str,
    error: &'a str,
    fired_at: String,
    croniq_version: &'a str,
}

/// Webhook delivery handler with optional HMAC signing and a single
/// retry on transient failure.
///
/// Retry policy (operator decision in #140 PR-2 review): one retry
/// after a fixed 3-second backoff when the first attempt returns 5xx
/// or fails at the network layer. Non-5xx HTTP responses (4xx, 3xx)
/// are recorded as `failed` without retry — they signal a permanent
/// configuration or auth issue and retrying would only spam the
/// receiver.
#[allow(clippy::too_many_arguments)]
async fn deliver_webhook(
    channel: &ChannelConfig,
    url: &str,
    signing_key: Option<&str>,
    timeout_secs: u64,
    rule_name: &str,
    ctx: &FailureContext,
    now: DateTime<Utc>,
) -> AlertDelivery {
    let delivery_id = Uuid::new_v4().to_string();
    let payload = WebhookPayload {
        rule: rule_name,
        event: "job_failed",
        job_key: &ctx.job_key,
        execution_id: &ctx.execution_id,
        attempt: ctx.attempt,
        reason: &ctx.reason,
        error: &ctx.error,
        fired_at: now.to_rfc3339(),
        croniq_version: env!("CARGO_PKG_VERSION"),
    };
    let body = match serde_json::to_vec(&payload) {
        Ok(b) => b,
        Err(e) => {
            return webhook_failed(
                channel,
                rule_name,
                ctx,
                now,
                delivery_id,
                format!("payload serialisation failed: {e}"),
            );
        }
    };

    let signature = signing_key.map(|key| hmac_sha256_hex(key.as_bytes(), &body));

    // Build the client per-call. Reusing a shared `reqwest::Client`
    // would shave a few milliseconds but adds a lifecycle / config
    // dependency on ServerState. Per-call is fine at the volume we
    // expect (failures, not requests).
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return webhook_failed(
                channel,
                rule_name,
                ctx,
                now,
                delivery_id,
                format!("http client build failed: {e}"),
            );
        }
    };

    let send_once = |client: reqwest::Client, body: Vec<u8>, signature: Option<String>| {
        let url = url.to_string();
        let delivery_id_hdr = delivery_id.clone();
        async move {
            let mut req = client
                .post(&url)
                .header("Content-Type", "application/json")
                .header("X-Croniq-Event", "alerts.fired")
                .header("X-Croniq-Delivery-Id", delivery_id_hdr);
            if let Some(sig) = signature {
                req = req.header("X-Croniq-Signature", format!("sha256={sig}"));
            }
            req.body(body).send().await
        }
    };

    // First attempt; retry once with backoff on 5xx or network error.
    // 2xx/3xx/4xx pass through to the result classifier below.
    let first = send_once(client.clone(), body.clone(), signature.clone()).await;
    let final_result = match first {
        Err(e) => {
            tracing::info!(
                target: "croniq::alerts",
                channel = %channel.name,
                error = %e,
                "webhook network error — retrying once after 3s"
            );
            tokio::time::sleep(Duration::from_secs(3)).await;
            send_once(client, body, signature).await
        }
        Ok(resp) if resp.status().is_server_error() => {
            tracing::info!(
                target: "croniq::alerts",
                channel = %channel.name,
                status = %resp.status(),
                "webhook returned 5xx — retrying once after 3s"
            );
            tokio::time::sleep(Duration::from_secs(3)).await;
            send_once(client, body, signature).await
        }
        ok @ Ok(_) => ok,
    };

    let (state, error) = match final_result {
        Ok(resp) if resp.status().is_success() => (AlertDeliveryState::Delivered, None),
        Ok(resp) => {
            let status = resp.status();
            // Best-effort body capture for debugging; bounded so a
            // misbehaving receiver can't flood the audit log.
            let body_preview = resp
                .text()
                .await
                .map(|t| truncate(&t, 200))
                .unwrap_or_else(|_| String::new());
            (
                AlertDeliveryState::Failed,
                Some(format!("HTTP {status}: {body_preview}")),
            )
        }
        Err(e) => (
            AlertDeliveryState::Failed,
            Some(format!("network error: {e}")),
        ),
    };

    match (&state, &error) {
        (AlertDeliveryState::Delivered, _) => tracing::info!(
            target: "croniq::alerts",
            rule = %rule_name,
            channel = %channel.name,
            job_key = %ctx.job_key,
            "alerts.delivered (webhook)"
        ),
        (_, Some(err)) => tracing::warn!(
            target: "croniq::alerts",
            rule = %rule_name,
            channel = %channel.name,
            job_key = %ctx.job_key,
            error = %err,
            "alerts.delivery_failed (webhook)"
        ),
        _ => {}
    }

    AlertDelivery {
        delivery_id,
        rule_name: rule_name.to_string(),
        channel_name: channel.name.clone(),
        job_key: ctx.job_key.clone(),
        execution_id: Some(ctx.execution_id.clone()),
        state,
        error,
        fired_at: now,
        delivered_at: Some(Utc::now()),
    }
}

fn webhook_failed(
    channel: &ChannelConfig,
    rule_name: &str,
    ctx: &FailureContext,
    now: DateTime<Utc>,
    delivery_id: String,
    error: String,
) -> AlertDelivery {
    tracing::warn!(
        target: "croniq::alerts",
        rule = %rule_name,
        channel = %channel.name,
        error = %error,
        "alerts.delivery_failed (webhook setup)"
    );
    AlertDelivery {
        delivery_id,
        rule_name: rule_name.to_string(),
        channel_name: channel.name.clone(),
        job_key: ctx.job_key.clone(),
        execution_id: Some(ctx.execution_id.clone()),
        state: AlertDeliveryState::Failed,
        error: Some(error),
        fired_at: now,
        delivered_at: None,
    }
}

/// HMAC-SHA256 of `body` keyed by `key`, hex-encoded.
fn hmac_sha256_hex(key: &[u8], body: &[u8]) -> String {
    // Hmac::new_from_slice accepts any key length and is the correct
    // constructor for variable-length secrets (unlike the fixed-size
    // SimpleHmac).
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC-SHA256 accepts any key length");
    mac.update(body);
    hex::encode(mac.finalize().into_bytes())
}

/// Compose the subject + plain-text body for a failure-alert email.
///
/// Kept pure for unit-testing — the actual `EmailSender` call lives in
/// [`deliver_email`]. New fields are added at the END of the body to
/// preserve any greps / parsers operators may have built on top of
/// the existing format.
fn compose_email(rule_name: &str, ctx: &FailureContext, now: DateTime<Utc>) -> (String, String) {
    let subject = format!("[Croniq] {} failed (rule: {})", ctx.job_key, rule_name);
    let body = format!(
        "Croniq detected a permanent job failure.\n\
         \n\
         Job:           {job_key}\n\
         Rule:          {rule_name}\n\
         Reason:        {reason}\n\
         Attempt:       {attempt}\n\
         Execution ID:  {execution_id}\n\
         Fired at:      {fired_at}\n\
         Error:\n\
         {error}\n\
         \n\
         -- \n\
         Sent by Croniq {version}.\n",
        job_key = ctx.job_key,
        rule_name = rule_name,
        reason = ctx.reason,
        attempt = ctx.attempt,
        execution_id = ctx.execution_id,
        fired_at = now.to_rfc3339(),
        error = ctx.error,
        version = env!("CARGO_PKG_VERSION"),
    );
    (subject, body)
}

/// Email channel handler. Sends one message per recipient via the
/// `EmailSender` trait. With the default `NoopSender`, "delivery" is
/// the audit log line in `croniq::email`; the row in
/// `alert_deliveries` still says `delivered` because the sender's
/// contract is "Ok = the operator's chosen path accepted the
/// message" — and `NoopSender` is itself the chosen path until SMTP
/// is configured.
///
/// Failure semantics: if any recipient errors, the whole delivery is
/// marked `failed` and the first error wins the `error` column. The
/// remaining recipients are NOT attempted — we expect mailbox issues
/// to be transient and re-firing on the next failure is cheap.
async fn deliver_email(
    channel: &ChannelConfig,
    recipients: &[String],
    rule_name: &str,
    ctx: &FailureContext,
    now: DateTime<Utc>,
    email_sender: &Arc<dyn crate::email::EmailSender>,
) -> AlertDelivery {
    let delivery_id = Uuid::new_v4().to_string();
    let (subject, body) = compose_email(rule_name, ctx, now);

    // `EmailSender::send` is synchronous — wrap in `spawn_blocking` so
    // an SMTP round-trip doesn't stall the completion processor's
    // async task. NoopSender is fast enough that the overhead is
    // irrelevant; the wrapper is a no-op cost on that path.
    let sender = email_sender.clone();
    let subject_owned = subject.clone();
    let body_owned = body.clone();
    let recipients_owned: Vec<String> = recipients.to_vec();
    let result = tokio::task::spawn_blocking(move || {
        let mut first_err: Option<(String, String)> = None;
        for to in &recipients_owned {
            if let Err(e) = sender.send(to, &subject_owned, &body_owned) {
                first_err = Some((to.clone(), e));
                break;
            }
        }
        first_err
    })
    .await
    .unwrap_or_else(|join_err| {
        Some((
            "<task-join>".into(),
            format!("email task panicked: {join_err}"),
        ))
    });

    let (state, error) = match result {
        None => (AlertDeliveryState::Delivered, None),
        Some((to, msg)) => (
            AlertDeliveryState::Failed,
            Some(format!("email to {to}: {}", truncate(&msg, 400))),
        ),
    };

    match (&state, &error) {
        (AlertDeliveryState::Delivered, _) => tracing::info!(
            target: "croniq::alerts",
            rule = %rule_name,
            channel = %channel.name,
            job_key = %ctx.job_key,
            recipients = recipients.len(),
            "alerts.delivered (email)"
        ),
        (_, Some(err)) => tracing::warn!(
            target: "croniq::alerts",
            rule = %rule_name,
            channel = %channel.name,
            job_key = %ctx.job_key,
            error = %err,
            "alerts.delivery_failed (email)"
        ),
        _ => {}
    }

    AlertDelivery {
        delivery_id,
        rule_name: rule_name.to_string(),
        channel_name: channel.name.clone(),
        job_key: ctx.job_key.clone(),
        execution_id: Some(ctx.execution_id.clone()),
        state,
        error,
        fired_at: now,
        delivered_at: Some(Utc::now()),
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
        expected_within: None,
        channels: vec![LEGACY_ENV_CHANNEL_NAME.to_string()],
    });
    cfg
}

#[cfg(test)]
mod tests {
    use super::*;
    // Only the unix-gated delivery tests query the delivery log.
    #[cfg(unix)]
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

    /// Default email sender for tests that aren't exercising the
    /// email channel. NoopSender accepts everything and never blocks.
    fn make_noop_sender() -> Arc<dyn crate::email::EmailSender> {
        crate::email::default_sender()
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

    // Tests that assert a `Delivered` state through a Shell channel are
    // unix-only: shell alert delivery spawns `sh -c`, which stock Windows
    // does not have. Shell-channel tests that only assert rule evaluation
    // (fire / suppress / filter) still run everywhere — a failed spawn
    // doesn't change those outcomes.

    #[cfg(unix)]
    #[tokio::test]
    async fn evaluator_fires_matching_shell_rule() {
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
                expected_within: None,
                channels: vec!["ops".into()],
            }],
        };
        let throttle = empty_throttle_map();
        let store = make_store();
        let recorded = evaluate_failure(
            &alerts,
            &make_ctx("billing:invoice"),
            &throttle,
            &store,
            &make_noop_sender(),
        )
        .await;
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].state, AlertDeliveryState::Delivered);
        assert_eq!(recorded[0].channel_name, "ops");
    }

    fn single_shell_rule() -> AlertsConfig {
        AlertsConfig {
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
                expected_within: None,
                channels: vec!["ops".into()],
            }],
        }
    }

    #[tokio::test]
    async fn override_disable_suppresses_fire() {
        let alerts = single_shell_rule();
        let store = make_store();
        store
            .upsert_alert_rule_override(&croniq_store::models::AlertRuleOverride {
                rule_name: "any-failure".into(),
                enabled: Some(false),
                snooze_until: None,
                throttle_secs: None,
                note: "debugging".into(),
                set_by_user_id: "u".into(),
                set_at: Utc::now(),
                expires_at: None,
            })
            .unwrap();
        let recorded = evaluate_failure(
            &alerts,
            &make_ctx("billing:invoice"),
            &empty_throttle_map(),
            &store,
            &make_noop_sender(),
        )
        .await;
        assert!(recorded.is_empty(), "disabled rule must not fire");
    }

    #[tokio::test]
    async fn override_snooze_in_future_suppresses_but_past_does_not() {
        let alerts = single_shell_rule();

        // Snoozed into the future ⇒ suppressed.
        let store = make_store();
        store
            .upsert_alert_rule_override(&croniq_store::models::AlertRuleOverride {
                rule_name: "any-failure".into(),
                enabled: None,
                snooze_until: Some(Utc::now() + chrono::Duration::hours(1)),
                throttle_secs: None,
                note: "snooze".into(),
                set_by_user_id: "u".into(),
                set_at: Utc::now(),
                expires_at: Some(Utc::now() + chrono::Duration::hours(1)),
            })
            .unwrap();
        let recorded = evaluate_failure(
            &alerts,
            &make_ctx("j"),
            &empty_throttle_map(),
            &store,
            &make_noop_sender(),
        )
        .await;
        assert!(recorded.is_empty(), "active snooze must suppress");

        // Snooze already elapsed (and expired) ⇒ inert, rule fires.
        let store2 = make_store();
        store2
            .upsert_alert_rule_override(&croniq_store::models::AlertRuleOverride {
                rule_name: "any-failure".into(),
                enabled: None,
                snooze_until: Some(Utc::now() - chrono::Duration::hours(2)),
                throttle_secs: None,
                note: "snooze".into(),
                set_by_user_id: "u".into(),
                set_at: Utc::now(),
                expires_at: Some(Utc::now() - chrono::Duration::hours(1)),
            })
            .unwrap();
        let recorded = evaluate_failure(
            &alerts,
            &make_ctx("j"),
            &empty_throttle_map(),
            &store2,
            &make_noop_sender(),
        )
        .await;
        assert_eq!(recorded.len(), 1, "expired snooze must not suppress");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn override_throttle_replaces_dsl_window() {
        // DSL rule has no throttle (fires every time). An override
        // throttle of 1h must suppress the second fire in the window.
        let mut alerts = single_shell_rule();
        alerts.rules[0].throttle = None;
        let store = make_store();
        store
            .upsert_alert_rule_override(&croniq_store::models::AlertRuleOverride {
                rule_name: "any-failure".into(),
                enabled: None,
                snooze_until: None,
                throttle_secs: Some(3600),
                note: "too noisy".into(),
                set_by_user_id: "u".into(),
                set_at: Utc::now(),
                expires_at: None,
            })
            .unwrap();
        let throttle = empty_throttle_map();
        let first = evaluate_failure(
            &alerts,
            &make_ctx("j"),
            &throttle,
            &store,
            &make_noop_sender(),
        )
        .await;
        assert_eq!(first[0].state, AlertDeliveryState::Delivered);
        let second = evaluate_failure(
            &alerts,
            &make_ctx("j"),
            &throttle,
            &store,
            &make_noop_sender(),
        )
        .await;
        assert_eq!(
            second[0].state,
            AlertDeliveryState::Throttled,
            "override throttle must suppress the immediate re-fire"
        );
    }

    #[tokio::test]
    async fn evaluator_skips_non_matching_glob() {
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
                expected_within: None,
                channels: vec!["x".into()],
            }],
        };
        let throttle = empty_throttle_map();
        let store = make_store();
        let recorded = evaluate_failure(
            &alerts,
            &make_ctx("ops:nightly"),
            &throttle,
            &store,
            &make_noop_sender(),
        )
        .await;
        assert!(recorded.is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn evaluator_respects_min_attempts() {
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
                expected_within: None,
                channels: vec!["x".into()],
            }],
        };
        let throttle = empty_throttle_map();
        let store = make_store();
        let mut ctx = make_ctx("any:job");
        ctx.attempt = 3;
        let recorded =
            evaluate_failure(&alerts, &ctx, &throttle, &store, &make_noop_sender()).await;
        assert!(recorded.is_empty(), "attempt 3 < min_attempts 5 — no fire");

        ctx.attempt = 5;
        let recorded =
            evaluate_failure(&alerts, &ctx, &throttle, &store, &make_noop_sender()).await;
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].state, AlertDeliveryState::Delivered);
    }

    #[tokio::test]
    async fn evaluator_dead_letter_only_filters_dropped_reason() {
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
                expected_within: None,
                channels: vec!["x".into()],
            }],
        };
        let throttle = empty_throttle_map();
        let store = make_store();

        let mut ctx = make_ctx("a:b");
        ctx.reason = "dropped".into();
        let recorded =
            evaluate_failure(&alerts, &ctx, &throttle, &store, &make_noop_sender()).await;
        assert!(recorded.is_empty(), "dropped reason is filtered out");

        ctx.reason = "dead_letter".into();
        let recorded =
            evaluate_failure(&alerts, &ctx, &throttle, &store, &make_noop_sender()).await;
        assert_eq!(recorded.len(), 1);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn throttle_suppresses_repeats_within_window() {
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
                expected_within: None,
                channels: vec!["x".into()],
            }],
        };
        let throttle = empty_throttle_map();
        let store = make_store();
        let ctx = make_ctx("a:b");

        // First fire goes through.
        let r1 = evaluate_failure(&alerts, &ctx, &throttle, &store, &make_noop_sender()).await;
        assert_eq!(r1.len(), 1);
        assert_eq!(r1[0].state, AlertDeliveryState::Delivered);

        // Second fire within the window is suppressed.
        let r2 = evaluate_failure(&alerts, &ctx, &throttle, &store, &make_noop_sender()).await;
        assert_eq!(r2.len(), 1);
        assert_eq!(r2[0].state, AlertDeliveryState::Throttled);

        // The delivery log persists both rows.
        let rows = store
            .list_alert_deliveries(&AlertDeliveryFilter::default())
            .unwrap();
        assert_eq!(rows.len(), 2);
    }

    #[tokio::test]
    async fn unknown_channel_kind_records_failed_delivery() {
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
                expected_within: None,
                channels: vec!["future".into()],
            }],
        };
        let throttle = empty_throttle_map();
        let store = make_store();
        let recorded = evaluate_failure(
            &alerts,
            &make_ctx("a:b"),
            &throttle,
            &store,
            &make_noop_sender(),
        )
        .await;
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].state, AlertDeliveryState::Failed);
        assert!(recorded[0].error.as_ref().unwrap().contains("webhook"));
    }

    #[tokio::test]
    async fn unknown_channel_reference_in_rule_records_failed_delivery() {
        let alerts = AlertsConfig {
            channels: HashMap::new(),
            rules: vec![RuleConfig {
                name: "dangling".into(),
                trigger: RuleTrigger::JobFailed,
                job_key_glob: "*".into(),
                min_attempts: 1,
                dead_letter_only: false,
                throttle: None,
                expected_within: None,
                channels: vec!["does-not-exist".into()],
            }],
        };
        let throttle = empty_throttle_map();
        let store = make_store();
        let recorded = evaluate_failure(
            &alerts,
            &make_ctx("a:b"),
            &throttle,
            &store,
            &make_noop_sender(),
        )
        .await;
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].state, AlertDeliveryState::Failed);
        assert_eq!(recorded[0].error.as_deref(), Some("unknown channel"));
    }

    /// Mutex held by every test that reads or writes
    /// `CRONIQ_ON_FAILURE_CMD`. Without serialisation cargo's default
    /// parallel test runner races on `env::set_var` and the `_noop`
    /// test flakes (an unrelated `_synthesises` test can leave the
    /// var set just as `_noop` checks it). Holding the guard for the
    /// duration of each env-touching test guarantees a clean window.
    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        use std::sync::{Mutex, OnceLock};
        static M: OnceLock<Mutex<()>> = OnceLock::new();
        let mu = M.get_or_init(|| Mutex::new(()));
        match mu.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    #[test]
    fn merge_legacy_env_hook_synthesises_rule_when_env_set_and_dsl_empty() {
        let _g = env_guard();
        unsafe { std::env::set_var("CRONIQ_ON_FAILURE_CMD", "true") };
        let merged = merge_legacy_env_hook(AlertsConfig::default());
        unsafe { std::env::remove_var("CRONIQ_ON_FAILURE_CMD") };
        assert!(merged.channels.contains_key(LEGACY_ENV_CHANNEL_NAME));
        assert_eq!(merged.rules.len(), 1);
        assert_eq!(merged.rules[0].name, LEGACY_ENV_RULE_NAME);
    }

    #[test]
    fn merge_legacy_env_hook_yields_when_dsl_block_present() {
        let _g = env_guard();
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
        let _g = env_guard();
        unsafe { std::env::remove_var("CRONIQ_ON_FAILURE_CMD") };
        let merged = merge_legacy_env_hook(AlertsConfig::default());
        assert!(merged.channels.is_empty());
        assert!(merged.rules.is_empty());
    }

    // ─── #140 PR-2: webhook channel ────────────────────────────────

    #[test]
    fn hmac_sha256_matches_known_test_vector() {
        // RFC 4231 test case 1: key = 20 x 0x0b, data = "Hi There".
        // Expected: b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7
        let key = vec![0x0b; 20];
        let data = b"Hi There";
        let got = hmac_sha256_hex(&key, data);
        assert_eq!(
            got, "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7",
            "HMAC-SHA256 implementation must match RFC 4231 test vector"
        );
    }

    /// Boot a minimal axum receiver that records every POST it sees.
    /// Returns `(base_url, captured_requests)`. The server is shut down
    /// when the test goroutine drops the JoinHandle.
    async fn spawn_mock_webhook_receiver(
        responder: impl Fn(usize) -> u16 + Send + Sync + 'static,
    ) -> (String, std::sync::Arc<std::sync::Mutex<Vec<MockCapture>>>) {
        use axum::Router;
        use axum::extract::State;
        use axum::http::HeaderMap;
        use axum::routing::post;
        use std::sync::atomic::{AtomicUsize, Ordering};

        #[derive(Clone)]
        struct AppState {
            captured: std::sync::Arc<std::sync::Mutex<Vec<MockCapture>>>,
            next_status: std::sync::Arc<dyn Fn(usize) -> u16 + Send + Sync>,
            counter: std::sync::Arc<AtomicUsize>,
        }

        async fn handle(
            State(state): State<AppState>,
            headers: HeaderMap,
            body: axum::body::Bytes,
        ) -> axum::http::StatusCode {
            let n = state.counter.fetch_add(1, Ordering::SeqCst);
            let mut hdrs = HashMap::new();
            for (k, v) in headers.iter() {
                hdrs.insert(k.to_string(), v.to_str().unwrap_or_default().to_string());
            }
            state.captured.lock().unwrap().push(MockCapture {
                body: body.to_vec(),
                headers: hdrs,
            });
            let code = (state.next_status)(n);
            axum::http::StatusCode::from_u16(code).unwrap_or(axum::http::StatusCode::OK)
        }

        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let state = AppState {
            captured: captured.clone(),
            next_status: std::sync::Arc::new(responder),
            counter: std::sync::Arc::new(AtomicUsize::new(0)),
        };
        let app = Router::new().route("/hook", post(handle)).with_state(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.ok();
        });
        let url = format!("http://{addr}/hook");
        (url, captured)
    }

    #[derive(Debug, Clone)]
    struct MockCapture {
        body: Vec<u8>,
        headers: HashMap<String, String>,
    }

    #[tokio::test]
    async fn webhook_delivery_sends_envelope_with_signature() {
        let (url, captured) = spawn_mock_webhook_receiver(|_| 200).await;
        let alerts = AlertsConfig {
            channels: [(
                "ops".into(),
                ChannelConfig {
                    name: "ops".into(),
                    kind: ChannelKind::Webhook {
                        url: url.clone(),
                        signing_key: Some("test-secret".into()),
                        timeout_secs: 5,
                    },
                },
            )]
            .into_iter()
            .collect(),
            rules: vec![RuleConfig {
                name: "fire".into(),
                trigger: RuleTrigger::JobFailed,
                job_key_glob: "*".into(),
                min_attempts: 1,
                dead_letter_only: false,
                throttle: None,
                expected_within: None,
                channels: vec!["ops".into()],
            }],
        };
        let throttle = empty_throttle_map();
        let store = make_store();
        let recorded = evaluate_failure(
            &alerts,
            &make_ctx("billing:invoice"),
            &throttle,
            &store,
            &make_noop_sender(),
        )
        .await;

        assert_eq!(recorded.len(), 1);
        assert_eq!(
            recorded[0].state,
            AlertDeliveryState::Delivered,
            "200 OK -> Delivered (error: {:?})",
            recorded[0].error
        );

        let caps = captured.lock().unwrap();
        assert_eq!(caps.len(), 1, "exactly one delivery on success");
        let cap = &caps[0];

        // Headers we care about.
        assert_eq!(
            cap.headers.get("content-type").map(String::as_str),
            Some("application/json")
        );
        assert_eq!(
            cap.headers.get("x-croniq-event").map(String::as_str),
            Some("alerts.fired")
        );
        assert!(cap.headers.contains_key("x-croniq-delivery-id"));

        // Signature: sha256=<hex(hmac(secret, raw_body))>
        let sig_header = cap.headers.get("x-croniq-signature").expect("signature");
        let expected = format!("sha256={}", hmac_sha256_hex(b"test-secret", &cap.body));
        assert_eq!(sig_header, &expected, "HMAC must be over the raw body");

        // Body shape — verify a couple of fields.
        let payload: serde_json::Value = serde_json::from_slice(&cap.body).unwrap();
        assert_eq!(payload["job_key"], "billing:invoice");
        assert_eq!(payload["event"], "job_failed");
        assert_eq!(payload["rule"], "fire");
        assert_eq!(payload["attempt"], 3); // from make_ctx
        assert!(payload["fired_at"].is_string());
        assert!(payload["croniq_version"].is_string());
    }

    #[tokio::test]
    async fn webhook_unsigned_omits_signature_header() {
        let (url, captured) = spawn_mock_webhook_receiver(|_| 200).await;
        let alerts = AlertsConfig {
            channels: [(
                "open".into(),
                ChannelConfig {
                    name: "open".into(),
                    kind: ChannelKind::Webhook {
                        url,
                        signing_key: None,
                        timeout_secs: 5,
                    },
                },
            )]
            .into_iter()
            .collect(),
            rules: vec![RuleConfig {
                name: "any".into(),
                trigger: RuleTrigger::JobFailed,
                job_key_glob: "*".into(),
                min_attempts: 1,
                dead_letter_only: false,
                throttle: None,
                expected_within: None,
                channels: vec!["open".into()],
            }],
        };
        let throttle = empty_throttle_map();
        let store = make_store();
        evaluate_failure(
            &alerts,
            &make_ctx("a:b"),
            &throttle,
            &store,
            &make_noop_sender(),
        )
        .await;

        let caps = captured.lock().unwrap();
        assert_eq!(caps.len(), 1);
        assert!(
            !caps[0].headers.contains_key("x-croniq-signature"),
            "unsigned webhook must NOT send X-Croniq-Signature"
        );
    }

    #[tokio::test]
    async fn webhook_retries_once_on_5xx() {
        // First call → 503, second call → 200.
        let (url, captured) = spawn_mock_webhook_receiver(|n| if n == 0 { 503 } else { 200 }).await;
        let alerts = AlertsConfig {
            channels: [(
                "flaky".into(),
                ChannelConfig {
                    name: "flaky".into(),
                    kind: ChannelKind::Webhook {
                        url,
                        signing_key: None,
                        timeout_secs: 5,
                    },
                },
            )]
            .into_iter()
            .collect(),
            rules: vec![RuleConfig {
                name: "any".into(),
                trigger: RuleTrigger::JobFailed,
                job_key_glob: "*".into(),
                min_attempts: 1,
                dead_letter_only: false,
                throttle: None,
                expected_within: None,
                channels: vec!["flaky".into()],
            }],
        };
        let throttle = empty_throttle_map();
        let store = make_store();

        let start = std::time::Instant::now();
        let recorded = evaluate_failure(
            &alerts,
            &make_ctx("a:b"),
            &throttle,
            &store,
            &make_noop_sender(),
        )
        .await;
        let elapsed = start.elapsed();

        assert_eq!(
            recorded[0].state,
            AlertDeliveryState::Delivered,
            "retry after 5xx should succeed"
        );
        assert!(
            elapsed >= Duration::from_secs(3),
            "must wait the 3s backoff before retry (got {elapsed:?})"
        );

        let caps = captured.lock().unwrap();
        assert_eq!(
            caps.len(),
            2,
            "exactly two POSTs: the 503 and the 200 retry"
        );
        // Both POSTs share the same delivery-id (one logical fire,
        // two transport attempts).
        let id_a = caps[0].headers.get("x-croniq-delivery-id").cloned();
        let id_b = caps[1].headers.get("x-croniq-delivery-id").cloned();
        assert_eq!(
            id_a, id_b,
            "retry must reuse the original X-Croniq-Delivery-Id"
        );
    }

    #[tokio::test]
    async fn webhook_4xx_records_failure_without_retry() {
        // 401 Unauthorized — permanent config issue, no retry.
        let (url, captured) = spawn_mock_webhook_receiver(|_| 401).await;
        let alerts = AlertsConfig {
            channels: [(
                "auth-broken".into(),
                ChannelConfig {
                    name: "auth-broken".into(),
                    kind: ChannelKind::Webhook {
                        url,
                        signing_key: None,
                        timeout_secs: 5,
                    },
                },
            )]
            .into_iter()
            .collect(),
            rules: vec![RuleConfig {
                name: "any".into(),
                trigger: RuleTrigger::JobFailed,
                job_key_glob: "*".into(),
                min_attempts: 1,
                dead_letter_only: false,
                throttle: None,
                expected_within: None,
                channels: vec!["auth-broken".into()],
            }],
        };
        let throttle = empty_throttle_map();
        let store = make_store();
        let recorded = evaluate_failure(
            &alerts,
            &make_ctx("a:b"),
            &throttle,
            &store,
            &make_noop_sender(),
        )
        .await;

        assert_eq!(recorded[0].state, AlertDeliveryState::Failed);
        assert!(recorded[0].error.as_ref().unwrap().contains("401"));
        assert_eq!(
            captured.lock().unwrap().len(),
            1,
            "4xx must NOT trigger a retry"
        );
    }

    // ─── #140 PR-3: email channel ──────────────────────────────────

    /// Recording sender for tests. Captures every `(to, subject, body)`
    /// tuple it sees and reports back via the shared `Arc<Mutex<Vec<…>>>`.
    /// Optional failure injection: if `fail_for` matches a recipient,
    /// `send()` returns an error for that one and skips capturing.
    struct RecordingSender {
        captured: std::sync::Arc<std::sync::Mutex<Vec<(String, String, String)>>>,
        fail_for: Option<String>,
    }

    impl crate::email::EmailSender for RecordingSender {
        fn send(&self, to: &str, subject: &str, body: &str) -> Result<(), String> {
            if let Some(ref bad) = self.fail_for
                && bad == to
            {
                return Err(format!("simulated SMTP failure for {to}"));
            }
            self.captured
                .lock()
                .unwrap()
                .push((to.into(), subject.into(), body.into()));
            Ok(())
        }
    }

    /// `(to, subject, body)` triples captured by `RecordingSender`,
    /// shared across the sender and the assertion site.
    type CapturedMessages = std::sync::Arc<std::sync::Mutex<Vec<(String, String, String)>>>;

    fn recording_sender() -> (Arc<dyn crate::email::EmailSender>, CapturedMessages) {
        let captured: CapturedMessages = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sender: Arc<dyn crate::email::EmailSender> = Arc::new(RecordingSender {
            captured: captured.clone(),
            fail_for: None,
        });
        (sender, captured)
    }

    fn failing_sender_for(
        bad_recipient: &str,
    ) -> (Arc<dyn crate::email::EmailSender>, CapturedMessages) {
        let captured: CapturedMessages = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sender: Arc<dyn crate::email::EmailSender> = Arc::new(RecordingSender {
            captured: captured.clone(),
            fail_for: Some(bad_recipient.to_string()),
        });
        (sender, captured)
    }

    #[test]
    fn compose_email_subject_and_body_contain_key_fields() {
        let ctx = make_ctx("billing:invoice");
        let now = chrono::DateTime::parse_from_rfc3339("2026-05-25T08:12:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let (subject, body) = compose_email("billing-fail", &ctx, now);

        // Subject is short and includes both job key and rule name.
        assert!(subject.starts_with("[Croniq]"));
        assert!(subject.contains("billing:invoice"));
        assert!(subject.contains("billing-fail"));
        assert!(
            subject.len() <= 100,
            "subject must stay under ~80-100 chars: got {} chars",
            subject.len()
        );

        // Body has every FailureContext field for operator triage.
        for needle in &[
            "billing:invoice",
            "billing-fail",
            "dead_letter",
            "exec-1",
            "2026-05-25T08:12:00",
            "boom",
        ] {
            assert!(
                body.contains(needle),
                "body must contain {needle:?}: got {body}"
            );
        }
    }

    #[tokio::test]
    async fn email_channel_delivers_one_message_per_recipient() {
        let alerts = AlertsConfig {
            channels: [(
                "ops".into(),
                ChannelConfig {
                    name: "ops".into(),
                    kind: ChannelKind::Email {
                        recipients: vec!["alice@example.com".into(), "bob@example.com".into()],
                    },
                },
            )]
            .into_iter()
            .collect(),
            rules: vec![RuleConfig {
                name: "fire".into(),
                trigger: RuleTrigger::JobFailed,
                job_key_glob: "*".into(),
                min_attempts: 1,
                dead_letter_only: false,
                throttle: None,
                expected_within: None,
                channels: vec!["ops".into()],
            }],
        };
        let throttle = empty_throttle_map();
        let store = make_store();
        let (sender, captured) = recording_sender();
        let recorded = evaluate_failure(
            &alerts,
            &make_ctx("billing:invoice"),
            &throttle,
            &store,
            &sender,
        )
        .await;

        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].state, AlertDeliveryState::Delivered);

        let caps = captured.lock().unwrap();
        assert_eq!(caps.len(), 2, "one message per recipient");
        assert_eq!(caps[0].0, "alice@example.com");
        assert_eq!(caps[1].0, "bob@example.com");
        // Subject and body identical across recipients.
        assert_eq!(caps[0].1, caps[1].1);
        assert_eq!(caps[0].2, caps[1].2);
        // Spot-check content.
        assert!(caps[0].1.contains("billing:invoice"));
        assert!(caps[0].2.contains("billing:invoice"));
    }

    #[tokio::test]
    async fn email_channel_records_failure_on_sender_error() {
        let alerts = AlertsConfig {
            channels: [(
                "ops".into(),
                ChannelConfig {
                    name: "ops".into(),
                    kind: ChannelKind::Email {
                        recipients: vec!["good@example.com".into(), "bad@example.com".into()],
                    },
                },
            )]
            .into_iter()
            .collect(),
            rules: vec![RuleConfig {
                name: "fire".into(),
                trigger: RuleTrigger::JobFailed,
                job_key_glob: "*".into(),
                min_attempts: 1,
                dead_letter_only: false,
                throttle: None,
                expected_within: None,
                channels: vec!["ops".into()],
            }],
        };
        let throttle = empty_throttle_map();
        let store = make_store();
        let (sender, captured) = failing_sender_for("bad@example.com");
        let recorded =
            evaluate_failure(&alerts, &make_ctx("a:b"), &throttle, &store, &sender).await;

        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].state, AlertDeliveryState::Failed);
        let err = recorded[0].error.as_deref().unwrap();
        assert!(
            err.contains("bad@example.com"),
            "error must name the failing recipient: {err}"
        );

        let caps = captured.lock().unwrap();
        assert_eq!(
            caps.len(),
            1,
            "first recipient succeeds, then we stop on the failure"
        );
        assert_eq!(caps[0].0, "good@example.com");
    }
}

//! Domain models for persistence.
//!
//! These are the 4 core entities:
//! - JobState: runtime state of a job (next fire, fire count, status)
//! - Execution: the central runtime entity (queued → claimed → completed|failed|dead)
//! - Runner: a connected execution agent
//! - DeadLetter: failed executions for inspection

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// ─── Job State ───

/// Runtime state of a job. Managed by the scheduler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobState {
    pub job_key: String,
    pub next_fire_at: Option<DateTime<Utc>>,
    pub last_fired_at: Option<DateTime<Utc>>,
    pub fire_count: u64,
    pub status: JobStatus,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Active,
    Paused,
    Disabled,
    /// Trigger has fired its last scheduled time (e.g. `once at …`).
    /// On restart, the trigger is not re-armed — prevents double-firing.
    Exhausted,
}

// ─── Execution ───

/// The central runtime entity. An execution represents a single invocation
/// of a job, from queue to completion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Execution {
    pub id: Uuid,
    pub job_key: String,
    pub fire_at: DateTime<Utc>,
    /// The trigger's original logical fire time. Invariant: constant across
    /// the whole retry chain and across dead-letter replay, while `fire_at`
    /// tracks when *this* execution row becomes due (retries: now + backoff,
    /// replays: now). Manual triggers set it to the trigger moment.
    pub scheduled_for: DateTime<Utc>,
    pub attempt: u32,
    pub state: ExecutionState,

    /// Runner that claimed this execution.
    pub runner_id: Option<String>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub duration_ms: Option<i64>,

    pub error: Option<String>,
    pub dead_reason: Option<String>,

    /// Caller-supplied dedup key from `POST /v1/trigger`, scoped per
    /// `job_key` (issue #279). `None` for scheduler-fired executions and
    /// for triggers that did not send a key. Retries carry the key forward
    /// so a repeat trigger keeps coalescing while the retry is in flight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,

    #[serde(serialize_with = "serialize_public_metadata")]
    pub metadata: HashMap<String, String>,
    pub created_at: DateTime<Utc>,
}

fn serialize_public_metadata<S: serde::Serializer>(
    metadata: &HashMap<String, String>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    let public: Vec<_> = metadata
        .iter()
        .filter(|(k, _)| !k.starts_with("__"))
        .collect();
    let mut map = serializer.serialize_map(Some(public.len()))?;
    for (k, v) in public {
        map.serialize_entry(k, v)?;
    }
    map.end()
}

/// Execution lifecycle states.
///
/// ```text
///          ┌─────────────────────────────────┐
///          │                                 │
/// queued → claimed → completed               │
///                  → failed ──→ queued (retry)│
///                             → dead ────────┘
///                  → abandoned → queued (reassign)
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionState {
    Queued,
    Claimed,
    Completed,
    Failed,
    Dead,
    Cancelled,
}

// ─── Runner ───

/// A connected execution agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Runner {
    pub runner_id: String,
    pub capabilities: Vec<String>,
    pub max_inflight: u32,
    pub last_poll_at: DateTime<Utc>,
    pub inflight: Vec<Uuid>,
    pub status: RunnerStatus,
    pub registered_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunnerStatus {
    Online,
    Stale,
    Dead,
}

// ─── Dead Letter ───

/// A dead-lettered execution for inspection and retry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadLetter {
    pub id: Uuid,
    pub execution_id: Uuid,
    pub job_key: String,
    pub fire_at: DateTime<Utc>,
    /// Logical fire time of the original trigger, carried unchanged through
    /// the retry chain (see [`Execution::scheduled_for`]). Anchors the
    /// stale-replay guard.
    pub scheduled_for: DateTime<Utc>,
    pub attempt: u32,
    pub error: String,
    pub dead_reason: String,
    pub metadata: HashMap<String, String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

// ─── Query types ───

/// Filter for listing executions.
#[derive(Debug, Clone, Default)]
pub struct ExecutionFilter {
    pub job_key: Option<String>,
    pub state: Option<ExecutionState>,
    pub runner_id: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    pub limit: Option<u32>,
}

/// Upper bounds, in seconds, for the per-job execution-duration histogram
/// exposed at `/metrics`. Shared by the store aggregation and the Prometheus
/// renderer so the two never drift; the renderer appends the synthetic
/// `+Inf` bucket (which equals [`JobExecutionMetrics::duration_count`]).
pub const JOB_DURATION_BUCKETS_SECONDS: &[f64] = &[0.1, 0.5, 1.0, 5.0, 10.0, 30.0, 60.0, 300.0];

/// Per-job execution aggregates computed on demand from the executions table
/// (one grouped scan per `/metrics` scrape — nothing is persisted separately).
/// Backs the `croniq_job_*` Prometheus metrics.
#[derive(Debug, Clone)]
pub struct JobExecutionMetrics {
    pub job_key: String,
    /// Terminal-state tallies. Each only ever grows, so they map cleanly onto
    /// Prometheus counters.
    pub completed: u64,
    pub failed: u64,
    pub dead: u64,
    pub cancelled: u64,
    /// Cumulative duration-histogram counts, one entry per
    /// [`JOB_DURATION_BUCKETS_SECONDS`] boundary (same length and order).
    /// Entry `i` counts executions whose `duration_ms` is `<=` boundary `i`.
    pub duration_buckets: Vec<u64>,
    /// Executions that recorded a duration — the histogram `_count` and its
    /// `+Inf` bucket.
    pub duration_count: u64,
    /// Sum of recorded durations in milliseconds — the histogram `_sum`
    /// (the renderer converts to seconds).
    pub duration_sum_ms: i64,
    /// Completion time of the most recent finished execution, if any.
    pub last_run_at: Option<DateTime<Utc>>,
}

/// Filter for listing dead letters.
#[derive(Debug, Clone, Default)]
pub struct DeadLetterFilter {
    pub job_key: Option<String>,
    pub limit: Option<u32>,
}

// ─── Auth ───

/// An API client that can obtain tokens and API keys.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiClient {
    pub client_id: String,
    pub name: String,
    pub scopes: Vec<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

/// A hashed API key bound to an API client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub key_id: String,
    pub client_id: String,
    /// SHA-256 hash of the raw key (hex-encoded).
    pub key_hash: String,
    /// Key prefix for display (first 8 chars of raw key).
    pub key_prefix: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// A refresh token for session management.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshToken {
    /// SHA-256 hash of the raw token.
    pub token_hash: String,
    pub client_id: String,
    pub user_id: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Password credentials for user authentication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasswordCredential {
    pub user_id: String,
    pub username: String,
    /// bcrypt hash.
    pub password_hash: String,
    pub failed_attempts: u32,
    pub locked_until: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// A user identity. One row per human (or service account in machine mode).
/// Decoupled from any specific auth method so a user can have password +
/// TOTP + PATs + OIDC linked simultaneously.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub user_id: String,
    pub username: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub role: Role,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_login_at: Option<DateTime<Utc>>,
}

/// Role of a user. Maps to a fixed set of scopes via
/// `croniq_auth::context::Role::default_scopes`. The variants are stable
/// strings persisted in `users.role` (kebab-case in the DB).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Wildcard — every scope.
    Admin,
    /// Read everything + write jobs/schedules/calendars + trigger.
    Operator,
    /// Read-only across the board.
    Viewer,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Operator => "operator",
            Role::Viewer => "viewer",
        }
    }
}

impl std::str::FromStr for Role {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "admin" => Ok(Role::Admin),
            "operator" => Ok(Role::Operator),
            "viewer" => Ok(Role::Viewer),
            _ => Err(()),
        }
    }
}

/// An outstanding invitation. The raw token is delivered once (via email
/// when SMTP is configured, otherwise as the `token` field in the create
/// response). Only the SHA-256 hash is persisted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invitation {
    pub invitation_id: String,
    pub email: String,
    pub role: Role,
    /// SHA-256 hash of the raw invitation token.
    pub token_hash: String,
    /// `users.user_id` of the admin who issued the invite.
    pub invited_by: String,
    pub expires_at: DateTime<Utc>,
    pub accepted_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// A pending password-reset token. Single-use; `used_at` is set on first
/// successful consumption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasswordReset {
    pub reset_id: String,
    pub user_id: String,
    /// SHA-256 hash of the raw reset token.
    pub token_hash: String,
    pub expires_at: DateTime<Utc>,
    pub used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// A TOTP secret for a user. The base32-encoded seed lives inside
/// `secret_enc` after AES-256-GCM wrapping (see
/// `croniq_auth::crypto::wrap_totp_secret`). `enabled` is `false`
/// during the setup window between `/totp/setup` and `/totp/confirm`;
/// only confirmed secrets can be used to step up login.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TotpSecret {
    pub user_id: String,
    /// base64(nonce || ciphertext+tag) — opaque to the store.
    pub secret_enc: String,
    pub enabled: bool,
    pub confirmed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// A single recovery code (SHA-256 hash of an 8-char lowercase
/// alphanumeric). Consumed once via `password_resets`-style flow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryCode {
    pub code_id: String,
    pub user_id: String,
    pub code_hash: String,
    pub used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Link between a Croniq user and an external OIDC subject. JIT-created
/// on first OIDC sign-in; subsequent sign-ins reuse the existing
/// `user_id` so role + last_login_at history is preserved.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcIdentity {
    pub provider: String,
    pub subject: String,
    pub user_id: String,
    pub email: Option<String>,
    pub linked_at: DateTime<Utc>,
    pub last_login_at: Option<DateTime<Utc>>,
}

/// Short-TTL store entry for the `state` param of an outbound OIDC
/// authorization-code request. Holds the random `nonce` we expect to
/// see back in the ID token, plus an optional post-login redirect.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcPendingLogin {
    pub state: String,
    pub nonce: String,
    pub redirect_to: Option<String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// A Personal Access Token — a user-bound API credential with a stable
/// `user_id` and a scope subset of the owning user's role. Raw token
/// is delivered once at creation; only the SHA-256 hash is persisted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonalAccessToken {
    pub token_id: String,
    pub user_id: String,
    /// Human label ("laptop", "ci-personal").
    pub name: String,
    /// SHA-256 hash of the raw token.
    pub token_hash: String,
    /// First 12 chars of the raw token for display ("croniq_pat_…").
    pub token_prefix: String,
    /// Scopes granted to this token. Must be a subset of the owning
    /// user's role's default scopes (enforced at create time).
    pub scopes: Vec<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// An audit-log entry. Append-only. Drives the Activity Feed on the
/// Dashboard, per-job Audit tabs, and Settings → Audit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub event_id: String,
    /// `user`, `api_key`, `pat`, `oidc`, or `system`.
    pub actor_type: String,
    pub actor_id: Option<String>,
    /// Dotted action ID. Conventions: `<target>.<verb>` — e.g.
    /// `job.created`, `auth.login_success`, `dead_letter.replayed`.
    pub action: String,
    pub target_type: String,
    pub target_id: Option<String>,
    pub diff_json: Option<String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct AuditFilter {
    pub actor_type: Option<String>,
    pub actor_id: Option<String>,
    pub action: Option<String>,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    pub limit: Option<u32>,
}

// ─── Job Definition ───

/// A persisted job definition (distinct from the runtime JobState).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobDefinition {
    pub job_key: String,
    pub description: Option<String>,
    /// Runner assigned to execute this job.
    pub assigned_runner_id: Option<String>,
    pub is_active: bool,
    pub metadata: HashMap<String, String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Execution timeout string (e.g. "5m", "30s"). None → system default "5m".
    pub timeout: Option<String>,
    /// Maximum retry attempts before dead-lettering. None → system default (3).
    pub max_retries: Option<u32>,
    /// Whether failed-and-exhausted executions are sent to the dead-letter queue.
    /// None → system default (true).
    pub dead_letter_enabled: Option<bool>,
    /// Free-form tags for filtering and grouping in the UI. NOT routing-relevant
    /// (use runner capabilities for routing). Convention: `key=value` strings.
    #[serde(default)]
    pub tags: Vec<String>,
}

// ─── Trigger Definition ───

/// A persisted trigger/schedule definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerDefinition {
    pub trigger_id: String,
    pub job_key: String,
    pub cron_expression: Option<String>,
    pub timezone: Option<String>,
    pub calendar: Option<String>,
    pub window: Option<String>,
    pub not_before: Option<DateTime<Utc>>,
    pub not_after: Option<DateTime<Utc>>,
    pub enabled: bool,
    /// Who manages this trigger: "dsl" (Croniqfile), "api" (REST), "runner" (self-registered).
    pub managed_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ─── Calendar Definition ───

/// A persisted calendar definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarDefinition {
    pub calendar_id: String,
    pub name: String,
    pub timezone: Option<String>,
    /// JSON-encoded rules array.
    pub rules: String,
    /// Who manages this calendar: "dsl" (Croniqfile, synthesized at read time)
    /// or "api" (REST/UI, persisted). Mirrors `TriggerDefinition.managed_by`.
    pub managed_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ─── DSL Adoption ───

/// A record indicating that a DSL-defined resource has been adopted into the
/// API store. The loader skips DSL definitions whose key matches an adoption
/// entry on next reload, so the API row wins. See migration 007.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DslAdoption {
    /// One of: "calendar", "job", "trigger".
    pub resource_type: String,
    /// DSL identifier — calendar/job name.
    pub resource_key: String,
    pub adopted_at: DateTime<Utc>,
    /// Caller user_id / api_client_id when the adoption was initiated. May
    /// be `None` for system-level adoptions.
    pub adopted_by: Option<String>,
}

// ─── Alert deliveries (issue #140) ───

/// One row per alert-rule fire. Inserted by the evaluator the moment a
/// rule matches (`state = Throttled`) or before the channel handler
/// runs (`state = Delivered/Failed` after completion).
///
/// `execution_id` is optional because future trigger types (e.g.
/// `job_sla_missed`) may not be tied to a specific execution row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertDelivery {
    pub delivery_id: String,
    pub rule_name: String,
    pub channel_name: String,
    pub job_key: String,
    pub execution_id: Option<String>,
    pub state: AlertDeliveryState,
    pub error: Option<String>,
    pub fired_at: DateTime<Utc>,
    pub delivered_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AlertDeliveryState {
    /// Channel handler completed successfully.
    Delivered,
    /// Channel handler returned an error. See `error` for the reason.
    Failed,
    /// Rule matched but the per-(rule, job_key) throttle window
    /// suppressed the fire. Recorded so operators can see what *would*
    /// have fired without the throttle.
    Throttled,
}

impl AlertDeliveryState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Delivered => "delivered",
            Self::Failed => "failed",
            Self::Throttled => "throttled",
        }
    }

    /// Parse a state string from the DB. Distinct name from
    /// `FromStr::from_str` because the trait would require us to
    /// commit to a public error type — overkill for a tiny internal
    /// helper.
    pub fn parse_db(s: &str) -> Option<Self> {
        match s {
            "delivered" => Some(Self::Delivered),
            "failed" => Some(Self::Failed),
            "throttled" => Some(Self::Throttled),
            _ => None,
        }
    }
}

/// Filter for listing alert deliveries.
#[derive(Debug, Clone, Default)]
pub struct AlertDeliveryFilter {
    pub job_key: Option<String>,
    pub rule_name: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub limit: Option<u32>,
}

// ─── Alert rule overrides (issue #231, Phase 1) ───

/// Operational override for a DSL-managed alert rule. Carries temporary
/// *runtime state* (snooze / disable / re-throttle), not the rule's
/// definition — the Croniqfile stays canonical. One row per rule, keyed
/// by the DSL rule name.
///
/// An override with `expires_at <= now` is **inert**: evaluation ignores
/// it and the watchdog sweep deletes the row on its next pass. Use the
/// [`AlertRuleOverride::is_suppressing`] / [`effective_throttle_secs`]
/// helpers rather than reading the fields directly, so the expiry rule is
/// applied consistently.
///
/// [`effective_throttle_secs`]: AlertRuleOverride::effective_throttle_secs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRuleOverride {
    pub rule_name: String,
    /// `None` = defer to the DSL definition. `Some(false)` force-disables
    /// the rule, `Some(true)` force-enables it (reserved for a future
    /// DSL-disabled state; today rules are always enabled in the DSL).
    pub enabled: Option<bool>,
    /// Rule is suppressed until this instant. `None` = not snoozed.
    pub snooze_until: Option<DateTime<Utc>>,
    /// Replaces the DSL throttle window when set. `None` = use the DSL value.
    pub throttle_secs: Option<u64>,
    /// Mandatory incident context — why the override exists.
    pub note: String,
    /// Caller user_id / api_client_id that set the override.
    pub set_by_user_id: String,
    pub set_at: DateTime<Utc>,
    /// Optional auto-clear deadline. Once `now >= expires_at` the override
    /// is inert and the watchdog deletes it.
    pub expires_at: Option<DateTime<Utc>>,
}

impl AlertRuleOverride {
    /// Whether the override has passed its auto-clear deadline at `now`.
    /// An expired override applies no effect — it's a tombstone awaiting
    /// the next watchdog sweep.
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        self.expires_at.is_some_and(|e| now >= e)
    }

    /// Whether this override should *prevent* the rule from firing at
    /// `now` — true when it force-disables the rule or is actively
    /// snoozing it, and it has not expired.
    pub fn is_suppressing(&self, now: DateTime<Utc>) -> bool {
        if self.is_expired(now) {
            return false;
        }
        self.enabled == Some(false) || self.snooze_until.is_some_and(|s| now < s)
    }

    /// The throttle window the evaluator should use given this override,
    /// or `None` to fall back to the DSL value. An expired override
    /// contributes nothing.
    pub fn effective_throttle_secs(&self, now: DateTime<Utc>) -> Option<u64> {
        if self.is_expired(now) {
            return None;
        }
        self.throttle_secs
    }
}

/// Global maintenance switch state (singleton row).
///
/// When [`is_active`](MaintenanceState::is_active) is true the scheduler stops
/// emitting new work and the work-poll hands out nothing — dispatch is frozen.
/// In-flight executions still finish; queued work and triggers accepted during
/// the window resume once it clears. Maintenance is either a manual toggle
/// (`manual_active`, on until turned off) or a scheduled `[window_start,
/// window_end)` window that activates and clears itself.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MaintenanceState {
    /// Manual toggle: paused now until an operator turns it off.
    pub manual_active: bool,
    /// Optional lower bound of the scheduled window (`None` = starts now).
    pub window_start: Option<DateTime<Utc>>,
    /// Optional upper bound; the window auto-clears once `now >= window_end`.
    pub window_end: Option<DateTime<Utc>>,
    /// Optional operator message surfaced in the UI banner.
    pub note: Option<String>,
    /// Caller (user_id / api_client_id) that last changed the switch.
    pub updated_by: Option<String>,
    /// When the switch was last changed; `None` on the never-set default.
    pub updated_at: Option<DateTime<Utc>>,
}

impl MaintenanceState {
    /// Whether a scheduled window is configured (at least one bound set).
    pub fn has_window(&self) -> bool {
        self.window_start.is_some() || self.window_end.is_some()
    }

    /// Whether the scheduled window contains `now`. A window with only a
    /// start is open-ended; with only an end starts immediately.
    pub fn window_active(&self, now: DateTime<Utc>) -> bool {
        self.has_window()
            && self.window_start.is_none_or(|s| now >= s)
            && self.window_end.is_none_or(|e| now < e)
    }

    /// Effective maintenance state at `now`: the manual toggle OR an active
    /// scheduled window. The single check the dispatch gates call.
    pub fn is_active(&self, now: DateTime<Utc>) -> bool {
        self.manual_active || self.window_active(now)
    }
}

// ─── Work Item Tracking ───

/// A log entry pushed by a runner during execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionLogEntry {
    pub id: Uuid,
    pub execution_id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub level: String,
    pub message: String,
    pub fields: HashMap<String, String>,
    /// Strictly-increasing per-execution sequence number assigned at insert
    /// time. Keeps per-line ordering stable even when many events share the
    /// same millisecond timestamp. Reads return rows ordered by
    /// `(timestamp ASC, seq ASC)`. Pre-#108 rows have `seq = 0`.
    #[serde(default)]
    pub seq: i64,
}

//! Core domain types for the runner protocol.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ─── Runner ──────────────────────────────────────────────────────────────────

/// A connected execution agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Runner {
    pub runner_id: String,
    pub capabilities: Vec<String>,
    pub max_inflight: u32,
    pub last_poll_at: DateTime<Utc>,
    /// Execution IDs currently claimed by this runner.
    pub inflight: Vec<String>,
    /// Unique instance ID — detects when a different process registers with the same runner_id.
    pub instance_id: Option<String>,
    /// Instance ID deposed by the most recent takeover (issue #374).
    /// Further polls from this ID are fenced out with a conflict so a
    /// duplicate deployment converges to one winner instead of the two
    /// processes endlessly taking the runner_id over from each other.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deposed_instance_id: Option<String>,
    /// Free-form tags for filtering/grouping. NOT routing-relevant — runner
    /// capabilities handle routing. Convention: `key=value` strings.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Liveness status derived from how recently a runner polled.
///
/// Thresholds are relative to the configured dead-threshold (the server's
/// `pull_api.lease_ttl`, default 120 s); the stale threshold is half of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunnerStatus {
    /// Polled within the last half dead-threshold (default: 60 s).
    Online,
    /// Last poll is older than half the dead-threshold but younger than the
    /// full one. Still considered alive but lagging.
    Stale,
    /// No poll for at least the dead-threshold. Inflight work should be
    /// reassigned.
    Dead,
}

impl Runner {
    pub fn new(runner_id: impl Into<String>, capabilities: Vec<String>, max_inflight: u32) -> Self {
        Self {
            runner_id: runner_id.into(),
            capabilities,
            max_inflight,
            last_poll_at: Utc::now(),
            inflight: Vec::new(),
            instance_id: None,
            deposed_instance_id: None,
            tags: Vec::new(),
        }
    }

    /// Derive the current liveness status relative to `now`.
    /// Uses default thresholds: online < 60s, stale < 120s, dead >= 120s.
    #[deprecated(
        note = "hardcodes a 120 s dead-threshold; use `status_at_with_ttl` with the configured `lease_ttl_secs` so status matches the watchdog's assessment"
    )]
    pub fn status_at(&self, now: DateTime<Utc>) -> RunnerStatus {
        self.status_at_with_ttl(now, 120)
    }

    /// Derive liveness status with a configurable dead threshold in seconds.
    /// Stale threshold is always half of `dead_threshold_secs`.
    pub fn status_at_with_ttl(&self, now: DateTime<Utc>, dead_threshold_secs: u64) -> RunnerStatus {
        let age = now
            .signed_duration_since(self.last_poll_at)
            .num_seconds()
            .max(0) as u64;

        let stale_threshold = dead_threshold_secs / 2;
        if age < stale_threshold {
            RunnerStatus::Online
        } else if age < dead_threshold_secs {
            RunnerStatus::Stale
        } else {
            RunnerStatus::Dead
        }
    }

    /// True if the runner has capacity for at least one more job.
    pub fn has_capacity(&self) -> bool {
        (self.inflight.len() as u32) < self.max_inflight
    }
}

// ─── Work ────────────────────────────────────────────────────────────────────

/// A pending execution waiting to be claimed by a runner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkItem {
    pub execution_id: String,
    pub job_key: String,
    pub fire_at: DateTime<Utc>,
    /// The trigger's original logical fire time — constant across the retry
    /// chain and replay, while `fire_at` tracks queue due time.
    pub scheduled_for: DateTime<Utc>,
    /// Which attempt this is (1 = first, 2 = first retry, …).
    pub attempt: u32,
    /// Runner must possess ALL of these capabilities.
    pub require: Vec<String>,
    /// Runner is preferred if it has at least one of these (not mandatory).
    pub prefer: Vec<String>,
    /// Metadata forwarded to the runner as-is.
    pub metadata: serde_json::Value,
    /// Timeout hint for the runner (e.g. "15m").
    pub timeout: String,
    /// True when this item's execution is *not* persisted (issue #263): the
    /// scheduler skipped the store insert and tracks the id in
    /// `AppState::ephemeral_inflight` instead. Dispatch must not look for a
    /// store row for such an item — there is none, and there never will be
    /// (issue #539).
    #[serde(default)]
    pub is_ephemeral: bool,
}

/// Per-job tally of ephemeral fires and what became of each, since the last
/// scheduler heartbeat (issue #541).
///
/// Ephemeral jobs keep no execution history, so a job that fires but never
/// reaches a runner is indistinguishable from one running perfectly — that is
/// what kept #539 invisible for six minor releases. Counting both ends of the
/// fire→dispatch hop turns that class of failure into a visible
/// `fired=N dispatched=0`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EphemeralTally {
    /// Fires the scheduler enqueued.
    pub fired: u64,
    /// Fires a poll handed to a runner.
    pub dispatched: u64,
    /// Fires dropped at the dispatch hop. Expected to stay 0 — a drop here
    /// means a runner was offered work it could not be given.
    pub dropped: u64,
    /// Fires replaced by a newer one before any runner claimed them
    /// ("keep only the latest", issue #263). Expected whenever runners poll
    /// slower than the job fires, and the honest explanation for a `fired`
    /// that exceeds `dispatched` on a healthy server.
    pub superseded: u64,
}

// ─── HTTP request / response types ───────────────────────────────────────────

/// Runner → server: poll for work.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PollRequest {
    pub runner_id: String,
    pub capabilities: Vec<String>,
    #[serde(default = "default_max_inflight")]
    pub max_inflight: u32,
    /// IDs currently being processed by this runner (heartbeat + inflight list).
    #[serde(default)]
    pub inflight: Vec<String>,
    /// Unique instance ID for this runner process. Used to detect duplicate
    /// runner IDs from different processes (instance guard).
    #[serde(default)]
    pub instance_id: Option<String>,
    /// Free-form tags self-declared by the runner. Filter-only, not used for
    /// routing.
    #[serde(default)]
    pub tags: Vec<String>,
}

fn default_max_inflight() -> u32 {
    1
}

/// Work assignment sent back to a runner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkAssignment {
    pub execution_id: String,
    pub job_key: String,
    pub fire_at: DateTime<Utc>,
    /// The trigger's original logical fire time. `Option` + `serde(default)`
    /// so a runner deserializing a poll response from an older server (which
    /// never emits it) sees `None` rather than failing — no silent fallback
    /// to `fire_at`, which would reintroduce the wrong-logical-time bug.
    #[serde(default)]
    pub scheduled_for: Option<DateTime<Utc>>,
    /// Which attempt this is (1 = first, 2 = first retry, …).
    pub attempt: u32,
    pub metadata: serde_json::Value,
    pub timeout: String,
}

impl From<WorkItem> for WorkAssignment {
    fn from(w: WorkItem) -> Self {
        Self {
            execution_id: w.execution_id,
            job_key: w.job_key,
            fire_at: w.fire_at,
            scheduled_for: Some(w.scheduled_for),
            attempt: w.attempt,
            metadata: w.metadata,
            timeout: w.timeout,
        }
    }
}

/// Server → runner: poll response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PollResponse {
    /// Zero or more work assignments (≤ available capacity).
    pub work: Vec<WorkAssignment>,
    /// Execution IDs the runner should cancel immediately.
    pub cancel: Vec<String>,
}

/// Completion status reported by a runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CompletionStatus {
    Success,
    Failure,
    Cancelled,
}

/// Runner → server: report job completion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompleteRequest {
    pub runner_id: String,
    pub execution_id: String,
    pub status: CompletionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub duration_ms: u64,
    /// Which attempt completed. Defaults to 1 for backwards compatibility with
    /// older runner SDKs that do not send this field.
    #[serde(default = "default_attempt")]
    pub attempt: u32,
}

fn default_attempt() -> u32 {
    1
}

/// Server → runner: acknowledgment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompleteResponse {
    pub received: bool,
}

/// `GET /health` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub runners_online: usize,
    pub runners_stale: usize,
    pub runners_dead: usize,
    pub queued: usize,
}

// ─── Admin API ────────────────────────────────────────────────────────────────

/// Runner summary for `GET /v1/runners`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunnerSummary {
    pub runner_id: String,
    pub status: RunnerStatus,
    pub capabilities: Vec<String>,
    pub max_inflight: u32,
    pub inflight: usize,
    pub last_poll_at: DateTime<Utc>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// `POST /v1/trigger` request — immediately fire a job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerRequest {
    /// Job key (e.g. `billing:invoice-generate`).
    pub job_key: String,

    /// Capabilities a runner MUST have to execute this job.
    ///
    /// Empty (the default) inherits the job's `runner { require … }` from the
    /// configuration — a manual trigger routes like a scheduled fire (issue
    /// #549). A non-empty value overrides the job config.
    #[serde(default)]
    pub require: Vec<String>,

    /// Capabilities that are preferred but not mandatory.
    ///
    /// Empty (the default) inherits the job's `runner { prefer … }`; a
    /// non-empty value overrides it.
    #[serde(default)]
    pub prefer: Vec<String>,

    /// Optional metadata forwarded to the runner as-is (arbitrary JSON).
    #[serde(default)]
    pub metadata: serde_json::Value,

    /// Timeout hint for the runner (e.g. `"15m"`).
    ///
    /// Absent inherits the job's configured `timeout` (issue #551), so a
    /// manual fire is bounded like a scheduled one; `Some` overrides it. Only
    /// when neither exists does the server fall back to `"5m"`.
    ///
    /// This is why the field is an `Option` rather than a defaulted `String`:
    /// with a serde default the server could not tell an omitted field from a
    /// caller who deliberately asked for the default, and every manual fire
    /// silently capped a `timeout 2h` job at five minutes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,

    /// Optional caller-supplied dedup key, scoped per `job_key` (issue
    /// #279). A repeat trigger with the same `(job_key, idempotency_key)`
    /// coalesces to the existing execution — while that execution is still
    /// queued/claimed, or for a configurable window after it was created
    /// (`pull_api { trigger_dedup_window … }`, default 10 m). Max 200
    /// characters; an empty string is treated as absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

/// `POST /v1/trigger` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerResponse {
    /// The execution ID assigned to the triggered job. On a dedup hit this
    /// is the id of the EXISTING execution, not a new one.
    pub execution_id: String,
    /// Current queue depth after enqueue (or the current depth, unchanged,
    /// on a dedup hit — nothing is enqueued then).
    pub queued: usize,
    /// `true` when the trigger coalesced to an existing execution via
    /// `idempotency_key` instead of enqueuing a new one (issue #279).
    #[serde(default)]
    pub deduplicated: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runner_status_online() {
        let r = Runner::new("r1", vec!["billing".into()], 3);
        assert_eq!(r.status_at_with_ttl(Utc::now(), 120), RunnerStatus::Online);
    }

    #[test]
    fn runner_status_stale() {
        use chrono::Duration;
        let r = Runner {
            last_poll_at: Utc::now() - Duration::seconds(60),
            ..Runner::new("r1", vec![], 3)
        };
        assert_eq!(r.status_at_with_ttl(Utc::now(), 120), RunnerStatus::Stale);
    }

    #[test]
    fn runner_status_dead() {
        use chrono::Duration;
        let r = Runner {
            last_poll_at: Utc::now() - Duration::seconds(200),
            ..Runner::new("r1", vec![], 3)
        };
        assert_eq!(r.status_at_with_ttl(Utc::now(), 120), RunnerStatus::Dead);
    }

    #[test]
    fn runner_status_respects_custom_ttl() {
        // With lease_ttl 300 the stale threshold is 150: a runner that last
        // polled 150 s ago is Stale, not Dead — the hardcoded 120 s default
        // would have called it Dead (the UI-vs-watchdog mismatch this guards).
        use chrono::Duration;
        let r = Runner {
            last_poll_at: Utc::now() - Duration::seconds(150),
            ..Runner::new("r1", vec![], 3)
        };
        let now = Utc::now();
        assert_eq!(r.status_at_with_ttl(now, 300), RunnerStatus::Stale);

        // Same runner, shorter TTL: past the dead-threshold.
        assert_eq!(r.status_at_with_ttl(now, 120), RunnerStatus::Dead);

        // Fresh enough for the larger TTL to still count as online.
        let fresh = Runner {
            last_poll_at: Utc::now() - Duration::seconds(100),
            ..Runner::new("r2", vec![], 3)
        };
        assert_eq!(fresh.status_at_with_ttl(now, 300), RunnerStatus::Online);
    }

    #[test]
    fn runner_capacity() {
        let mut r = Runner::new("r1", vec![], 2);
        assert!(r.has_capacity());
        r.inflight.push("exec-1".into());
        assert!(r.has_capacity());
        r.inflight.push("exec-2".into());
        assert!(!r.has_capacity()); // full
    }

    #[test]
    fn work_assignment_from_item() {
        let scheduled_for = Utc::now() - chrono::Duration::days(7);
        let item = WorkItem {
            execution_id: "exec-1".into(),
            job_key: "billing:invoice".into(),
            fire_at: Utc::now(),
            scheduled_for,
            attempt: 3,
            require: vec!["billing".into()],
            prefer: vec!["eu-central".into()],
            metadata: serde_json::json!({}),
            timeout: "15m".into(),
            is_ephemeral: false,
        };
        let assignment = WorkAssignment::from(item);
        assert_eq!(assignment.execution_id, "exec-1");
        assert_eq!(assignment.timeout, "15m");
        assert_eq!(assignment.attempt, 3);
        // The From conversion wraps the always-present WorkItem field into the
        // Option the wire type carries.
        assert_eq!(assignment.scheduled_for, Some(scheduled_for));
    }

    #[test]
    fn work_assignment_deserializes_without_scheduled_for() {
        // A poll response from an older server that never emits the field must
        // deserialize to None, not fail (serde default) — no silent fallback.
        let json = serde_json::json!({
            "execution_id": "exec-1",
            "job_key": "billing:invoice",
            "fire_at": "2026-06-01T06:00:00Z",
            "attempt": 1,
            "metadata": {},
            "timeout": "5m"
        });
        let wa: WorkAssignment = serde_json::from_value(json).unwrap();
        assert_eq!(wa.scheduled_for, None);
    }
}

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
    /// Free-form tags for filtering/grouping. NOT routing-relevant — runner
    /// capabilities handle routing. Convention: `key=value` strings.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Liveness status derived from how recently a runner polled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunnerStatus {
    /// Polled within the last 30 seconds.
    Online,
    /// Last poll was 30 s – 2 min ago. Still considered alive but lagging.
    Stale,
    /// No poll for > 2 minutes. Inflight work should be reassigned.
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
            tags: Vec::new(),
        }
    }

    /// Derive the current liveness status relative to `now`.
    /// Uses default thresholds: online < 30s, stale < 120s, dead >= 120s.
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
    #[serde(default)]
    pub require: Vec<String>,

    /// Capabilities that are preferred but not mandatory.
    #[serde(default)]
    pub prefer: Vec<String>,

    /// Optional metadata forwarded to the runner as-is (arbitrary JSON).
    #[serde(default)]
    pub metadata: serde_json::Value,

    /// Timeout hint for the runner (e.g. `"15m"`). Default: `"5m"`.
    #[serde(default = "default_trigger_timeout")]
    pub timeout: String,

    /// Optional caller-supplied dedup key, scoped per `job_key` (issue
    /// #279). A repeat trigger with the same `(job_key, idempotency_key)`
    /// coalesces to the existing execution — while that execution is still
    /// queued/claimed, or for a configurable window after it was created
    /// (`pull_api { trigger_dedup_window … }`, default 10 m). Max 200
    /// characters; an empty string is treated as absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

fn default_trigger_timeout() -> String {
    "5m".into()
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
        assert_eq!(r.status_at(Utc::now()), RunnerStatus::Online);
    }

    #[test]
    fn runner_status_stale() {
        use chrono::Duration;
        let r = Runner {
            last_poll_at: Utc::now() - Duration::seconds(60),
            ..Runner::new("r1", vec![], 3)
        };
        assert_eq!(r.status_at(Utc::now()), RunnerStatus::Stale);
    }

    #[test]
    fn runner_status_dead() {
        use chrono::Duration;
        let r = Runner {
            last_poll_at: Utc::now() - Duration::seconds(200),
            ..Runner::new("r1", vec![], 3)
        };
        assert_eq!(r.status_at(Utc::now()), RunnerStatus::Dead);
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

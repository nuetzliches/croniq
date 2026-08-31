//! HTTP Pull-API: axum handlers for `POST /v1/poll`, `POST /v1/complete`,
//! and `GET /health`.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use chrono::{DateTime, Duration as ChronoDuration, Utc};

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use tokio::sync::{Notify, RwLock};

use crate::{
    queue::WorkQueue,
    registry::RunnerRegistry,
    types::{
        CompleteRequest, CompleteResponse, EphemeralTally, HealthResponse, PollRequest,
        PollResponse, RunnerStatus, WorkAssignment,
    },
};

// ─── Shared state ─────────────────────────────────────────────────────────────

/// A per-execution lease-liveness record (issue #438).
///
/// Kept in memory like the registry's `inflight` bookkeeping: after a server
/// restart the map refills within one poll/renew interval, well inside the
/// stale-claim reaper's grace window.
#[derive(Debug, Clone)]
pub struct LeaseRenewal {
    /// The runner that refreshed the lease. The stale-claim reaper only
    /// honours the record when it matches the execution's claiming runner,
    /// so a runner cannot keep a foreign claim alive by naming it.
    pub runner_id: String,
    /// When the lease was last refreshed.
    pub renewed_at: DateTime<Utc>,
}

/// State shared across all request handlers.
#[derive(Debug)]
pub struct AppState {
    pub registry: RwLock<RunnerRegistry>,
    pub queue: RwLock<WorkQueue>,
    /// Notified whenever a new WorkItem is enqueued.
    /// Poll handlers wait on this to implement low-latency long-polling.
    pub work_notify: Notify,
    /// Lease TTL in seconds: after this duration without a poll, a runner is
    /// considered dead and its executions are requeued. Default: 120s.
    pub lease_ttl_secs: u64,
    /// Per-runner queue of execution IDs the operator has requested to
    /// cancel (issue #176). Drained on the runner's next poll and delivered
    /// in `PollResponse.cancel`. In-memory only — a server restart loses
    /// pending cancels; the operator can re-issue from the dashboard. The
    /// store-side state transition to `cancelled` is recorded synchronously
    /// when the cancel is issued, so a restart doesn't lose the *intent*.
    pub cancel_queues: RwLock<HashMap<String, Vec<String>>>,
    /// Execution IDs dispatched for **ephemeral** jobs, which intentionally
    /// have no persisted execution row (issue #263). The scheduler records
    /// each id here on dispatch; the completion processor consults it so an
    /// ephemeral completion is acknowledged as a no-op instead of logging
    /// `execution not found for completion`. Maps id → dispatch time so a
    /// stale entry (whose runner died before reporting) can be pruned and
    /// never leaks the map.
    pub ephemeral_inflight: RwLock<HashMap<String, DateTime<Utc>>>,
    /// Per-execution lease liveness (issue #438): execution id → the runner
    /// that holds it and when its lease was last refreshed. Refreshed by
    /// every poll for each execution the runner reports inflight, and by
    /// `POST /v1/work/renew` for the single execution named there. The
    /// watchdog's stale-claim reaper exempts a claim only while its OWN
    /// entry is fresh — renewing one execution does not keep the reaper off
    /// the runner's other claims.
    pub lease_renewals: RwLock<HashMap<String, LeaseRenewal>>,
    /// Per-job ephemeral tallies since the last scheduler heartbeat
    /// (issue #541). Both ends of the fire→dispatch hop count into it — the
    /// scheduler on firing, the poll path on handing work out — because
    /// neither side can see the other, and an ephemeral job that fires
    /// without ever being dispatched looks exactly like a healthy one. The
    /// heartbeat drains the map. `BTreeMap` keeps the rendered order stable
    /// across heartbeats.
    pub ephemeral_stats: RwLock<BTreeMap<String, EphemeralTally>>,
}

impl AppState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            registry: RwLock::new(RunnerRegistry::new()),
            queue: RwLock::new(WorkQueue::new()),
            work_notify: Notify::new(),
            lease_ttl_secs: 120,
            cancel_queues: RwLock::new(HashMap::new()),
            ephemeral_inflight: RwLock::new(HashMap::new()),
            lease_renewals: RwLock::new(HashMap::new()),
            ephemeral_stats: RwLock::new(BTreeMap::new()),
        })
    }

    pub fn with_lease_ttl(lease_ttl_secs: u64) -> Arc<Self> {
        Arc::new(Self {
            registry: RwLock::new(RunnerRegistry::new()),
            queue: RwLock::new(WorkQueue::new()),
            work_notify: Notify::new(),
            lease_ttl_secs,
            cancel_queues: RwLock::new(HashMap::new()),
            ephemeral_inflight: RwLock::new(HashMap::new()),
            lease_renewals: RwLock::new(HashMap::new()),
            ephemeral_stats: RwLock::new(BTreeMap::new()),
        })
    }

    /// Push an execution ID onto the runner's cancel queue. The cancel is
    /// delivered on the runner's next poll; the in-process notify also
    /// wakes any long-poll currently waiting on the work queue so the
    /// runner sees it without an extra retry. Idempotent — pushing the
    /// same id twice keeps a single entry.
    pub async fn push_cancel(&self, runner_id: &str, execution_id: &str) {
        let mut queues = self.cancel_queues.write().await;
        let entry = queues.entry(runner_id.to_string()).or_default();
        if !entry.iter().any(|id| id == execution_id) {
            entry.push(execution_id.to_string());
        }
        drop(queues);
        // Wake any long-poll waiter for this runner — they were sleeping
        // on the work queue and won't notice the cancel otherwise.
        self.work_notify.notify_waiters();
    }

    /// Drain (remove + return) all pending cancels for a runner. Called on
    /// every poll so cancels are delivered exactly once.
    pub async fn drain_cancels(&self, runner_id: &str) -> Vec<String> {
        let mut queues = self.cancel_queues.write().await;
        queues.remove(runner_id).unwrap_or_default()
    }

    /// Record an ephemeral execution as dispatched-but-not-persisted
    /// (issue #263). Opportunistically prunes entries older than `max_age`
    /// first, so a runner that dies mid-execution — and so never reports a
    /// completion that would clear its id — can't leak the map.
    pub async fn record_ephemeral(
        &self,
        execution_id: &str,
        now: DateTime<Utc>,
        max_age: ChronoDuration,
    ) {
        let mut m = self.ephemeral_inflight.write().await;
        m.retain(|_, dispatched_at| now.signed_duration_since(*dispatched_at) < max_age);
        m.insert(execution_id.to_string(), now);
    }

    /// Remove an ephemeral execution id and report whether it was tracked.
    /// The completion processor calls this on a store miss: `true` means the
    /// "missing" execution is an expected ephemeral one (acknowledge, don't
    /// warn); `false` means a genuinely unknown execution.
    pub async fn take_ephemeral(&self, execution_id: &str) -> bool {
        self.ephemeral_inflight
            .write()
            .await
            .remove(execution_id)
            .is_some()
    }

    /// Forget ephemeral ids that will never produce a completion — e.g. a
    /// queued ephemeral item replaced by a newer fire before any runner
    /// claimed it (issue #263 "keep only the latest").
    pub async fn forget_ephemeral(&self, execution_ids: &[String]) {
        if execution_ids.is_empty() {
            return;
        }
        let mut m = self.ephemeral_inflight.write().await;
        for id in execution_ids {
            m.remove(id);
        }
    }

    /// Count `count` ephemeral fires the scheduler enqueued (issue #541).
    pub async fn record_ephemeral_fired(&self, job_key: &str, count: u64) {
        self.bump_ephemeral(job_key, count, |t, n| t.fired += n)
            .await;
    }

    /// Count ephemeral fires a poll handed to a runner.
    pub async fn record_ephemeral_dispatched(&self, job_key: &str, count: u64) {
        self.bump_ephemeral(job_key, count, |t, n| t.dispatched += n)
            .await;
    }

    /// Count ephemeral fires dropped at the dispatch hop — the shape of
    /// failure that made #539 invisible.
    pub async fn record_ephemeral_dropped(&self, job_key: &str, count: u64) {
        self.bump_ephemeral(job_key, count, |t, n| t.dropped += n)
            .await;
    }

    /// Count ephemeral fires replaced by a newer fire before dispatch. Not a
    /// fault: it is why `fired` may exceed `dispatched` on a healthy server.
    pub async fn record_ephemeral_superseded(&self, job_key: &str, count: u64) {
        self.bump_ephemeral(job_key, count, |t, n| t.superseded += n)
            .await;
    }

    async fn bump_ephemeral(
        &self,
        job_key: &str,
        count: u64,
        add: impl FnOnce(&mut EphemeralTally, u64),
    ) {
        if count == 0 {
            return;
        }
        let mut stats = self.ephemeral_stats.write().await;
        add(stats.entry(job_key.to_string()).or_default(), count);
    }

    /// Drain the ephemeral tallies. The heartbeat reports per-interval
    /// numbers, so reading them clears them.
    pub async fn take_ephemeral_stats(&self) -> BTreeMap<String, EphemeralTally> {
        std::mem::take(&mut *self.ephemeral_stats.write().await)
    }

    /// Grace window for per-execution lease liveness, shared with the
    /// stale-claim reaper's threshold (issue #374): a lease refreshed within
    /// this window counts as live.
    pub fn lease_grace_secs(&self) -> u64 {
        (2 * self.lease_ttl_secs).max(120)
    }

    /// Refresh the per-execution lease of each id in `execution_ids` on
    /// behalf of `runner_id` (issue #438). Called on every poll with the
    /// runner's reported inflight list, at dispatch for freshly assigned
    /// work, and by `POST /v1/work/renew` with the one execution it names.
    ///
    /// Opportunistically prunes entries older than the grace window first —
    /// a lease that old exempts nothing any more, so dropping it keeps the
    /// map bounded by recently-live executions.
    pub async fn touch_leases(
        &self,
        runner_id: &str,
        execution_ids: &[String],
        now: DateTime<Utc>,
    ) {
        if execution_ids.is_empty() {
            return;
        }
        let grace = ChronoDuration::seconds(self.lease_grace_secs() as i64);
        let mut leases = self.lease_renewals.write().await;
        leases.retain(|_, lease| now.signed_duration_since(lease.renewed_at) < grace);
        for id in execution_ids {
            leases.insert(
                id.clone(),
                LeaseRenewal {
                    runner_id: runner_id.to_string(),
                    renewed_at: now,
                },
            );
        }
    }

    /// Drop the lease record of a finished or reaped execution. Only removes
    /// the entry when `runner_id` matches the recorded holder, so an ack that
    /// the completion CAS is about to reject (wrong runner) cannot strip a
    /// live execution of its reaper exemption.
    pub async fn clear_lease(&self, runner_id: &str, execution_id: &str) {
        let mut leases = self.lease_renewals.write().await;
        if leases
            .get(execution_id)
            .is_some_and(|lease| lease.runner_id == runner_id)
        {
            leases.remove(execution_id);
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            registry: RwLock::new(RunnerRegistry::new()),
            queue: RwLock::new(WorkQueue::new()),
            work_notify: Notify::new(),
            lease_ttl_secs: 120,
            cancel_queues: RwLock::new(HashMap::new()),
            ephemeral_inflight: RwLock::new(HashMap::new()),
            lease_renewals: RwLock::new(HashMap::new()),
            ephemeral_stats: RwLock::new(BTreeMap::new()),
        }
    }
}

// ─── Router ───────────────────────────────────────────────────────────────────

/// Build the axum router for the Pull-API.
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/v1/poll", post(handle_poll))
        .route("/v1/complete", post(handle_complete))
        .route("/health", get(handle_health))
        .with_state(state)
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

/// `POST /v1/poll`
///
/// The runner announces itself (or refreshes its heartbeat) and requests work.
/// Returns up to `max_inflight - len(inflight)` work assignments.
pub async fn handle_poll(
    State(state): State<Arc<AppState>>,
    Json(req): Json<PollRequest>,
) -> (StatusCode, Json<PollResponse>) {
    // Update registry
    let mut reg = state.registry.write().await;
    let _ = reg.register_or_update(
        &req.runner_id,
        req.capabilities.clone(),
        req.max_inflight,
        req.inflight.clone(),
        req.instance_id.clone(),
        req.tags.clone(),
    );

    let capacity = (req.max_inflight as usize).saturating_sub(req.inflight.len());
    drop(reg); // release write lock before acquiring queue lock

    // Dequeue eligible work
    let work: Vec<WorkAssignment> = if capacity > 0 {
        let mut q = state.queue.write().await;
        let items = q.dequeue_many_for(&req.capabilities, capacity);
        drop(q);

        // Claim items in registry
        let mut reg = state.registry.write().await;
        items
            .into_iter()
            .filter(|item| reg.claim(&req.runner_id, &item.execution_id))
            .map(WorkAssignment::from)
            .collect()
    } else {
        vec![]
    };

    let cancel = state.drain_cancels(&req.runner_id).await;
    let response = PollResponse { work, cancel };

    (StatusCode::OK, Json(response))
}

/// `POST /v1/complete`
///
/// The runner reports that an execution has finished (success or failure).
/// The handler releases the execution from the runner's inflight list.
async fn handle_complete(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CompleteRequest>,
) -> (StatusCode, Json<CompleteResponse>) {
    let mut reg = state.registry.write().await;
    reg.release(&req.runner_id, &req.execution_id);

    (StatusCode::OK, Json(CompleteResponse { received: true }))
}

/// `GET /health`
pub async fn handle_health(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    let now = chrono::Utc::now();
    let reg = state.registry.read().await;
    let queue = state.queue.read().await;

    let response = HealthResponse {
        status: "ok".into(),
        runners_online: reg
            .by_status_with_ttl(RunnerStatus::Online, now, state.lease_ttl_secs)
            .len(),
        runners_stale: reg
            .by_status_with_ttl(RunnerStatus::Stale, now, state.lease_ttl_secs)
            .len(),
        runners_dead: reg
            .by_status_with_ttl(RunnerStatus::Dead, now, state.lease_ttl_secs)
            .len(),
        queued: queue.len(),
    };

    Json(response)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::*;
    use crate::types::WorkItem;

    async fn make_state() -> Arc<AppState> {
        AppState::new()
    }

    async fn post_json(app: Router, uri: &str, body: serde_json::Value) -> serde_json::Value {
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn get_json(app: Router, uri: &str) -> serde_json::Value {
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn poll_registers_runner() {
        let state = make_state().await;
        let app = router(Arc::clone(&state));

        let body = serde_json::json!({
            "runner_id": "r1",
            "capabilities": ["billing"],
            "max_inflight": 3,
            "inflight": []
        });

        let resp = post_json(app, "/v1/poll", body).await;
        assert_eq!(resp["work"].as_array().unwrap().len(), 0);
        assert_eq!(resp["cancel"].as_array().unwrap().len(), 0);

        let reg = state.registry.read().await;
        assert!(reg.get("r1").is_some());
    }

    #[tokio::test]
    async fn poll_returns_matching_work() {
        let state = make_state().await;

        // Enqueue a work item that requires "billing"
        {
            let mut q = state.queue.write().await;
            q.enqueue(WorkItem {
                execution_id: "exec-42".into(),
                job_key: "billing:invoice".into(),
                fire_at: chrono::Utc::now(),
                scheduled_for: chrono::Utc::now(),
                attempt: 1,
                require: vec!["billing".into()],
                prefer: vec![],
                metadata: serde_json::json!({}),
                timeout: "15m".into(),
                is_ephemeral: false,
            });
        }

        let app = router(Arc::clone(&state));

        let resp = post_json(
            app,
            "/v1/poll",
            serde_json::json!({
                "runner_id": "r1",
                "capabilities": ["billing"],
                "max_inflight": 3,
                "inflight": []
            }),
        )
        .await;

        let work = resp["work"].as_array().unwrap();
        assert_eq!(work.len(), 1);
        assert_eq!(work[0]["execution_id"], "exec-42");
        assert_eq!(work[0]["timeout"], "15m");
    }

    #[tokio::test]
    async fn poll_respects_capability_requirements() {
        let state = make_state().await;

        // Enqueue item requiring "billing" — runner only has "etl"
        {
            let mut q = state.queue.write().await;
            q.enqueue(WorkItem {
                execution_id: "exec-billing".into(),
                job_key: "billing:invoice".into(),
                fire_at: chrono::Utc::now(),
                scheduled_for: chrono::Utc::now(),
                attempt: 1,
                require: vec!["billing".into()],
                prefer: vec![],
                metadata: serde_json::json!({}),
                timeout: "5m".into(),
                is_ephemeral: false,
            });
        }

        let app = router(Arc::clone(&state));

        let resp = post_json(
            app,
            "/v1/poll",
            serde_json::json!({
                "runner_id": "etl-worker",
                "capabilities": ["etl"],
                "max_inflight": 3,
                "inflight": []
            }),
        )
        .await;

        // ETL runner can't claim billing work
        assert_eq!(resp["work"].as_array().unwrap().len(), 0);

        // Item remains in queue
        let q = state.queue.read().await;
        assert_eq!(q.len(), 1);
    }

    #[tokio::test]
    async fn poll_respects_max_inflight() {
        let state = make_state().await;

        // Enqueue 3 items
        {
            let mut q = state.queue.write().await;
            for i in 0..3 {
                q.enqueue(WorkItem {
                    execution_id: format!("exec-{i}"),
                    job_key: "job:a".into(),
                    fire_at: chrono::Utc::now(),
                    scheduled_for: chrono::Utc::now(),
                    attempt: 1,
                    require: vec![],
                    prefer: vec![],
                    metadata: serde_json::json!({}),
                    timeout: "5m".into(),
                    is_ephemeral: false,
                });
            }
        }

        let app = router(Arc::clone(&state));

        // Runner has max_inflight=2 and is already running 1 → capacity = 1
        let resp = post_json(
            app,
            "/v1/poll",
            serde_json::json!({
                "runner_id": "r1",
                "capabilities": [],
                "max_inflight": 2,
                "inflight": ["exec-existing"]
            }),
        )
        .await;

        let work = resp["work"].as_array().unwrap();
        assert_eq!(work.len(), 1); // only 1 slot available

        // 2 items remain in queue
        let q = state.queue.read().await;
        assert_eq!(q.len(), 2);
    }

    #[tokio::test]
    async fn complete_releases_inflight() {
        let state = make_state().await;

        // Register runner with an inflight execution
        {
            let mut reg = state.registry.write().await;
            let _ = reg.register_or_update("r1", vec![], 3, vec!["exec-42".into()], None, vec![]);
        }

        let app = router(Arc::clone(&state));

        let resp = post_json(
            app,
            "/v1/complete",
            serde_json::json!({
                "runner_id": "r1",
                "execution_id": "exec-42",
                "status": "success",
                "duration_ms": 1200
            }),
        )
        .await;

        assert_eq!(resp["received"], true);

        // Inflight cleared
        let reg = state.registry.read().await;
        let runner = reg.get("r1").unwrap();
        assert!(runner.inflight.is_empty());
    }

    #[tokio::test]
    async fn health_reports_counts() {
        let state = make_state().await;

        // Register one runner
        {
            let mut reg = state.registry.write().await;
            let _ = reg.register_or_update("r1", vec![], 3, vec![], None, vec![]);
        }

        // Enqueue two items
        {
            let mut q = state.queue.write().await;
            for i in 0..2 {
                q.enqueue(WorkItem {
                    execution_id: format!("exec-{i}"),
                    job_key: "job:a".into(),
                    fire_at: chrono::Utc::now(),
                    scheduled_for: chrono::Utc::now(),
                    attempt: 1,
                    require: vec![],
                    prefer: vec![],
                    metadata: serde_json::json!({}),
                    timeout: "5m".into(),
                    is_ephemeral: false,
                });
            }
        }

        let app = router(Arc::clone(&state));
        let resp = get_json(app, "/health").await;

        assert_eq!(resp["status"], "ok");
        assert_eq!(resp["runners_online"], 1);
        assert_eq!(resp["queued"], 2);
    }

    #[tokio::test]
    async fn push_and_drain_cancels_round_trip() {
        // AppState's per-runner cancel queue is push-once, drain-once.
        let state = make_state().await;
        state.push_cancel("r1", "exec-1").await;
        state.push_cancel("r1", "exec-2").await;
        // Idempotent: pushing the same id twice keeps a single entry.
        state.push_cancel("r1", "exec-1").await;

        let drained = state.drain_cancels("r1").await;
        assert_eq!(drained, vec!["exec-1", "exec-2"]);
        // After drain the queue is empty.
        let drained_again = state.drain_cancels("r1").await;
        assert!(drained_again.is_empty());
    }

    #[tokio::test]
    async fn poll_delivers_pending_cancels() {
        // A runner polling while at capacity (or with no work available)
        // still gets any cancels pushed by the admin endpoint.
        let state = make_state().await;
        state.push_cancel("r1", "exec-cancel-me").await;

        let app = router(Arc::clone(&state));
        let resp = post_json(
            app,
            "/v1/poll",
            serde_json::json!({
                "runner_id": "r1",
                "capabilities": [],
                "max_inflight": 1,
                "inflight": []
            }),
        )
        .await;

        assert_eq!(resp["work"].as_array().unwrap().len(), 0);
        assert_eq!(resp["cancel"].as_array().unwrap().len(), 1);
        assert_eq!(resp["cancel"][0], "exec-cancel-me");

        // Cancel was delivered exactly once — second poll yields nothing.
        let app = router(Arc::clone(&state));
        let resp2 = post_json(
            app,
            "/v1/poll",
            serde_json::json!({
                "runner_id": "r1",
                "capabilities": [],
                "max_inflight": 1,
                "inflight": []
            }),
        )
        .await;
        assert!(resp2["cancel"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn touch_leases_records_holder_and_prunes_expired_entries() {
        // Default lease_ttl 120 s ⇒ grace window 240 s.
        let state = make_state().await;
        let now = chrono::Utc::now();

        let stale_at = now - chrono::Duration::seconds(500);
        state
            .touch_leases("r1", &["exec-old".into()], stale_at)
            .await;
        state.touch_leases("r2", &["exec-new".into()], now).await;

        let leases = state.lease_renewals.read().await;
        assert!(
            !leases.contains_key("exec-old"),
            "entries older than the grace window are pruned on the next touch"
        );
        let lease = leases.get("exec-new").expect("fresh lease recorded");
        assert_eq!(lease.runner_id, "r2");
        assert_eq!(lease.renewed_at, now);
    }

    #[tokio::test]
    async fn clear_lease_only_removes_the_holders_own_entry() {
        let state = make_state().await;
        let now = chrono::Utc::now();
        state.touch_leases("r1", &["exec-1".into()], now).await;

        // A different runner (e.g. an ack the completion CAS will reject)
        // must not strip the live execution of its lease.
        state.clear_lease("r2", "exec-1").await;
        assert!(state.lease_renewals.read().await.contains_key("exec-1"));

        state.clear_lease("r1", "exec-1").await;
        assert!(!state.lease_renewals.read().await.contains_key("exec-1"));
    }

    #[tokio::test]
    async fn complete_with_failure_status() {
        let state = make_state().await;

        {
            let mut reg = state.registry.write().await;
            let _ = reg.register_or_update("r1", vec![], 3, vec!["exec-99".into()], None, vec![]);
        }

        let app = router(Arc::clone(&state));

        let resp = post_json(
            app,
            "/v1/complete",
            serde_json::json!({
                "runner_id": "r1",
                "execution_id": "exec-99",
                "status": "failure",
                "error": "Connection refused: db:5432",
                "duration_ms": 3200
            }),
        )
        .await;

        assert_eq!(resp["received"], true);
    }
}

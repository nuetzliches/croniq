//! HTTP Pull-API: axum handlers for `POST /v1/poll`, `POST /v1/complete`,
//! and `GET /health`.

use std::sync::Arc;

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
        CompleteRequest, CompleteResponse, HealthResponse, PollRequest, PollResponse, RunnerStatus,
        WorkAssignment,
    },
};

// ─── Shared state ─────────────────────────────────────────────────────────────

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
}

impl AppState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            registry: RwLock::new(RunnerRegistry::new()),
            queue: RwLock::new(WorkQueue::new()),
            work_notify: Notify::new(),
            lease_ttl_secs: 120,
        })
    }

    pub fn with_lease_ttl(lease_ttl_secs: u64) -> Arc<Self> {
        Arc::new(Self {
            registry: RwLock::new(RunnerRegistry::new()),
            queue: RwLock::new(WorkQueue::new()),
            work_notify: Notify::new(),
            lease_ttl_secs,
        })
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            registry: RwLock::new(RunnerRegistry::new()),
            queue: RwLock::new(WorkQueue::new()),
            work_notify: Notify::new(),
            lease_ttl_secs: 120,
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

    let response = PollResponse {
        work,
        cancel: vec![],
    };

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
        runners_online: reg.by_status(RunnerStatus::Online, now).len(),
        runners_stale: reg.by_status(RunnerStatus::Stale, now).len(),
        runners_dead: reg.by_status(RunnerStatus::Dead, now).len(),
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
                attempt: 1,
                require: vec!["billing".into()],
                prefer: vec![],
                metadata: serde_json::json!({}),
                timeout: "15m".into(),
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
                attempt: 1,
                require: vec!["billing".into()],
                prefer: vec![],
                metadata: serde_json::json!({}),
                timeout: "5m".into(),
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
                    attempt: 1,
                    require: vec![],
                    prefer: vec![],
                    metadata: serde_json::json!({}),
                    timeout: "5m".into(),
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
            let _ = reg.register_or_update("r1", vec![], 3, vec!["exec-42".into()], None);
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
            let _ = reg.register_or_update("r1", vec![], 3, vec![], None);
        }

        // Enqueue two items
        {
            let mut q = state.queue.write().await;
            for i in 0..2 {
                q.enqueue(WorkItem {
                    execution_id: format!("exec-{i}"),
                    job_key: "job:a".into(),
                    fire_at: chrono::Utc::now(),
                    attempt: 1,
                    require: vec![],
                    prefer: vec![],
                    metadata: serde_json::json!({}),
                    timeout: "5m".into(),
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
    async fn complete_with_failure_status() {
        let state = make_state().await;

        {
            let mut reg = state.registry.write().await;
            let _ = reg.register_or_update("r1", vec![], 3, vec!["exec-99".into()], None);
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

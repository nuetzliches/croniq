//! Extended HTTP API: runner Pull-API plus the server-side completion handler.
//!
//! Routes:
//! - `POST /v1/poll`     → dispatches work from the queue to polling runners
//! - `POST /v1/complete` → releases inflight + forwards to completion processor
//! - `GET  /health`      → liveness + queue depth

use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use chrono::Utc;
use croniq_runner::{
    AppState, CompleteResponse, RunnerStatus, RunnerSummary, TriggerRequest, TriggerResponse,
    WorkAssignment, WorkItem,
    types::{CompleteRequest, HealthResponse, PollRequest, PollResponse},
};
use tokio::sync::mpsc;

use crate::completion::CompletionEvent;

/// Default maximum time a poll request will block waiting for work.
const DEFAULT_LONG_POLL_TIMEOUT: Duration = Duration::from_secs(30);

// ─── Server state ─────────────────────────────────────────────────────────────

/// Full server state: runner sub-state + completion channel.
#[derive(Debug)]
pub struct ServerState {
    /// Shared runner state (registry + queue).
    pub runner: Arc<AppState>,
    /// Channel for forwarding completion events to the processor task.
    pub completion_tx: mpsc::UnboundedSender<CompletionEvent>,
    /// How long a poll request may block waiting for work.
    /// Defaults to 30 s; can be reduced in tests.
    pub long_poll_timeout: Duration,
}

impl ServerState {
    pub fn new(
        runner: Arc<AppState>,
        completion_tx: mpsc::UnboundedSender<CompletionEvent>,
    ) -> Arc<Self> {
        Arc::new(Self {
            runner,
            completion_tx,
            long_poll_timeout: DEFAULT_LONG_POLL_TIMEOUT,
        })
    }

    /// Construct with a custom long-poll timeout (useful in tests).
    pub fn with_timeout(
        runner: Arc<AppState>,
        completion_tx: mpsc::UnboundedSender<CompletionEvent>,
        long_poll_timeout: Duration,
    ) -> Arc<Self> {
        Arc::new(Self { runner, completion_tx, long_poll_timeout })
    }
}

// ─── Router ───────────────────────────────────────────────────────────────────

pub fn server_router(state: Arc<ServerState>) -> Router {
    Router::new()
        .route("/v1/poll", post(handle_poll))
        .route("/v1/complete", post(handle_complete))
        .route("/v1/runners", get(handle_list_runners))
        .route("/v1/trigger", post(handle_trigger))
        .route("/health", get(handle_health))
        .with_state(state)
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

/// `POST /v1/poll` — heartbeat + work dispatch with long-poll support.
///
/// If the queue is empty and the runner has capacity, the handler waits up to
/// `LONG_POLL_TIMEOUT` for a `work_notify` signal before returning an empty
/// response. This eliminates the need for runners to busy-poll.
async fn handle_poll(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<PollRequest>,
) -> (StatusCode, Json<PollResponse>) {
    // Update registry heartbeat
    {
        let mut reg = state.runner.registry.write().await;
        reg.register_or_update(
            &req.runner_id,
            req.capabilities.clone(),
            req.max_inflight,
            req.inflight.clone(),
        );
    }

    let capacity = (req.max_inflight as usize).saturating_sub(req.inflight.len());

    if capacity == 0 {
        // Runner is at capacity — no point waiting
        return (StatusCode::OK, Json(PollResponse { work: vec![], cancel: vec![] }));
    }

    // Try to dequeue immediately; if nothing available, long-poll for up to
    // LONG_POLL_TIMEOUT waiting for a work_notify signal.
    loop {
        // Set up the notification listener BEFORE checking the queue so we
        // cannot miss an enqueue that races with our check.
        let notified = state.runner.work_notify.notified();

        let work = try_dequeue_for(&state.runner, &req.runner_id, &req.capabilities, capacity).await;

        if !work.is_empty() {
            return (StatusCode::OK, Json(PollResponse { work, cancel: vec![] }));
        }

        // Queue empty — wait for a notification or timeout
        tokio::select! {
            _ = notified => {
                // A new item was enqueued — loop and try again
            }
            _ = tokio::time::sleep(state.long_poll_timeout) => {
                // Timeout: return empty response, runner will poll again
                return (StatusCode::OK, Json(PollResponse { work: vec![], cancel: vec![] }));
            }
        }
    }
}

/// Attempt to dequeue items for a runner without blocking.
async fn try_dequeue_for(
    runner: &Arc<AppState>,
    runner_id: &str,
    capabilities: &[String],
    capacity: usize,
) -> Vec<WorkAssignment> {
    let mut q = runner.queue.write().await;
    let items = q.dequeue_many_for(capabilities, capacity);
    drop(q);

    if items.is_empty() {
        return vec![];
    }

    let mut reg = runner.registry.write().await;
    items
        .into_iter()
        .filter(|item| reg.claim(runner_id, &item.execution_id))
        .map(WorkAssignment::from)
        .collect()
}

/// `POST /v1/complete` — release inflight + forward to processor.
async fn handle_complete(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<CompleteRequest>,
) -> (StatusCode, Json<CompleteResponse>) {
    {
        let mut reg = state.runner.registry.write().await;
        reg.release(&req.runner_id, &req.execution_id);
    }

    let event = CompletionEvent {
        runner_id: req.runner_id.clone(),
        execution_id: req.execution_id.clone(),
        status: req.status,
        error: req.error.clone(),
        duration_ms: req.duration_ms,
        attempt: req.attempt,
    };

    if let Err(e) = state.completion_tx.send(event) {
        tracing::error!(error = %e, "completion channel closed");
    }

    (StatusCode::OK, Json(CompleteResponse { received: true }))
}

/// `GET /v1/runners` — list all known runners with liveness status.
async fn handle_list_runners(
    State(state): State<Arc<ServerState>>,
) -> Json<Vec<RunnerSummary>> {
    let now = Utc::now();
    let reg = state.runner.registry.read().await;

    let summaries = reg
        .all()
        .map(|r| RunnerSummary {
            runner_id: r.runner_id.clone(),
            status: r.status_at(now),
            capabilities: r.capabilities.clone(),
            max_inflight: r.max_inflight,
            inflight: r.inflight.len(),
            last_poll_at: r.last_poll_at,
        })
        .collect();

    Json(summaries)
}

/// `POST /v1/trigger` — immediately enqueue a job execution.
async fn handle_trigger(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<TriggerRequest>,
) -> (StatusCode, Json<TriggerResponse>) {
    let execution_id = uuid::Uuid::new_v4().to_string();

    let item = WorkItem {
        execution_id: execution_id.clone(),
        job_key: req.job_key,
        fire_at: Utc::now(),
        attempt: 1,
        require: req.require,
        prefer: req.prefer,
        metadata: req.metadata,
        timeout: req.timeout,
    };

    let queued = {
        let mut q = state.runner.queue.write().await;
        q.enqueue(item);
        q.len()
    };
    state.runner.work_notify.notify_waiters();

    (
        StatusCode::OK,
        Json(TriggerResponse { execution_id, queued }),
    )
}

/// `GET /health`
async fn handle_health(State(state): State<Arc<ServerState>>) -> Json<HealthResponse> {
    let now = Utc::now();
    let reg = state.runner.registry.read().await;
    let queue = state.runner.queue.read().await;

    Json(HealthResponse {
        status: "ok".into(),
        runners_online: reg.by_status(RunnerStatus::Online, now).len(),
        runners_stale: reg.by_status(RunnerStatus::Stale, now).len(),
        runners_dead: reg.by_status(RunnerStatus::Dead, now).len(),
        queued: queue.len(),
    })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::util::ServiceExt;

    use super::*;
    use croniq_runner::WorkItem;

    fn make_state() -> (Arc<ServerState>, mpsc::UnboundedReceiver<CompletionEvent>) {
        let runner = AppState::new();
        let (tx, rx) = mpsc::unbounded_channel();
        // Use a very short long-poll timeout in tests so they complete quickly
        let state = ServerState::with_timeout(runner, tx, Duration::from_millis(50));
        (state, rx)
    }

    async fn post_json(app: Router, uri: &str, body: serde_json::Value) -> serde_json::Value {
        let resp = app
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
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn get_json(app: Router, uri: &str) -> serde_json::Value {
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn poll_registers_runner() {
        let (state, _rx) = make_state();
        let app = server_router(Arc::clone(&state));

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

        assert_eq!(resp["work"].as_array().unwrap().len(), 0);

        let reg = state.runner.registry.read().await;
        assert!(reg.get("r1").is_some());
    }

    #[tokio::test]
    async fn poll_dispatches_queued_work() {
        let (state, _rx) = make_state();

        {
            let mut q = state.runner.queue.write().await;
            q.enqueue(WorkItem {
                execution_id: "exec-1".into(),
                job_key: "billing:invoice".into(),
                fire_at: chrono::Utc::now(),
                attempt: 1,
                require: vec![],
                prefer: vec![],
                metadata: serde_json::json!({}),
                timeout: "15m".into(),
            });
        }

        let app = server_router(Arc::clone(&state));

        let resp = post_json(
            app,
            "/v1/poll",
            serde_json::json!({
                "runner_id": "r1",
                "capabilities": [],
                "max_inflight": 3,
                "inflight": []
            }),
        )
        .await;

        let work = resp["work"].as_array().unwrap();
        assert_eq!(work.len(), 1);
        assert_eq!(work[0]["execution_id"], "exec-1");
    }

    #[tokio::test]
    async fn complete_releases_inflight() {
        let (state, _rx) = make_state();

        {
            let mut reg = state.runner.registry.write().await;
            reg.register_or_update("r1", vec![], 3, vec!["exec-42".into()]);
        }

        let app = server_router(Arc::clone(&state));

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

        let reg = state.runner.registry.read().await;
        assert!(reg.get("r1").unwrap().inflight.is_empty());
    }

    #[tokio::test]
    async fn complete_forwards_to_channel() {
        let (state, mut rx) = make_state();

        {
            let mut reg = state.runner.registry.write().await;
            reg.register_or_update("r1", vec![], 3, vec!["exec-99".into()]);
        }

        let app = server_router(Arc::clone(&state));

        post_json(
            app,
            "/v1/complete",
            serde_json::json!({
                "runner_id": "r1",
                "execution_id": "exec-99",
                "status": "failure",
                "error": "Connection refused",
                "duration_ms": 250
            }),
        )
        .await;

        let event = rx.try_recv().unwrap();
        assert_eq!(event.execution_id, "exec-99");
        assert_eq!(event.error.as_deref(), Some("Connection refused"));
    }

    #[tokio::test]
    async fn health_returns_ok() {
        let (state, _rx) = make_state();
        let app = server_router(Arc::clone(&state));

        let resp = get_json(app, "/health").await;
        assert_eq!(resp["status"], "ok");
        assert_eq!(resp["queued"], 0);
    }
}

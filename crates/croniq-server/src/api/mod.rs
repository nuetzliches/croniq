//! Extended HTTP API: runner Pull-API, auth, and management endpoints.

pub mod auth_endpoints;
pub mod auth_middleware;
pub mod calendars;
pub mod dead_letters;
pub mod execution_logs;
pub mod jobs;
pub mod schedules;
pub mod work;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    middleware,
    routing::{get, post, delete},
};
use chrono::Utc;
use croniq_auth::jwt::JwtConfig;
use croniq_runner::{
    AppState, CompleteResponse, RunnerStatus, RunnerSummary, TriggerRequest, TriggerResponse,
    WorkAssignment, WorkItem,
    types::{CompleteRequest, HealthResponse, PollRequest, PollResponse},
};
use tokio::sync::mpsc;

use crate::completion::CompletionEvent;
use crate::store::DynStore;
use croniq_store::models::ExecutionFilter;
use croniq_store::traits::{ExecutionStore, JobStore};

/// Default maximum time a poll request will block waiting for work.
const DEFAULT_LONG_POLL_TIMEOUT: Duration = Duration::from_secs(30);

// ─── Server state ─────────────────────────────────────────────────────────────

/// Full server state: runner sub-state + completion channel.
pub struct ServerState {
    /// Shared runner state (registry + queue).
    pub runner: Arc<AppState>,
    /// Channel for forwarding completion events to the processor task.
    pub completion_tx: mpsc::UnboundedSender<CompletionEvent>,
    /// How long a poll request may block waiting for work.
    /// Defaults to 30 s; can be reduced in tests.
    pub long_poll_timeout: Duration,
    /// JWT configuration for token-based auth. None = auth disabled.
    pub jwt_config: Option<JwtConfig>,
    /// Persistent store for querying jobs and executions.
    pub store: Option<DynStore>,
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
            jwt_config: None,
            store: None,
        })
    }

    /// Construct with JWT auth and optional store.
    pub fn with_auth(
        runner: Arc<AppState>,
        completion_tx: mpsc::UnboundedSender<CompletionEvent>,
        jwt_config: Option<JwtConfig>,
        store: Option<DynStore>,
    ) -> Arc<Self> {
        Arc::new(Self {
            runner,
            completion_tx,
            long_poll_timeout: DEFAULT_LONG_POLL_TIMEOUT,
            jwt_config,
            store,
        })
    }

    /// Construct with a custom long-poll timeout (useful in tests).
    pub fn with_timeout(
        runner: Arc<AppState>,
        completion_tx: mpsc::UnboundedSender<CompletionEvent>,
        long_poll_timeout: Duration,
    ) -> Arc<Self> {
        Arc::new(Self { runner, completion_tx, long_poll_timeout, jwt_config: None, store: None })
    }
}

// ─── Router ───────────────────────────────────────────────────────────────────

pub fn server_router(state: Arc<ServerState>) -> Router {
    // Authenticated routes
    let authenticated = Router::new()
        // Work protocol
        .route("/v1/poll", post(handle_poll))             // legacy compat
        .route("/v1/complete", post(handle_complete))       // legacy compat
        .route("/v1/work/poll", post(handle_poll))
        .route("/v1/work/ack", post(handle_complete))
        .route("/v1/work/renew", post(work::handle_renew))
        .route("/v1/work/{execution_id}/events", post(work::handle_events))
        // Runners
        .route("/v1/runners", get(handle_list_runners))
        .route("/v1/runners/{id}", delete(handle_delete_runner))
        .route("/v1/trigger", post(handle_trigger))
        // Jobs CRUD
        .route("/v1/jobs", get(jobs::handle_list).post(jobs::handle_create))
        .route("/v1/jobs/{job_key}", get(jobs::handle_get).delete(jobs::handle_delete))
        .route("/v1/jobs/{job_key}/activate", post(jobs::handle_activate))
        // Schedules CRUD
        .route("/v1/schedules", get(schedules::handle_list).post(schedules::handle_create))
        .route("/v1/schedules/{trigger_id}", get(schedules::handle_get).delete(schedules::handle_delete))
        // Calendars CRUD
        .route("/v1/calendars", get(calendars::handle_list).post(calendars::handle_create))
        .route("/v1/calendars/{id}", get(calendars::handle_get).delete(calendars::handle_delete))
        // Dead letters
        .route("/v1/dead-letters", get(dead_letters::handle_list))
        .route("/v1/dead-letters/{id}", get(dead_letters::handle_get).delete(dead_letters::handle_delete))
        // Executions + logs
        .route("/v1/executions", get(handle_list_executions))
        .route("/v1/executions/{id}/logs", get(execution_logs::handle_get_logs))
        // Auth management
        .route("/v1/api-clients", get(auth_endpoints::handle_list_clients).post(auth_endpoints::handle_create_client))
        .route("/v1/api-clients/{id}", delete(auth_endpoints::handle_delete_client))
        .route("/v1/api-clients/{id}/tokens", post(auth_endpoints::handle_issue_client_token))
        .route("/v1/api-keys", post(auth_endpoints::handle_create_api_key))
        .route("/v1/api-keys/{id}", delete(auth_endpoints::handle_revoke_api_key))
        .route_layer(middleware::from_fn_with_state(
            Arc::clone(&state),
            auth_middleware::require_auth,
        ));

    // Public routes (health + auth login/refresh/logout)
    let public = Router::new()
        .route("/health", get(handle_health))
        .route("/v1/auth/login", post(auth_endpoints::handle_login))
        .route("/v1/auth/refresh", post(auth_endpoints::handle_refresh))
        .route("/v1/auth/logout", post(auth_endpoints::handle_logout));

    authenticated.merge(public).with_state(state)
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

/// `DELETE /v1/runners/{id}` — deregister a runner.
async fn handle_delete_runner(
    State(state): State<Arc<ServerState>>,
    axum::extract::Path(runner_id): axum::extract::Path<String>,
) -> StatusCode {
    let mut reg = state.runner.registry.write().await;
    reg.remove(&runner_id);
    StatusCode::NO_CONTENT
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

/// `GET /v1/executions` — list recent executions from the store.
async fn handle_list_executions(
    State(state): State<Arc<ServerState>>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let store = state.store.as_ref().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let filter = ExecutionFilter {
        job_key: params.get("job_key").cloned(),
        state: params.get("state").and_then(|s| match s.as_str() {
            "queued" => Some(croniq_store::models::ExecutionState::Queued),
            "claimed" => Some(croniq_store::models::ExecutionState::Claimed),
            "completed" => Some(croniq_store::models::ExecutionState::Completed),
            "failed" => Some(croniq_store::models::ExecutionState::Failed),
            "dead" => Some(croniq_store::models::ExecutionState::Dead),
            "cancelled" => Some(croniq_store::models::ExecutionState::Cancelled),
            _ => None,
        }),
        limit: params.get("limit").and_then(|l| l.parse().ok()),
        ..Default::default()
    };
    let executions = store.list_executions(&filter).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::to_value(&executions).unwrap_or_default()))
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

    // ─── Auth middleware tests ────────────────────────────────────────────────

    fn make_auth_state(secret: &str) -> (Arc<ServerState>, mpsc::UnboundedReceiver<CompletionEvent>) {
        let runner = AppState::new();
        let (tx, rx) = mpsc::unbounded_channel();
        let jwt_config = JwtConfig {
            secret: secret.to_string(),
            ..Default::default()
        };
        let state = Arc::new(ServerState {
            runner,
            completion_tx: tx,
            long_poll_timeout: Duration::from_millis(50),
            jwt_config: Some(jwt_config),
            store: None,
        });
        (state, rx)
    }

    async fn status_of(app: Router, method: &str, uri: &str, body: Option<serde_json::Value>, bearer: Option<&str>) -> u16 {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(token) = bearer {
            builder = builder.header("authorization", format!("Bearer {token}"));
        }
        if body.is_some() {
            builder = builder.header("content-type", "application/json");
        }
        let body = body.map(|b| Body::from(b.to_string())).unwrap_or(Body::empty());
        let resp = app.oneshot(builder.body(body).unwrap()).await.unwrap();
        resp.status().as_u16()
    }

    #[tokio::test]
    async fn auth_rejects_without_token() {
        let (state, _rx) = make_auth_state("test-secret");
        let app = server_router(Arc::clone(&state));

        let status = status_of(app, "POST", "/v1/poll", Some(serde_json::json!({
            "runner_id": "r1", "capabilities": [], "max_inflight": 1, "inflight": []
        })), None).await;

        assert_eq!(status, 401);
    }

    #[tokio::test]
    async fn auth_rejects_invalid_jwt() {
        let (state, _rx) = make_auth_state("test-secret");
        let app = server_router(Arc::clone(&state));

        let status = status_of(app, "POST", "/v1/poll", Some(serde_json::json!({
            "runner_id": "r1", "capabilities": [], "max_inflight": 1, "inflight": []
        })), Some("invalid.jwt.token")).await;

        assert_eq!(status, 401);
    }

    #[tokio::test]
    async fn auth_accepts_valid_jwt() {
        let (state, _rx) = make_auth_state("test-secret");
        let jwt_config = state.jwt_config.as_ref().unwrap();
        let pair = croniq_auth::jwt::issue_token_pair(
            jwt_config, "test-user", "test-client",
            croniq_auth::CallerType::User, &["admin".into()],
        ).unwrap();

        let app = server_router(Arc::clone(&state));
        let status = status_of(app, "POST", "/v1/poll", Some(serde_json::json!({
            "runner_id": "r1", "capabilities": [], "max_inflight": 1, "inflight": []
        })), Some(&pair.access_token)).await;

        assert_eq!(status, 200);
    }

    #[tokio::test]
    async fn auth_health_is_public() {
        let (state, _rx) = make_auth_state("test-secret");
        let app = server_router(Arc::clone(&state));

        let status = status_of(app, "GET", "/health", None, None).await;
        assert_eq!(status, 200);
    }

    #[tokio::test]
    async fn auth_login_is_public() {
        let (state, _rx) = make_auth_state("test-secret");
        let app = server_router(Arc::clone(&state));

        // Login endpoint should be reachable (will fail with 503 since no store)
        let status = status_of(app, "POST", "/v1/auth/login", Some(serde_json::json!({
            "username": "admin", "password": "pass"
        })), None).await;

        // 503 because no store configured, but NOT 401/404
        assert_eq!(status, 503);
    }
}

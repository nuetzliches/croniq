//! Extended HTTP API: runner Pull-API, auth, and management endpoints.

pub mod admin;
pub mod auth_endpoints;
pub mod auth_middleware;
pub mod calendars;
pub mod dashboard;
pub mod dead_letters;
pub mod execution_logs;
pub mod jobs;
pub mod runners_sse;
pub mod schedules;
pub mod tags;
pub mod work;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::api::auth_middleware::require_scope;
use axum::{
    Extension, Json, Router,
    extract::State,
    http::StatusCode,
    middleware,
    routing::{delete, get, post, put},
};
use chrono::{DateTime, Utc};
use croniq_auth::CallerContext;
use croniq_auth::context::Scope;
use croniq_auth::jwt::JwtConfig;
use croniq_runner::{
    AppState, CompleteResponse, RunnerStatus, RunnerSummary, TriggerRequest, TriggerResponse,
    WorkAssignment, WorkItem,
    types::{CompleteRequest, HealthResponse, PollRequest, PollResponse},
};
use croniq_scheduler::trigger::Trigger;
use serde::Serialize;
use tokio::sync::mpsc;

use crate::completion::CompletionEvent;
use crate::reload::ReloadCounters;
use crate::scheduler::SchedulerCommand;
use crate::store::DynStore;
use croniq_config::compile::{CalendarConfig, JobConfig};
use croniq_store::models::{Execution, ExecutionFilter, ExecutionState};

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
    /// Channel to send commands to the live scheduler (add/remove jobs).
    pub scheduler_tx: Option<mpsc::UnboundedSender<SchedulerCommand>>,
    /// Shared trigger map for dashboard forecast (read-only snapshot).
    pub triggers: Option<Arc<tokio::sync::RwLock<HashMap<String, Trigger>>>>,
    /// DSL-defined jobs (from the Croniqfile). Shared with the scheduler task,
    /// which replaces its contents on Croniqfile hot-reload. The REST API
    /// unions this with the persisted store so DSL jobs appear in `/v1/jobs`
    /// and `/v1/schedules` alongside API/runner-registered ones.
    pub dsl_jobs: Option<Arc<tokio::sync::RwLock<Vec<JobConfig>>>>,
    /// DSL-defined calendars (from the Croniqfile). Same hot-reload semantics
    /// as `dsl_jobs`. The REST API synthesizes them with `managed_by="dsl"`
    /// in `/v1/calendars` so the UI can reference them in schedule editors.
    pub dsl_calendars: Option<Arc<tokio::sync::RwLock<Vec<CalendarConfig>>>>,
    /// Server-wide policy flag from the Croniqfile `policy { dsl_adopt_on_mutate ... }`
    /// block. When `true`, the explicit `/adopt` endpoint copies a DSL
    /// resource into the API store; when `false` (default), `/adopt`
    /// returns 409 and PUT/DELETE on DSL resources stay blocked.
    pub policy_dsl_adopt_on_mutate: Arc<std::sync::atomic::AtomicBool>,
    /// Path to the Croniqfile, needed by the admin reload endpoint.
    pub config_path: Option<std::path::PathBuf>,
    /// Counters for `croniq_config_reload_total`, incremented by both the
    /// file-watcher reload path and the admin reload endpoint.
    pub reload_counters: Arc<ReloadCounters>,
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
            scheduler_tx: None,
            triggers: None,
            dsl_jobs: None,
            dsl_calendars: None,
            policy_dsl_adopt_on_mutate: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            config_path: None,
            reload_counters: ReloadCounters::new(),
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
            scheduler_tx: None,
            triggers: None,
            dsl_jobs: None,
            dsl_calendars: None,
            policy_dsl_adopt_on_mutate: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            config_path: None,
            reload_counters: ReloadCounters::new(),
        })
    }

    /// Construct with a custom long-poll timeout (useful in tests).
    pub fn with_timeout(
        runner: Arc<AppState>,
        completion_tx: mpsc::UnboundedSender<CompletionEvent>,
        long_poll_timeout: Duration,
    ) -> Arc<Self> {
        Arc::new(Self {
            runner,
            completion_tx,
            long_poll_timeout,
            jwt_config: None,
            store: None,
            scheduler_tx: None,
            triggers: None,
            dsl_jobs: None,
            dsl_calendars: None,
            policy_dsl_adopt_on_mutate: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            config_path: None,
            reload_counters: ReloadCounters::new(),
        })
    }
}

// ─── Router ───────────────────────────────────────────────────────────────────

pub fn server_router(state: Arc<ServerState>) -> Router {
    // Authenticated routes
    let authenticated = Router::new()
        // Work protocol
        .route("/v1/poll", post(handle_poll)) // legacy compat
        .route("/v1/complete", post(handle_complete)) // legacy compat
        .route("/v1/work/poll", post(handle_poll))
        .route("/v1/work/ack", post(handle_complete))
        .route("/v1/work/renew", post(work::handle_renew))
        .route("/v1/work/{execution_id}/events", post(work::handle_events))
        // Runners
        .route("/v1/runners", get(handle_list_runners))
        .route("/v1/runners/{id}", delete(handle_delete_runner))
        .route("/v1/runners/stream", get(runners_sse::handle_runner_stream))
        .route("/v1/trigger", post(handle_trigger))
        // Jobs CRUD
        .route("/v1/jobs", get(jobs::handle_list).post(jobs::handle_create))
        .route(
            "/v1/jobs/{job_key}",
            get(jobs::handle_get)
                .put(jobs::handle_update)
                .delete(jobs::handle_delete),
        )
        .route("/v1/jobs/{job_key}/activate", post(jobs::handle_activate))
        .route(
            "/v1/jobs/{job_key}/deactivate",
            post(jobs::handle_deactivate),
        )
        .route("/v1/jobs/register", post(jobs::handle_register))
        // Schedules CRUD
        .route(
            "/v1/schedules",
            get(schedules::handle_list).post(schedules::handle_create),
        )
        .route(
            "/v1/schedules/{trigger_id}",
            get(schedules::handle_get)
                .put(schedules::handle_update)
                .delete(schedules::handle_delete),
        )
        // Calendars CRUD
        .route(
            "/v1/calendars",
            get(calendars::handle_list).post(calendars::handle_create),
        )
        .route(
            "/v1/calendars/{id}",
            get(calendars::handle_get)
                .put(calendars::handle_update)
                .delete(calendars::handle_delete),
        )
        // Calendar adoption (Phase 2 — opt-in via Croniqfile policy block).
        .route("/v1/calendars/{id}/adopt", post(calendars::handle_adopt))
        .route(
            "/v1/calendars/{id}/unadopt",
            post(calendars::handle_unadopt),
        )
        // Job adoption (Phase 2.5 — same opt-in policy applies).
        .route("/v1/jobs/{job_key}/adopt", post(jobs::handle_adopt))
        .route("/v1/jobs/{job_key}/unadopt", post(jobs::handle_unadopt))
        // Dead letters
        .route("/v1/dead-letters", get(dead_letters::handle_list))
        .route(
            "/v1/dead-letters/{id}",
            get(dead_letters::handle_get).delete(dead_letters::handle_delete),
        )
        .route(
            "/v1/dead-letters/{id}/replay",
            post(dead_letters::handle_replay),
        )
        // Dashboard
        .route("/v1/dashboard/forecast", get(dashboard::handle_forecast))
        // Tags
        .route("/v1/tags", get(tags::handle_list_tags))
        // Executions + logs
        .route("/v1/executions", get(handle_list_executions))
        .route(
            "/v1/executions/{id}/logs",
            get(execution_logs::handle_get_logs),
        )
        // Admin
        .route("/v1/admin/reload-config", post(admin::handle_reload_config))
        // Auth management
        .route(
            "/v1/api-clients",
            get(auth_endpoints::handle_list_clients).post(auth_endpoints::handle_create_client),
        )
        .route(
            "/v1/api-clients/{id}",
            put(auth_endpoints::handle_update_client).delete(auth_endpoints::handle_delete_client),
        )
        .route(
            "/v1/api-clients/{id}/tokens",
            post(auth_endpoints::handle_issue_client_token),
        )
        .route("/v1/api-keys", post(auth_endpoints::handle_create_api_key))
        .route(
            "/v1/api-keys/{id}",
            delete(auth_endpoints::handle_revoke_api_key),
        )
        .route_layer(middleware::from_fn_with_state(
            Arc::clone(&state),
            auth_middleware::require_auth,
        ));

    // Public routes (health + version + auth login/refresh/logout)
    let public = Router::new()
        .route("/health", get(handle_health))
        .route("/version", get(handle_version))
        .route("/v1/auth/login", post(auth_endpoints::handle_login))
        .route("/v1/auth/refresh", post(auth_endpoints::handle_refresh))
        .route("/v1/auth/logout", post(auth_endpoints::handle_logout));

    let cors = tower_http::cors::CorsLayer::permissive();

    authenticated.merge(public).with_state(state).layer(cors)
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

/// `POST /v1/poll` — heartbeat + work dispatch with long-poll support.
///
/// If the queue is empty and the runner has capacity, the handler waits up to
/// `LONG_POLL_TIMEOUT` for a `work_notify` signal before returning an empty
/// response. This eliminates the need for runners to busy-poll.
async fn handle_poll(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Json(req): Json<PollRequest>,
) -> (StatusCode, Json<PollResponse>) {
    if let Err(s) = require_scope(&ctx, Scope::WORK_POLL) {
        return (
            s,
            Json(PollResponse {
                work: vec![],
                cancel: vec![],
            }),
        );
    }
    // Update registry heartbeat
    {
        let mut reg = state.runner.registry.write().await;
        let result = reg.register_or_update(
            &req.runner_id,
            req.capabilities.clone(),
            req.max_inflight,
            req.inflight.clone(),
            req.instance_id.clone(),
            req.tags.clone(),
        );
        if let Err(conflict) = result {
            tracing::warn!(
                runner_id = %req.runner_id,
                conflicting_instance = %conflict,
                "runner instance conflict — another instance already registered"
            );
            return (
                StatusCode::CONFLICT,
                Json(PollResponse {
                    work: vec![],
                    cancel: vec![],
                }),
            );
        }
    }

    let capacity = (req.max_inflight as usize).saturating_sub(req.inflight.len());

    if capacity == 0 {
        // Runner is at capacity — no point waiting
        return (
            StatusCode::OK,
            Json(PollResponse {
                work: vec![],
                cancel: vec![],
            }),
        );
    }

    // Try to dequeue immediately; if nothing available, long-poll for up to
    // LONG_POLL_TIMEOUT waiting for a work_notify signal.
    loop {
        // Set up the notification listener BEFORE checking the queue so we
        // cannot miss an enqueue that races with our check.
        let notified = state.runner.work_notify.notified();

        let work =
            try_dequeue_for(&state.runner, &req.runner_id, &req.capabilities, capacity).await;

        if !work.is_empty() {
            // Mark claimed executions in the persistent store so we track runner_id
            if let Some(ref store) = state.store {
                let now = Utc::now();
                for w in &work {
                    if let Ok(id) = uuid::Uuid::parse_str(&w.execution_id) {
                        let _ = store.claim_execution(id, &req.runner_id, now);
                    }
                }
            }
            return (
                StatusCode::OK,
                Json(PollResponse {
                    work,
                    cancel: vec![],
                }),
            );
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
    Extension(ctx): Extension<CallerContext>,
    Json(req): Json<CompleteRequest>,
) -> (StatusCode, Json<CompleteResponse>) {
    if let Err(s) = require_scope(&ctx, Scope::WORK_ACK) {
        return (s, Json(CompleteResponse { received: false }));
    }
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
    Extension(ctx): Extension<CallerContext>,
) -> Result<Json<Vec<RunnerSummary>>, StatusCode> {
    require_scope(&ctx, Scope::RUNNERS_READ)?;
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
            tags: r.tags.clone(),
        })
        .collect();

    Ok(Json(summaries))
}

/// `DELETE /v1/runners/{id}` — deregister a runner.
async fn handle_delete_runner(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(runner_id): axum::extract::Path<String>,
) -> StatusCode {
    if let Err(s) = require_scope(&ctx, Scope::RUNNERS_WRITE) {
        return s;
    }
    let mut reg = state.runner.registry.write().await;
    reg.remove(&runner_id);
    StatusCode::NO_CONTENT
}

/// `POST /v1/trigger` — immediately enqueue a job execution.
///
/// Persists the execution to the store first (just like the scheduler does)
/// so that the CompletionProcessor can find it for retries and dead-lettering.
async fn handle_trigger(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Json(req): Json<TriggerRequest>,
) -> (StatusCode, Json<TriggerResponse>) {
    if let Err(s) = require_scope(&ctx, Scope::JOBS_TRIGGER) {
        return (
            s,
            Json(TriggerResponse {
                execution_id: String::new(),
                queued: 0,
            }),
        );
    }
    let now = Utc::now();
    let exec_uuid = uuid::Uuid::new_v4();
    let execution_id = exec_uuid.to_string();

    // Build metadata: start from the DSL job's compiled metadata so that
    // __runner_exec (and other DSL-stamped keys) survive into the WorkItem
    // and the DB execution row. The caller's req.metadata values are overlaid
    // on top so they can still override or extend individual entries.
    let mut metadata: HashMap<String, String> = if let Some(ref dsl_jobs) = state.dsl_jobs {
        let jobs = dsl_jobs.read().await;
        jobs.iter()
            .find(|j| j.key == req.job_key)
            .map(|j| j.metadata.clone())
            .unwrap_or_default()
    } else {
        HashMap::new()
    };
    if let serde_json::Value::Object(ref map) = req.metadata {
        for (k, v) in map {
            metadata.insert(k.clone(), v.as_str().unwrap_or(&v.to_string()).to_string());
        }
    }
    if !req.require.is_empty() {
        metadata.insert(
            "__require".into(),
            serde_json::to_string(&req.require).unwrap_or_default(),
        );
    }
    if !req.prefer.is_empty() {
        metadata.insert(
            "__prefer".into(),
            serde_json::to_string(&req.prefer).unwrap_or_default(),
        );
    }

    // Persist the execution record to the store so that the CompletionProcessor
    // can find it when the runner reports success/failure.
    if let Some(ref store) = state.store {
        let execution = Execution {
            id: exec_uuid,
            job_key: req.job_key.clone(),
            fire_at: now,
            attempt: 1,
            state: ExecutionState::Queued,
            runner_id: None,
            claimed_at: None,
            started_at: None,
            completed_at: None,
            duration_ms: None,
            error: None,
            dead_reason: None,
            metadata: metadata.clone(),
            created_at: now,
        };
        if let Err(e) = store.create_execution(&execution) {
            tracing::error!(job_key = %req.job_key, error = %e, "failed to persist triggered execution");
        }
    }

    let item = WorkItem {
        execution_id: execution_id.clone(),
        job_key: req.job_key,
        fire_at: now,
        attempt: 1,
        require: req.require,
        prefer: req.prefer,
        metadata: serde_json::to_value(&metadata).unwrap_or_default(),
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
        Json(TriggerResponse {
            execution_id,
            queued,
        }),
    )
}

/// `GET /v1/executions` — list recent executions from the store.
async fn handle_list_executions(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    require_scope(&ctx, Scope::EXECUTIONS_READ)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let filter = ExecutionFilter {
        job_key: params.get("job_key").cloned(),
        runner_id: params.get("runner_id").cloned(),
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
    let executions = store
        .list_executions(&filter)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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

/// `GET /version` response — build + environment metadata.
///
/// Public (no auth) so the login page can render a live version chip before
/// the user has a token. All four values are non-sensitive: the Cargo
/// version, the short git SHA, the build timestamp, and a deploy-environment
/// label (`production`, `staging`, `dev`, …) read from `CRONIQ_ENV`.
#[derive(Debug, Clone, Serialize)]
pub struct VersionResponse {
    pub version: &'static str,
    pub git_sha: &'static str,
    pub build_time: String,
    pub env: String,
}

/// Cargo package version, baked in at compile time.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Short git SHA, stamped by `build.rs`. Falls back to `"unknown"` outside
/// a git checkout (release tarball, no `git` on PATH).
const GIT_SHA: &str = env!("CRONIQ_GIT_SHA");

/// Unix seconds at which this binary was built. Stamped by `build.rs` and
/// formatted as RFC 3339 at request time.
const BUILD_TIME_UNIX: &str = env!("CRONIQ_BUILD_TIME_UNIX");

/// `GET /version` — build + environment metadata.
async fn handle_version() -> Json<VersionResponse> {
    let build_time = BUILD_TIME_UNIX
        .parse::<i64>()
        .ok()
        .and_then(|s| DateTime::<Utc>::from_timestamp(s, 0))
        .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_else(|| "unknown".into());

    let env = std::env::var("CRONIQ_ENV").unwrap_or_else(|_| "unknown".into());

    Json(VersionResponse {
        version: VERSION,
        git_sha: GIT_SHA,
        build_time,
        env,
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
            let _ = reg.register_or_update("r1", vec![], 3, vec!["exec-42".into()], None, vec![]);
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
            let _ = reg.register_or_update("r1", vec![], 3, vec!["exec-99".into()], None, vec![]);
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

    #[tokio::test]
    async fn version_returns_build_metadata() {
        let (state, _rx) = make_state();
        let app = server_router(Arc::clone(&state));

        let resp = get_json(app, "/version").await;
        // Cargo always sets CARGO_PKG_VERSION, so this is never "unknown".
        assert_eq!(resp["version"], env!("CARGO_PKG_VERSION"));
        // git_sha + build_time are stamped by build.rs. We don't assert exact
        // values (they change every commit/build), only that they're present.
        assert!(resp["git_sha"].is_string());
        assert!(resp["build_time"].is_string());
        // env is read from CRONIQ_ENV at request time. The test process may or
        // may not have it set, but the field must always be a string.
        assert!(resp["env"].is_string());
    }

    #[tokio::test]
    async fn version_is_public() {
        // The login page renders before auth, so /version must not require a
        // token even when JWT auth is configured.
        let (state, _rx) = make_auth_state("test-secret");
        let app = server_router(Arc::clone(&state));

        let status = status_of(app, "GET", "/version", None, None).await;
        assert_eq!(status, 200);
    }

    #[tokio::test]
    async fn version_reads_env_var() {
        // SAFETY: tests in this module run on the tokio current-thread runtime
        // and don't race on env vars within a single test. CRONIQ_ENV is only
        // read by handle_version, so isolating to one test is sufficient.
        //
        // # Safety
        // `set_var` / `remove_var` are unsafe in Rust 2024 because they mutate
        // process-global state that may be observed by other threads. We
        // accept the risk here: the test runtime is single-threaded for this
        // test and no other code path reads CRONIQ_ENV concurrently.
        unsafe {
            std::env::set_var("CRONIQ_ENV", "staging");
        }
        let (state, _rx) = make_state();
        let app = server_router(Arc::clone(&state));

        let resp = get_json(app, "/version").await;
        assert_eq!(resp["env"], "staging");

        unsafe {
            std::env::remove_var("CRONIQ_ENV");
        }
    }

    // ─── Auth middleware tests ────────────────────────────────────────────────

    fn make_auth_state(
        secret: &str,
    ) -> (Arc<ServerState>, mpsc::UnboundedReceiver<CompletionEvent>) {
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
            scheduler_tx: None,
            triggers: None,
            dsl_jobs: None,
            dsl_calendars: None,
            policy_dsl_adopt_on_mutate: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            config_path: None,
            reload_counters: ReloadCounters::new(),
        });
        (state, rx)
    }

    async fn status_of(
        app: Router,
        method: &str,
        uri: &str,
        body: Option<serde_json::Value>,
        bearer: Option<&str>,
    ) -> u16 {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(token) = bearer {
            builder = builder.header("authorization", format!("Bearer {token}"));
        }
        if body.is_some() {
            builder = builder.header("content-type", "application/json");
        }
        let body = body
            .map(|b| Body::from(b.to_string()))
            .unwrap_or(Body::empty());
        let resp = app.oneshot(builder.body(body).unwrap()).await.unwrap();
        resp.status().as_u16()
    }

    #[tokio::test]
    async fn auth_rejects_without_token() {
        let (state, _rx) = make_auth_state("test-secret");
        let app = server_router(Arc::clone(&state));

        let status = status_of(
            app,
            "POST",
            "/v1/poll",
            Some(serde_json::json!({
                "runner_id": "r1", "capabilities": [], "max_inflight": 1, "inflight": []
            })),
            None,
        )
        .await;

        assert_eq!(status, 401);
    }

    #[tokio::test]
    async fn auth_rejects_invalid_jwt() {
        let (state, _rx) = make_auth_state("test-secret");
        let app = server_router(Arc::clone(&state));

        let status = status_of(
            app,
            "POST",
            "/v1/poll",
            Some(serde_json::json!({
                "runner_id": "r1", "capabilities": [], "max_inflight": 1, "inflight": []
            })),
            Some("invalid.jwt.token"),
        )
        .await;

        assert_eq!(status, 401);
    }

    #[tokio::test]
    async fn auth_accepts_valid_jwt() {
        let (state, _rx) = make_auth_state("test-secret");
        let jwt_config = state.jwt_config.as_ref().unwrap();
        let pair = croniq_auth::jwt::issue_token_pair(
            jwt_config,
            "test-user",
            "test-client",
            croniq_auth::CallerType::User,
            &["admin".into()],
        )
        .unwrap();

        let app = server_router(Arc::clone(&state));
        let status = status_of(
            app,
            "POST",
            "/v1/poll",
            Some(serde_json::json!({
                "runner_id": "r1", "capabilities": [], "max_inflight": 1, "inflight": []
            })),
            Some(&pair.access_token),
        )
        .await;

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
        let status = status_of(
            app,
            "POST",
            "/v1/auth/login",
            Some(serde_json::json!({
                "username": "admin", "password": "pass"
            })),
            None,
        )
        .await;

        // 503 because no store configured, but NOT 401/404
        assert_eq!(status, 503);
    }

    // ─── Trigger endpoint: DSL metadata propagation (issue #89) ─────────────

    #[tokio::test]
    async fn trigger_inherits_dsl_runner_exec_metadata() {
        // Regression for issue #89: POST /v1/trigger must include __runner_exec
        // from the DSL-compiled job metadata so the shell runner can decode the
        // command. {{...}} inside quoted command strings must survive the round-trip.
        use crate::loader::load_str;
        use croniq_config::compile::RUNNER_EXEC_METADATA_KEY;

        let dsl = r#"
            job test:docker-ps {
                every 1 hour
                runner { require shell-runner }
                runner shell {
                    command "docker ps --format '{{.Image}}'"
                }
            }
        "#;
        let loaded = load_str(dsl).unwrap();
        let jobs = loaded.runtime.jobs;
        assert!(
            jobs[0].metadata.contains_key(RUNNER_EXEC_METADATA_KEY),
            "compile should stamp __runner_exec: {:?}",
            jobs[0].metadata
        );

        let runner = AppState::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        let dsl_jobs = Arc::new(tokio::sync::RwLock::new(jobs));
        let state = Arc::new(ServerState {
            runner,
            completion_tx: tx,
            long_poll_timeout: Duration::from_millis(50),
            jwt_config: None,
            store: None,
            scheduler_tx: None,
            triggers: None,
            dsl_jobs: Some(dsl_jobs),
            dsl_calendars: None,
            policy_dsl_adopt_on_mutate: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            config_path: None,
            reload_counters: ReloadCounters::new(),
        });
        let app = server_router(Arc::clone(&state));

        let resp = post_json(
            app,
            "/v1/trigger",
            serde_json::json!({
                "job_key": "test:docker-ps",
                "metadata": {},
                "require": [],
                "prefer": []
            }),
        )
        .await;

        assert!(
            resp["execution_id"].is_string(),
            "expected execution_id in response, got: {resp}"
        );

        // The WorkItem in the queue must carry __runner_exec so the shell runner
        // can decode the command string (the original failure mode from issue #89).
        let q = state.runner.queue.read().await;
        let items = q.peek_n(1);
        assert_eq!(items.len(), 1, "one item should be queued");
        assert!(
            items[0].metadata.get(RUNNER_EXEC_METADATA_KEY).is_some(),
            "__runner_exec must be present in WorkItem.metadata; got: {:?}",
            items[0].metadata
        );
    }

    #[tokio::test]
    async fn trigger_request_metadata_overrides_dsl_metadata() {
        // Caller-supplied metadata overrides DSL values but does not wipe DSL keys
        // that the caller did not touch (e.g. __runner_exec stays present).
        use crate::loader::load_str;
        use croniq_config::compile::RUNNER_EXEC_METADATA_KEY;

        let dsl = r#"
            job test:override {
                every 1 hour
                runner shell { command "echo hello" }
                metadata { env prod }
            }
        "#;
        let loaded = load_str(dsl).unwrap();
        let jobs = loaded.runtime.jobs;

        let runner = AppState::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        let dsl_jobs = Arc::new(tokio::sync::RwLock::new(jobs));
        let state = Arc::new(ServerState {
            runner,
            completion_tx: tx,
            long_poll_timeout: Duration::from_millis(50),
            jwt_config: None,
            store: None,
            scheduler_tx: None,
            triggers: None,
            dsl_jobs: Some(dsl_jobs),
            dsl_calendars: None,
            policy_dsl_adopt_on_mutate: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            config_path: None,
            reload_counters: ReloadCounters::new(),
        });
        let app = server_router(Arc::clone(&state));

        post_json(
            app,
            "/v1/trigger",
            serde_json::json!({
                "job_key": "test:override",
                "metadata": { "env": "staging" },
                "require": [],
                "prefer": []
            }),
        )
        .await;

        let q = state.runner.queue.read().await;
        let items = q.peek_n(1);
        assert_eq!(items.len(), 1);
        // __runner_exec from DSL must survive
        assert!(
            items[0].metadata.get(RUNNER_EXEC_METADATA_KEY).is_some(),
            "__runner_exec must survive caller override"
        );
        // Caller's env=staging must override DSL env=prod
        assert_eq!(
            items[0].metadata["env"].as_str().unwrap(),
            "staging",
            "caller env must override DSL env"
        );
    }
}

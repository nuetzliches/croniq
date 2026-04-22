//! Jobs CRUD endpoints.

use std::sync::Arc;

use axum::{Json, extract::State, http::StatusCode};
use chrono::Utc;
use croniq_store::models::{JobDefinition, TriggerDefinition};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::ServerState;
use crate::loader::{trigger_from_definition, job_config_from_definition};
use crate::scheduler::SchedulerCommand;

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

/// `GET /v1/jobs`
pub async fn handle_list(
    State(state): State<Arc<ServerState>>,
) -> Result<Json<Vec<JobDefinition>>, StatusCode> {
    let store = state.store.as_ref().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let jobs = store.list_job_definitions().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(jobs))
}

/// `GET /v1/jobs/{job_key}`
pub async fn handle_get(
    State(state): State<Arc<ServerState>>,
    axum::extract::Path(job_key): axum::extract::Path<String>,
) -> Result<Json<JobDefinition>, StatusCode> {
    let store = state.store.as_ref().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    store.get_job_definition(&job_key)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

/// `POST /v1/jobs`
pub async fn handle_create(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<CreateJobRequest>,
) -> Result<(StatusCode, Json<JobDefinition>), StatusCode> {
    let store = state.store.as_ref().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
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
    store.create_job_definition(&job).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((StatusCode::CREATED, Json(job)))
}

/// `DELETE /v1/jobs/{job_key}`
pub async fn handle_delete(
    State(state): State<Arc<ServerState>>,
    axum::extract::Path(job_key): axum::extract::Path<String>,
) -> StatusCode {
    let Some(store) = state.store.as_ref() else { return StatusCode::SERVICE_UNAVAILABLE };
    match store.delete_job_definition(&job_key) {
        Ok(_) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// `POST /v1/jobs/{job_key}/activate`
pub async fn handle_activate(
    State(state): State<Arc<ServerState>>,
    axum::extract::Path(job_key): axum::extract::Path<String>,
) -> Result<Json<JobDefinition>, StatusCode> {
    let store = state.store.as_ref().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let mut job = store.get_job_definition(&job_key)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    job.is_active = true;
    job.updated_at = Utc::now();
    store.create_job_definition(&job).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(job))
}

/// `POST /v1/jobs/{job_key}/deactivate`
pub async fn handle_deactivate(
    State(state): State<Arc<ServerState>>,
    axum::extract::Path(job_key): axum::extract::Path<String>,
) -> Result<Json<JobDefinition>, StatusCode> {
    let store = state.store.as_ref().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let mut job = store.get_job_definition(&job_key)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    job.is_active = false;
    job.updated_at = Utc::now();
    store.create_job_definition(&job).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
/// - `managed_by: "dsl"` exists → skip (Croniqfile has precedence)
/// - `managed_by: "runner"/"api"` exists → update
/// - Not found → create
pub async fn handle_register(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<RegisterJobRequest>,
) -> Result<(StatusCode, Json<RegisterJobResponse>), StatusCode> {
    let store = state.store.as_ref().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let now = Utc::now();

    // Check collision: if managed_by "dsl" exists, skip
    let existing_triggers = store.list_triggers(Some(&req.job_key))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if existing_triggers.iter().any(|t| t.managed_by == "dsl") {
        return Ok((StatusCode::OK, Json(RegisterJobResponse {
            job_key: req.job_key,
            trigger_id: existing_triggers[0].trigger_id.clone(),
            status: "skipped_dsl_precedence".into(),
        })));
    }

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
    store.create_job_definition(&job_def).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Create or update trigger definition
    let trigger_id = existing_triggers.first()
        .map(|t| t.trigger_id.clone())
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let trigger_def = TriggerDefinition {
        trigger_id: trigger_id.clone(),
        job_key: req.job_key.clone(),
        cron_expression: Some(req.schedule.clone()),
        timezone: req.timezone.clone(),
        calendar: None,
        window: None,
        not_before: None,
        not_after: None,
        enabled: true,
        managed_by: "runner".into(),
        created_at: now,
        updated_at: now,
    };
    store.create_trigger(&trigger_def).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Push to live scheduler
    let status = if let Some(ref tx) = state.scheduler_tx {
        if let Some(trigger) = trigger_from_definition(&trigger_def, now) {
            let job_config = job_config_from_definition(&trigger_def, Some(&job_def));
            let _ = tx.send(SchedulerCommand::AddJob { job: Box::new(job_config), trigger: Box::new(trigger) });
            "registered"
        } else {
            "registered_no_schedule"
        }
    } else {
        "registered_no_scheduler"
    };

    let code = if existing_triggers.is_empty() { StatusCode::CREATED } else { StatusCode::OK };
    Ok((code, Json(RegisterJobResponse {
        job_key: req.job_key,
        trigger_id,
        status: status.into(),
    })))
}

//! Jobs CRUD endpoints.

use std::sync::Arc;

use axum::{Json, extract::State, http::StatusCode};
use chrono::Utc;
use croniq_store::models::JobDefinition;
use croniq_store::traits::JobDefinitionStore;
use serde::Deserialize;

use super::ServerState;

#[derive(Deserialize)]
pub struct CreateJobRequest {
    pub job_key: String,
    pub description: Option<String>,
    pub assigned_runner_id: Option<String>,
    #[serde(default)]
    pub metadata: std::collections::HashMap<String, String>,
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

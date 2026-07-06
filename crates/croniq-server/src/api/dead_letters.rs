//! Dead letter management endpoints.

use std::sync::Arc;

use axum::{
    Extension, Json,
    extract::{Query, State},
    http::StatusCode,
};
use chrono::Utc;
use croniq_auth::CallerContext;
use croniq_auth::context::Scope;
use croniq_runner::WorkItem;
use croniq_store::models::{DeadLetter, DeadLetterFilter, Execution, ExecutionState};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::ServerState;
use crate::api::auth_middleware::require_scope;

#[derive(Deserialize, Default)]
pub struct ListQuery {
    pub job_key: Option<String>,
    pub limit: Option<u32>,
}

/// `GET /v1/dead-letters`
pub async fn handle_list(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<DeadLetter>>, StatusCode> {
    require_scope(&ctx, Scope::DEAD_LETTERS_READ)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let filter = DeadLetterFilter {
        job_key: q.job_key,
        limit: Some(q.limit.unwrap_or(50)),
    };
    let letters = store
        .list_dead_letters(&filter)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(letters))
}

/// `GET /v1/dead-letters/{id}`
pub async fn handle_get(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<DeadLetter>, StatusCode> {
    require_scope(&ctx, Scope::DEAD_LETTERS_READ)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    store
        .get_dead_letter(uuid)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

/// `DELETE /v1/dead-letters/{id}`
pub async fn handle_delete(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> StatusCode {
    if let Err(s) = require_scope(&ctx, Scope::DEAD_LETTERS_WRITE) {
        return s;
    }
    let Some(store) = state.store.as_ref() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let Ok(uuid) = Uuid::parse_str(&id) else {
        return StatusCode::BAD_REQUEST;
    };
    match store.remove_dead_letter(uuid) {
        Ok(_) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[derive(Serialize)]
pub struct ReplayResponse {
    pub execution_id: String,
    pub attempt: u32,
}

/// `POST /v1/dead-letters/{id}/replay` — replay a dead letter as a new execution.
pub async fn handle_replay(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<(StatusCode, Json<ReplayResponse>), StatusCode> {
    require_scope(&ctx, Scope::DEAD_LETTERS_WRITE)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let dl_uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;

    let dl = store
        .get_dead_letter(dl_uuid)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let now = Utc::now();
    let new_id = Uuid::new_v4();
    let next_attempt = dl.attempt + 1;

    // Create a new execution from the dead letter
    let execution = Execution {
        id: new_id,
        job_key: dl.job_key.clone(),
        fire_at: now,
        attempt: next_attempt,
        state: ExecutionState::Queued,
        runner_id: None,
        claimed_at: None,
        started_at: None,
        completed_at: None,
        duration_ms: None,
        error: None,
        dead_reason: None,
        idempotency_key: None,
        metadata: dl.metadata.clone(),
        created_at: now,
    };

    store
        .create_execution(&execution)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    store
        .remove_dead_letter(dl_uuid)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Enqueue work item
    let item = WorkItem {
        execution_id: new_id.to_string(),
        job_key: dl.job_key,
        fire_at: now,
        attempt: next_attempt,
        require: dl
            .metadata
            .get("__require")
            .and_then(|v| serde_json::from_str(v).ok())
            .unwrap_or_default(),
        prefer: dl
            .metadata
            .get("__prefer")
            .and_then(|v| serde_json::from_str(v).ok())
            .unwrap_or_default(),
        metadata: serde_json::Value::Object(
            dl.metadata
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                .collect(),
        ),
        timeout: "5m".into(),
    };

    {
        let mut q = state.runner.queue.write().await;
        q.enqueue(item);
    }
    state.runner.work_notify.notify_waiters();

    Ok((
        StatusCode::OK,
        Json(ReplayResponse {
            execution_id: new_id.to_string(),
            attempt: next_attempt,
        }),
    ))
}

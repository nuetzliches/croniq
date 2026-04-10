//! Extended work protocol endpoints: lease renewal and structured events.

use std::sync::Arc;

use axum::{Json, extract::State, http::StatusCode};
use chrono::Utc;
use croniq_store::models::ExecutionLogEntry;
use croniq_store::traits::ExecutionLogStore;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::ServerState;

// ─── Renew ───

#[derive(Deserialize)]
pub struct RenewRequest {
    pub runner_id: String,
    pub execution_id: String,
}

#[derive(Serialize)]
pub struct RenewResponse {
    pub renewed: bool,
}

/// `POST /v1/work/renew` — renew a lease on a claimed execution.
pub async fn handle_renew(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<RenewRequest>,
) -> (StatusCode, Json<RenewResponse>) {
    // Update the runner's last_poll_at to extend its liveness
    let mut reg = state.runner.registry.write().await;
    if let Some(runner) = reg.get_mut(&req.runner_id) {
        runner.last_poll_at = Utc::now();
        (StatusCode::OK, Json(RenewResponse { renewed: true }))
    } else {
        (StatusCode::NOT_FOUND, Json(RenewResponse { renewed: false }))
    }
}

// ─── Events ───

#[derive(Deserialize)]
pub struct WorkEvent {
    pub level: Option<String>,
    pub message: String,
    #[serde(default)]
    pub fields: std::collections::HashMap<String, String>,
}

#[derive(Serialize)]
pub struct EventsResponse {
    pub accepted: usize,
}

/// `POST /v1/work/{execution_id}:events` — push structured log events.
pub async fn handle_events(
    State(state): State<Arc<ServerState>>,
    axum::extract::Path(execution_id): axum::extract::Path<String>,
    Json(events): Json<Vec<WorkEvent>>,
) -> Result<(StatusCode, Json<EventsResponse>), StatusCode> {
    let store = state.store.as_ref().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let exec_uuid = Uuid::parse_str(&execution_id).map_err(|_| StatusCode::BAD_REQUEST)?;

    let mut accepted = 0;
    for event in &events {
        let entry = ExecutionLogEntry {
            id: Uuid::new_v4(),
            execution_id: exec_uuid,
            timestamp: Utc::now(),
            level: event.level.clone().unwrap_or_else(|| "info".into()),
            message: event.message.clone(),
            fields: event.fields.clone(),
        };
        if store.append_log(&entry).is_ok() {
            accepted += 1;
        }
    }

    Ok((StatusCode::OK, Json(EventsResponse { accepted })))
}

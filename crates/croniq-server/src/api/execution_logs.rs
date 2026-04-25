//! Execution log endpoints.

use std::sync::Arc;

use axum::{Json, extract::State, http::StatusCode};
use croniq_store::models::ExecutionLogEntry;
use uuid::Uuid;

use super::ServerState;

/// `GET /v1/executions/{id}/logs`
pub async fn handle_get_logs(
    State(state): State<Arc<ServerState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<Vec<ExecutionLogEntry>>, StatusCode> {
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let logs = store
        .read_logs(uuid, 1000)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(logs))
}

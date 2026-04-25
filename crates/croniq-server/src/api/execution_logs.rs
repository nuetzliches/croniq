//! Execution log endpoints.

use std::sync::Arc;

use axum::{Extension, Json, extract::State, http::StatusCode};
use croniq_auth::CallerContext;
use croniq_auth::context::Scope;
use croniq_store::models::ExecutionLogEntry;
use uuid::Uuid;

use super::ServerState;
use crate::api::auth_middleware::require_scope;

/// `GET /v1/executions/{id}/logs`
pub async fn handle_get_logs(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<Vec<ExecutionLogEntry>>, StatusCode> {
    require_scope(&ctx, Scope::EXECUTIONS_READ)?;
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

//! Execution log endpoints.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{Extension, Json, extract::State, http::StatusCode};
use croniq_auth::CallerContext;
use croniq_auth::context::Scope;
use croniq_store::models::ExecutionLogEntry;
use uuid::Uuid;

use super::ServerState;
use crate::api::auth_middleware::require_scope;

/// Hard cap on rows returned per request. Per-line emission (#108) means a
/// chatty job can produce a few thousand log rows; 10k is enough headroom
/// for a typical CVE scan or large-test-suite run while bounding response
/// size and DB read time.
const LOG_LIMIT: u32 = 10_000;

/// `GET /v1/executions/{id}/logs?level=warn`
///
/// Optional `level` query parameter narrows the response to a single level
/// (`info` / `warn` / `error`). Filtering happens server-side after read so
/// the result is deterministic across pagination boundaries.
pub async fn handle_get_logs(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Result<Json<Vec<ExecutionLogEntry>>, StatusCode> {
    require_scope(&ctx, Scope::EXECUTIONS_READ)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let mut logs = store
        .read_logs(uuid, LOG_LIMIT)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if let Some(level) = params.get("level") {
        let wanted = level.as_str();
        logs.retain(|l| l.level == wanted);
    }

    Ok(Json(logs))
}

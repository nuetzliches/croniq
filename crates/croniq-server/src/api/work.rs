//! Extended work protocol endpoints: lease renewal and structured events.

use std::sync::Arc;

use axum::{Extension, Json, extract::State, http::StatusCode};
use chrono::Utc;
use croniq_auth::CallerContext;
use croniq_auth::context::Scope;
use croniq_store::models::ExecutionLogEntry;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::ServerState;
use crate::api::auth_middleware::require_scope;
use crate::api::runner_identity;

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
    Extension(ctx): Extension<CallerContext>,
    Json(req): Json<RenewRequest>,
) -> (StatusCode, Json<RenewResponse>) {
    if let Err(s) = require_scope(&ctx, Scope::WORK_RENEW) {
        return (s, Json(RenewResponse { renewed: false }));
    }
    // A lease belongs to a runner, so only that runner's credential may extend
    // it — otherwise a foreign caller could keep a dead runner looking alive
    // and suppress the watchdog's requeue of its abandoned executions.
    if let Err(s) = runner_identity::authorize_runner(&state, &ctx, &req.runner_id) {
        return (s, Json(RenewResponse { renewed: false }));
    }
    // Update the runner's last_poll_at to extend its liveness
    let mut reg = state.runner.registry.write().await;
    if let Some(runner) = reg.get_mut(&req.runner_id) {
        runner.last_poll_at = Utc::now();
        (StatusCode::OK, Json(RenewResponse { renewed: true }))
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(RenewResponse { renewed: false }),
        )
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
///
/// Per-line log emission (#108) means a single shell-runner job may push
/// thousands of events at once. Use the bulk-insert path so the entire
/// batch lands in one transaction with auto-assigned `seq` numbers
/// instead of one lock + INSERT per event.
pub async fn handle_events(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(execution_id): axum::extract::Path<String>,
    Json(events): Json<Vec<WorkEvent>>,
) -> Result<(StatusCode, Json<EventsResponse>), StatusCode> {
    require_scope(&ctx, Scope::WORK_EVENTS)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let exec_uuid = Uuid::parse_str(&execution_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    // This endpoint is addressed by execution, not by runner, so the fence is
    // the execution's claiming runner: only the credential that owns that
    // runner may write to the execution's log. Without it, `work:events` plus
    // any execution id is enough to inject or forge another runner's logs.
    runner_identity::authorize_execution(&state, &ctx, exec_uuid)?;

    let now = Utc::now();
    let entries: Vec<ExecutionLogEntry> = events
        .iter()
        .map(|event| ExecutionLogEntry {
            id: Uuid::new_v4(),
            execution_id: exec_uuid,
            timestamp: now,
            level: event.level.clone().unwrap_or_else(|| "info".into()),
            message: event.message.clone(),
            fields: event.fields.clone(),
            seq: 0, // assigned by store on insert
        })
        .collect();

    let total = entries.len();
    match store.append_logs_batch(&entries) {
        Ok(()) => Ok((StatusCode::OK, Json(EventsResponse { accepted: total }))),
        Err(e) => {
            tracing::warn!(execution_id = %execution_id, error = %e, "failed to append log batch");
            Ok((StatusCode::OK, Json(EventsResponse { accepted: 0 })))
        }
    }
}

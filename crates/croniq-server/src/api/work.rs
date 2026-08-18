//! Extended work protocol endpoints: lease renewal and structured events.

use std::sync::Arc;

use axum::{Extension, Json, extract::State, http::StatusCode};
use chrono::Utc;
use croniq_auth::CallerContext;
use croniq_auth::context::Scope;
use croniq_store::models::{ExecutionLogEntry, ExecutionState};
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

/// `POST /v1/work/renew` — renew the caller's lease on ONE claimed execution.
///
/// Per-execution semantics (issue #438): the named execution must exist
/// (`404` otherwise), be `claimed`, and be held by the runner the caller acts
/// as (`409` otherwise — the lease is not this runner's to extend). On
/// success exactly that execution's lease timestamp is refreshed, which is
/// the liveness exemption the watchdog's stale-claim reaper honours; the
/// runner's other claims do not ride along on the renew. The runner's
/// registry heartbeat is still bumped, because a renew does prove the
/// process is alive.
pub async fn handle_renew(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Json(req): Json<RenewRequest>,
) -> (StatusCode, Json<RenewResponse>) {
    fn refused(status: StatusCode) -> (StatusCode, Json<RenewResponse>) {
        (status, Json(RenewResponse { renewed: false }))
    }

    if let Err(s) = require_scope(&ctx, Scope::WORK_RENEW) {
        return refused(s);
    }
    // A lease belongs to a runner, so only that runner's credential may extend
    // it — otherwise a foreign caller could keep a dead runner looking alive
    // and suppress the watchdog's requeue of its abandoned executions.
    if let Err(s) = runner_identity::authorize_runner(&state, &ctx, &req.runner_id) {
        return refused(s);
    }

    let now = Utc::now();

    // Consult the named execution: `renewed: true` must mean exactly that —
    // this execution exists, is claimed by this runner, and its lease was
    // extended. Without a store there is no claim state to consult (bare
    // dev/test servers, where the stale-claim reaper does not run either),
    // so only the heartbeat semantics below remain.
    if let Some(store) = state.store.as_ref() {
        let Ok(exec_uuid) = Uuid::parse_str(&req.execution_id) else {
            // A malformed id names no execution.
            return refused(StatusCode::NOT_FOUND);
        };
        match store.get_execution(exec_uuid) {
            Ok(Some(execution)) => {
                if execution.state != ExecutionState::Claimed
                    || execution.runner_id.as_deref() != Some(req.runner_id.as_str())
                {
                    // Completed, cancelled, requeued by the watchdog, or held
                    // by a different runner: the lease is gone (or never was
                    // this runner's). Not retryable — the runner should stop
                    // renewing; a completion it still reports is judged by
                    // the completion CAS on its own.
                    return refused(StatusCode::CONFLICT);
                }
            }
            Ok(None) => {
                // Ephemeral executions (issue #263) intentionally have no
                // store row but are real dispatched work whose renew timer
                // runs like any other — recognise them instead of refusing.
                let ephemeral = state
                    .runner
                    .ephemeral_inflight
                    .read()
                    .await
                    .contains_key(&req.execution_id);
                if !ephemeral {
                    return refused(StatusCode::NOT_FOUND);
                }
            }
            Err(e) => {
                tracing::warn!(
                    execution_id = %req.execution_id,
                    error = %e,
                    "renew: could not load execution — refusing (retryable)"
                );
                return refused(StatusCode::SERVICE_UNAVAILABLE);
            }
        }
    }

    // Refresh THIS execution's lease — the stale-claim reaper's per-execution
    // liveness exemption (issue #438).
    state
        .runner
        .touch_leases(&req.runner_id, std::slice::from_ref(&req.execution_id), now)
        .await;

    // A successful renew still proves the runner process is alive: keep its
    // registry heartbeat fresh so the dead-runner sweep does not requeue the
    // session's claims. Absence from the registry (e.g. right after a server
    // restart, before the next poll re-registers) does not invalidate the
    // store-backed claim verified above.
    if let Some(runner) = state.runner.registry.write().await.get_mut(&req.runner_id) {
        runner.last_poll_at = now;
    }

    (StatusCode::OK, Json(RenewResponse { renewed: true }))
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

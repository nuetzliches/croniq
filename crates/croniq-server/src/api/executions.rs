//! Execution-lifecycle endpoints. Currently only the cancel endpoint —
//! issue #176 (server-side cancel-via-poll routing). The list endpoint
//! stays in `mod.rs` for now until other read endpoints land.

use std::sync::Arc;

use axum::{Extension, Json, extract::State, http::StatusCode};
use chrono::Utc;
use croniq_auth::CallerContext;
use croniq_auth::context::Scope;
use croniq_store::models::ExecutionState;
use serde::Serialize;
use uuid::Uuid;

use super::ServerState;
use crate::api::audit;
use crate::api::auth_middleware::require_scope;

/// `POST /v1/executions/{id}/cancel`
///
/// Cancel a queued or in-flight execution. Queued executions are cancelled
/// directly in the store. Claimed (running) executions are also flipped to
/// `cancelled` in the store synchronously and the execution_id is pushed
/// onto the owning runner's cancel queue — the runner picks it up on its
/// next poll and aborts the handler.
///
/// Idempotency: a second cancel for an already-cancelled execution returns
/// `200 OK` so retries from the dashboard don't surprise the operator.
///
/// Scope: `executions:cancel` (or `admin`).
pub async fn handle_cancel(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<CancelResponse>, StatusCode> {
    require_scope(&ctx, Scope::EXECUTIONS_CANCEL)?;

    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;

    let execution = store
        .get_execution(uuid)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let now = Utc::now();
    let (status_code, delivered_via_runner) = match execution.state {
        ExecutionState::Cancelled => {
            // Idempotent — already done, no audit (avoid log spam from
            // retried clicks).
            (200u16, false)
        }
        ExecutionState::Queued => {
            store
                .cancel_execution(uuid, now)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            (200, false)
        }
        ExecutionState::Claimed => {
            let runner_id = execution
                .runner_id
                .as_deref()
                .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
            // Push onto the runner's cancel queue BEFORE flipping store
            // state — if push_cancel panicked (it can't, but defensively),
            // we don't leave the store in `cancelled` with the runner still
            // running. Order doesn't affect correctness either way since
            // both are independent operations.
            state.runner.push_cancel(runner_id, &uuid.to_string()).await;
            store
                .cancel_execution(uuid, now)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            (200, true)
        }
        ExecutionState::Completed | ExecutionState::Failed | ExecutionState::Dead => {
            return Err(StatusCode::CONFLICT);
        }
    };

    if status_code == 200 && !matches!(execution.state, ExecutionState::Cancelled) {
        audit::record(
            store,
            &ctx,
            "execution.cancelled",
            "execution",
            Some(&uuid.to_string()),
            None,
        );
    }

    Ok(Json(CancelResponse {
        execution_id: uuid.to_string(),
        cancelled: true,
        delivered_via_runner,
    }))
}

#[derive(Serialize)]
pub struct CancelResponse {
    pub execution_id: String,
    /// `true` if the cancel was acknowledged (queued or claimed → cancelled,
    /// or already cancelled). Never `false` today; the field exists so
    /// callers can pattern on it as the lifecycle gets richer.
    pub cancelled: bool,
    /// `true` if the execution was in flight on a runner and the cancel was
    /// pushed to that runner's cancel queue (will arrive on the runner's
    /// next poll). `false` for cancels of still-queued executions, where
    /// the runner never saw the work.
    pub delivered_via_runner: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{ServerState, server_router};
    use crate::store::{DynStore, sqlite_store};
    use axum::body::Body;
    use axum::http::Request;
    use croniq_runner::AppState;
    use croniq_store::models::Execution;
    use croniq_store::sqlite::SqliteStore;
    use http_body_util::BodyExt;
    use std::collections::HashMap;
    use tokio::sync::mpsc;
    use tower::util::ServiceExt;

    fn make_store() -> DynStore {
        sqlite_store(SqliteStore::in_memory().unwrap())
    }

    fn make_state(store: DynStore) -> Arc<ServerState> {
        let runner = AppState::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        ServerState::with_auth(runner, tx, None, Some(store))
    }

    fn seed_execution(store: &DynStore, state: ExecutionState, runner_id: Option<&str>) -> Uuid {
        let id = Uuid::new_v4();
        let now = Utc::now();
        store
            .create_execution(&Execution {
                id,
                job_key: "test:job".into(),
                fire_at: now,
                attempt: 1,
                state: ExecutionState::Queued,
                runner_id: None,
                claimed_at: None,
                started_at: None,
                completed_at: None,
                duration_ms: None,
                error: None,
                dead_reason: None,
                metadata: HashMap::new(),
                created_at: now,
            })
            .unwrap();
        if let ExecutionState::Claimed = state {
            store
                .claim_execution(id, runner_id.expect("claimed needs runner_id"), now)
                .unwrap();
        }
        id
    }

    async fn post(app: axum::Router, uri: &str) -> (u16, serde_json::Value) {
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
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

    #[tokio::test]
    async fn cancel_queued_execution_flips_state_in_store() {
        let store = make_store();
        let exec_id = seed_execution(&store, ExecutionState::Queued, None);
        let state = make_state(store.clone());
        let app = server_router(Arc::clone(&state));

        let (status, body) = post(app, &format!("/v1/executions/{exec_id}/cancel")).await;
        assert_eq!(status, 200);
        assert_eq!(body["cancelled"], true);
        assert_eq!(body["delivered_via_runner"], false);

        let after = store.get_execution(exec_id).unwrap().unwrap();
        assert_eq!(after.state, ExecutionState::Cancelled);
    }

    #[tokio::test]
    async fn cancel_claimed_execution_pushes_to_runner_queue() {
        let store = make_store();
        let exec_id = seed_execution(&store, ExecutionState::Claimed, Some("r1"));
        let state = make_state(store.clone());
        let app = server_router(Arc::clone(&state));

        let (status, body) = post(app, &format!("/v1/executions/{exec_id}/cancel")).await;
        assert_eq!(status, 200);
        assert_eq!(body["delivered_via_runner"], true);

        // Store flipped to cancelled
        let after = store.get_execution(exec_id).unwrap().unwrap();
        assert_eq!(after.state, ExecutionState::Cancelled);

        // Runner queue contains the cancel
        let pending = state.runner.drain_cancels("r1").await;
        assert_eq!(pending, vec![exec_id.to_string()]);
    }

    #[tokio::test]
    async fn cancel_already_cancelled_is_idempotent() {
        let store = make_store();
        let exec_id = seed_execution(&store, ExecutionState::Queued, None);
        store.cancel_execution(exec_id, Utc::now()).unwrap();
        let state = make_state(store);
        let app = server_router(Arc::clone(&state));

        let (status, _body) = post(app, &format!("/v1/executions/{exec_id}/cancel")).await;
        assert_eq!(status, 200);
    }

    #[tokio::test]
    async fn cancel_unknown_execution_returns_404() {
        let state = make_state(make_store());
        let app = server_router(Arc::clone(&state));
        let unknown = Uuid::new_v4();

        let (status, _) = post(app, &format!("/v1/executions/{unknown}/cancel")).await;
        assert_eq!(status, 404);
    }

    #[tokio::test]
    async fn cancel_invalid_uuid_returns_400() {
        let state = make_state(make_store());
        let app = server_router(Arc::clone(&state));

        let (status, _) = post(app, "/v1/executions/not-a-uuid/cancel").await;
        assert_eq!(status, 400);
    }

    #[tokio::test]
    async fn cancel_completed_execution_returns_409() {
        let store = make_store();
        let exec_id = seed_execution(&store, ExecutionState::Claimed, Some("r1"));
        store
            .complete_execution(
                exec_id,
                ExecutionState::Completed,
                Some(1234),
                None,
                None,
                Utc::now(),
            )
            .unwrap();
        let state = make_state(store);
        let app = server_router(Arc::clone(&state));

        let (status, _) = post(app, &format!("/v1/executions/{exec_id}/cancel")).await;
        assert_eq!(status, 409);
    }
}

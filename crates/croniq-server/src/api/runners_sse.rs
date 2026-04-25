//! SSE stream for runner presence updates.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    Extension,
    extract::State,
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
};
use chrono::Utc;
use croniq_auth::CallerContext;
use croniq_auth::context::Scope;
use futures_core::Stream;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::IntervalStream;

use super::ServerState;
use crate::api::auth_middleware::require_scope;

/// `GET /v1/runners/stream` — SSE stream emitting runner presence snapshots.
///
/// Emits a JSON snapshot every 5 seconds with all runners and their status.
pub async fn handle_runner_stream(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, StatusCode> {
    require_scope(&ctx, Scope::RUNNERS_READ)?;
    let stream = IntervalStream::new(tokio::time::interval(Duration::from_secs(5)))
        .map(move |_| {
            let state = Arc::clone(&state);
            async move {
                let now = Utc::now();
                let reg = state.runner.registry.read().await;
                let runners: Vec<serde_json::Value> = reg
                    .all()
                    .map(|r| {
                        serde_json::json!({
                            "runner_id": r.runner_id,
                            "status": format!("{:?}", r.status_at(now)),
                            "capabilities": r.capabilities,
                            "inflight": r.inflight.len(),
                            "max_inflight": r.max_inflight,
                            "last_poll_at": r.last_poll_at.to_rfc3339(),
                        })
                    })
                    .collect();

                let data = serde_json::to_string(&runners).unwrap_or_default();
                Ok(Event::default().event("runners").data(data))
            }
        })
        .then(|fut| fut);

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

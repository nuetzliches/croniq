//! Prometheus-compatible metrics endpoint.
//!
//! Exposes key runtime metrics in Prometheus text exposition format at a
//! configurable endpoint (default: `/metrics`).

use std::sync::Arc;

use axum::{
    Router,
    extract::State,
    http::{StatusCode, header},
    response::IntoResponse,
    routing::get,
};

use crate::api::ServerState;
use croniq_runner::RunnerStatus;

/// Create a router for the metrics endpoint.
pub fn metrics_router(state: Arc<ServerState>) -> Router {
    Router::new()
        .route("/metrics", get(handle_metrics))
        .with_state(state)
}

async fn handle_metrics(
    State(state): State<Arc<ServerState>>,
) -> impl IntoResponse {
    use std::sync::atomic::Ordering;

    let now = chrono::Utc::now();
    let reg = state.runner.registry.read().await;
    let queue = state.runner.queue.read().await;

    let runners_online = reg.by_status(RunnerStatus::Online, now).len();
    let runners_stale = reg.by_status(RunnerStatus::Stale, now).len();
    let runners_dead = reg.by_status(RunnerStatus::Dead, now).len();
    let queue_depth = queue.len();

    let reload_success = state.reload_counters.success.load(Ordering::Relaxed);
    let reload_validation_err = state.reload_counters.validation_error.load(Ordering::Relaxed);
    let reload_apply_err = state.reload_counters.apply_error.load(Ordering::Relaxed);

    let body = format!(
        "# HELP croniq_runners_total Number of known runners by status.\n\
         # TYPE croniq_runners_total gauge\n\
         croniq_runners_total{{status=\"online\"}} {runners_online}\n\
         croniq_runners_total{{status=\"stale\"}} {runners_stale}\n\
         croniq_runners_total{{status=\"dead\"}} {runners_dead}\n\
         # HELP croniq_queue_depth Number of work items in the queue.\n\
         # TYPE croniq_queue_depth gauge\n\
         croniq_queue_depth {queue_depth}\n\
         # HELP croniq_config_reload_total Config reload attempts by outcome.\n\
         # TYPE croniq_config_reload_total counter\n\
         croniq_config_reload_total{{result=\"success\"}} {reload_success}\n\
         croniq_config_reload_total{{result=\"validation_error\"}} {reload_validation_err}\n\
         croniq_config_reload_total{{result=\"apply_error\"}} {reload_apply_err}\n"
    );

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")],
        body,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use croniq_runner::AppState;
    use http_body_util::BodyExt;
    use tokio::sync::mpsc;
    use tower::util::ServiceExt;

    #[tokio::test]
    async fn metrics_returns_prometheus_format() {
        let runner = AppState::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        let state = ServerState::new(runner, tx);
        let app = metrics_router(Arc::clone(&state));

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp.headers().get("content-type").unwrap().to_str().unwrap();
        assert!(ct.contains("text/plain"));

        let body = String::from_utf8(
            resp.into_body().collect().await.unwrap().to_bytes().to_vec(),
        )
        .unwrap();

        assert!(body.contains("croniq_runners_total"));
        assert!(body.contains("croniq_queue_depth"));
    }
}

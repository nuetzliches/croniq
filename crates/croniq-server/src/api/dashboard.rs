//! Dashboard forecast endpoint.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};
use chrono::Utc;
use serde::Deserialize;

use super::ServerState;
use crate::dashboard::compute_forecast;

#[derive(Deserialize)]
pub struct ForecastQuery {
    #[serde(default = "default_window")]
    pub window_minutes: u32,
    #[serde(default = "default_bucket")]
    pub bucket_minutes: u32,
}

fn default_window() -> u32 {
    60
}
fn default_bucket() -> u32 {
    5
}

/// `GET /v1/dashboard/forecast`
pub async fn handle_forecast(
    State(state): State<Arc<ServerState>>,
    Query(q): Query<ForecastQuery>,
) -> Result<Json<crate::dashboard::ForecastResponse>, StatusCode> {
    let triggers = state
        .triggers
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let triggers = triggers.read().await;
    let result = compute_forecast(&triggers, Utc::now(), q.window_minutes, q.bucket_minutes);
    Ok(Json(result))
}

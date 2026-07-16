//! Global maintenance switch endpoints.
//!
//! `GET /v1/maintenance` returns the current state to any authenticated caller
//! (the UI banner polls it). `PUT /v1/maintenance` sets it and is admin-only.
//! The switch itself is defined by [`croniq_store::models::MaintenanceState`];
//! the dispatch gates in the scheduler tick and the work-poll read the cached
//! copy on [`ServerState`].

use std::sync::Arc;

use axum::{Json, extract::State, http::StatusCode};
use chrono::{DateTime, Utc};
use croniq_auth::CallerContext;
use croniq_auth::context::Scope;
use croniq_store::models::MaintenanceState;
use serde::{Deserialize, Serialize};

use super::ServerState;
use crate::api::auth_middleware::require_scope;

/// Public shape of the maintenance switch. Carries the raw fields plus the
/// `active` flag computed server-side, so clients don't re-derive the
/// manual-OR-window logic (and stay consistent with the dispatch gates).
#[derive(Debug, Serialize)]
pub struct MaintenanceResponse {
    /// Effective right now: manual toggle on, or inside the scheduled window.
    pub active: bool,
    pub manual_active: bool,
    pub window_start: Option<DateTime<Utc>>,
    pub window_end: Option<DateTime<Utc>>,
    pub note: Option<String>,
    pub updated_by: Option<String>,
    pub updated_at: Option<DateTime<Utc>>,
}

impl MaintenanceResponse {
    fn from_state(s: &MaintenanceState, now: DateTime<Utc>) -> Self {
        Self {
            active: s.is_active(now),
            manual_active: s.manual_active,
            window_start: s.window_start,
            window_end: s.window_end,
            note: s.note.clone(),
            updated_by: s.updated_by.clone(),
            updated_at: s.updated_at,
        }
    }
}

/// Body of `PUT /v1/maintenance`. A full replacement of the switch state —
/// the client sends the desired end state, not a partial patch.
#[derive(Debug, Deserialize)]
pub struct SetMaintenanceRequest {
    /// Manual toggle: pause now until turned off.
    #[serde(default)]
    pub manual_active: bool,
    /// Optional scheduled-window bounds (RFC3339). Either may be null.
    #[serde(default)]
    pub window_start: Option<DateTime<Utc>>,
    #[serde(default)]
    pub window_end: Option<DateTime<Utc>>,
    /// Optional operator message shown in the UI banner.
    #[serde(default)]
    pub note: Option<String>,
}

/// `GET /v1/maintenance` — current switch state. Any authenticated caller; the
/// UI banner polls this so every user sees an active window.
pub async fn handle_get_maintenance(
    State(state): State<Arc<ServerState>>,
) -> Result<Json<MaintenanceResponse>, StatusCode> {
    let now = Utc::now();
    let snapshot = state
        .maintenance
        .read()
        .map(|m| m.clone())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(MaintenanceResponse::from_state(&snapshot, now)))
}

/// `PUT /v1/maintenance` — set the switch (admin only). Persists to the store,
/// updates the in-memory cache the dispatch gates read, and pings the runner
/// work-notify so a freeze/resume takes effect without waiting for the next
/// long-poll timeout.
pub async fn handle_set_maintenance(
    State(state): State<Arc<ServerState>>,
    axum::Extension(ctx): axum::Extension<CallerContext>,
    Json(req): Json<SetMaintenanceRequest>,
) -> Result<Json<MaintenanceResponse>, StatusCode> {
    require_scope(&ctx, Scope::ADMIN)?;

    let Some(store) = state.store.clone() else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };

    let now = Utc::now();
    let new_state = MaintenanceState {
        manual_active: req.manual_active,
        window_start: req.window_start,
        window_end: req.window_end,
        note: req
            .note
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty()),
        updated_by: Some(ctx.user_id.clone().unwrap_or_else(|| ctx.caller_id.clone())),
        updated_at: Some(now),
    };

    store
        .set_maintenance(&new_state)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if let Ok(mut guard) = state.maintenance.write() {
        *guard = new_state.clone();
    }

    // Wake long-polling runners so a resume hands out queued work immediately
    // and a fresh freeze is observed on their next loop iteration.
    state.runner.work_notify.notify_waiters();

    Ok(Json(MaintenanceResponse::from_state(&new_state, now)))
}

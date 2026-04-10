//! Calendars CRUD endpoints.

use std::sync::Arc;

use axum::{Json, extract::State, http::StatusCode};
use chrono::Utc;
use croniq_store::models::CalendarDefinition;
use serde::Deserialize;
use uuid::Uuid;

use super::ServerState;

#[derive(Deserialize)]
pub struct CreateCalendarRequest {
    pub name: String,
    pub timezone: Option<String>,
    /// JSON-encoded rules array.
    pub rules: String,
}

/// `GET /v1/calendars`
pub async fn handle_list(
    State(state): State<Arc<ServerState>>,
) -> Result<Json<Vec<CalendarDefinition>>, StatusCode> {
    let store = state.store.as_ref().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let cals = store.list_calendars().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(cals))
}

/// `GET /v1/calendars/{id}`
pub async fn handle_get(
    State(state): State<Arc<ServerState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<CalendarDefinition>, StatusCode> {
    let store = state.store.as_ref().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    store.get_calendar(&id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

/// `POST /v1/calendars`
pub async fn handle_create(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<CreateCalendarRequest>,
) -> Result<(StatusCode, Json<CalendarDefinition>), StatusCode> {
    let store = state.store.as_ref().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let now = Utc::now();
    let cal = CalendarDefinition {
        calendar_id: Uuid::new_v4().to_string(),
        name: req.name,
        timezone: req.timezone,
        rules: req.rules,
        created_at: now,
        updated_at: now,
    };
    store.create_calendar(&cal).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((StatusCode::CREATED, Json(cal)))
}

/// `DELETE /v1/calendars/{id}`
pub async fn handle_delete(
    State(state): State<Arc<ServerState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> StatusCode {
    let Some(store) = state.store.as_ref() else { return StatusCode::SERVICE_UNAVAILABLE };
    match store.delete_calendar(&id) {
        Ok(_) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

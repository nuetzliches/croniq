//! Calendars CRUD endpoints.

use std::sync::Arc;

use axum::{Json, extract::State, http::StatusCode};
use chrono::Utc;
use croniq_config::parser::Parser;
use croniq_store::models::CalendarDefinition;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::ServerState;

#[derive(Deserialize)]
pub struct CreateCalendarRequest {
    pub name: String,
    pub timezone: Option<String>,
    /// Calendar rules in Croniqfile DSL syntax (lines of `include`/`exclude`/`timezone`).
    pub rules: String,
}

#[derive(Serialize)]
pub struct ValidationError {
    pub error: &'static str,
    pub message: String,
}

/// Validate free-form calendar rules by wrapping them in a dummy calendar
/// block and running the Croniqfile parser. Returns a human-readable error
/// message on failure.
fn validate_rules(rules: &str) -> Result<(), String> {
    if rules.trim().is_empty() {
        return Ok(());
    }
    let source = format!("calendar \"__validate__\" {{\n{rules}\n}}\n");
    Parser::parse(&source).map(|_| ()).map_err(|e| e.to_string())
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
) -> Result<(StatusCode, Json<CalendarDefinition>), (StatusCode, Json<ValidationError>)> {
    if let Err(message) = validate_rules(&req.rules) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ValidationError { error: "invalid_rules", message }),
        ));
    }
    let store = state.store.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ValidationError { error: "no_store", message: "store unavailable".into() }),
    ))?;
    let now = Utc::now();
    let cal = CalendarDefinition {
        calendar_id: Uuid::new_v4().to_string(),
        name: req.name,
        timezone: req.timezone,
        rules: req.rules,
        created_at: now,
        updated_at: now,
    };
    store.create_calendar(&cal).map_err(|_| (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ValidationError { error: "store_error", message: "failed to persist calendar".into() }),
    ))?;
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

//! Dead letter management endpoints.

use std::sync::Arc;

use axum::{Json, extract::{Query, State}, http::StatusCode};
use croniq_store::models::{DeadLetter, DeadLetterFilter};
use croniq_store::traits::DeadLetterStore;
use serde::Deserialize;
use uuid::Uuid;

use super::ServerState;

#[derive(Deserialize, Default)]
pub struct ListQuery {
    pub job_key: Option<String>,
    pub limit: Option<u32>,
}

/// `GET /v1/dead-letters`
pub async fn handle_list(
    State(state): State<Arc<ServerState>>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<DeadLetter>>, StatusCode> {
    let store = state.store.as_ref().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let filter = DeadLetterFilter {
        job_key: q.job_key,
        limit: Some(q.limit.unwrap_or(50)),
    };
    let letters = store.list_dead_letters(&filter).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(letters))
}

/// `GET /v1/dead-letters/{id}`
pub async fn handle_get(
    State(state): State<Arc<ServerState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<DeadLetter>, StatusCode> {
    let store = state.store.as_ref().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    store.get_dead_letter(uuid)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

/// `DELETE /v1/dead-letters/{id}`
pub async fn handle_delete(
    State(state): State<Arc<ServerState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> StatusCode {
    let Some(store) = state.store.as_ref() else { return StatusCode::SERVICE_UNAVAILABLE };
    let Ok(uuid) = Uuid::parse_str(&id) else { return StatusCode::BAD_REQUEST };
    match store.remove_dead_letter(uuid) {
        Ok(_) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

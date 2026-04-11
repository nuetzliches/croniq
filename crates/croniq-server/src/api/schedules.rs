//! Schedules (trigger definitions) CRUD endpoints.

use std::sync::Arc;

use axum::{Json, extract::{Query, State}, http::StatusCode};
use chrono::Utc;
use croniq_store::models::TriggerDefinition;
use serde::Deserialize;
use uuid::Uuid;

use super::ServerState;

#[derive(Deserialize, Default)]
pub struct ListQuery {
    pub job_key: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateTriggerRequest {
    pub job_key: String,
    pub cron_expression: Option<String>,
    pub timezone: Option<String>,
    pub calendar: Option<String>,
    pub window: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool { true }

/// `GET /v1/schedules`
pub async fn handle_list(
    State(state): State<Arc<ServerState>>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<TriggerDefinition>>, StatusCode> {
    let store = state.store.as_ref().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let triggers = store.list_triggers(q.job_key.as_deref()).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(triggers))
}

/// `GET /v1/schedules/{trigger_id}`
pub async fn handle_get(
    State(state): State<Arc<ServerState>>,
    axum::extract::Path(trigger_id): axum::extract::Path<String>,
) -> Result<Json<TriggerDefinition>, StatusCode> {
    let store = state.store.as_ref().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    store.get_trigger(&trigger_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

/// `POST /v1/schedules`
pub async fn handle_create(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<CreateTriggerRequest>,
) -> Result<(StatusCode, Json<TriggerDefinition>), StatusCode> {
    let store = state.store.as_ref().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let now = Utc::now();
    let trigger = TriggerDefinition {
        trigger_id: Uuid::new_v4().to_string(),
        job_key: req.job_key,
        cron_expression: req.cron_expression,
        timezone: req.timezone,
        calendar: req.calendar,
        window: req.window,
        not_before: None,
        not_after: None,
        enabled: req.enabled,
        managed_by: "api".into(),
        created_at: now,
        updated_at: now,
    };
    store.create_trigger(&trigger).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Push to live scheduler if possible
    if let Some(ref tx) = state.scheduler_tx
        && let Some(rt_trigger) = crate::loader::trigger_from_definition(&trigger, now) {
            let job_config = crate::loader::job_config_from_definition(&trigger, None);
            let _ = tx.send(crate::scheduler::SchedulerCommand::AddJob {
                job: Box::new(job_config),
                trigger: Box::new(rt_trigger),
            });
        }

    Ok((StatusCode::CREATED, Json(trigger)))
}

/// `DELETE /v1/schedules/{trigger_id}`
pub async fn handle_delete(
    State(state): State<Arc<ServerState>>,
    axum::extract::Path(trigger_id): axum::extract::Path<String>,
) -> StatusCode {
    let Some(store) = state.store.as_ref() else { return StatusCode::SERVICE_UNAVAILABLE };
    match store.delete_trigger(&trigger_id) {
        Ok(_) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

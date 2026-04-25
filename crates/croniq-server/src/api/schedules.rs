//! Schedules (trigger definitions) CRUD endpoints.
//!
//! DSL-defined triggers are synthesized from `state.dsl_jobs` with a synthetic
//! ID (`dsl:{job_key}`) and the `managed_by: "dsl"` marker. Mutating these is
//! refused since the Croniqfile owns them.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};
use chrono::Utc;
use croniq_store::models::TriggerDefinition;
use serde::Deserialize;
use uuid::Uuid;

use super::ServerState;
use crate::loader::synth_trigger_def_from_dsl;

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

fn default_true() -> bool {
    true
}

/// `GET /v1/schedules`
pub async fn handle_list(
    State(state): State<Arc<ServerState>>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<TriggerDefinition>>, StatusCode> {
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let mut triggers = store
        .list_triggers(q.job_key.as_deref())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if let Some(dsl) = state.dsl_jobs.as_ref() {
        let guard = dsl.read().await;
        let now = Utc::now();
        for cfg in guard.iter() {
            if let Some(ref filter) = q.job_key
                && cfg.key != *filter
            {
                continue;
            }
            // DSL is authoritative for its job_key — don't emit a stored row
            // and a DSL row for the same job.
            triggers.retain(|t| t.job_key != cfg.key);
            triggers.push(synth_trigger_def_from_dsl(cfg, now));
        }
    }

    Ok(Json(triggers))
}

/// `GET /v1/schedules/{trigger_id}`
pub async fn handle_get(
    State(state): State<Arc<ServerState>>,
    axum::extract::Path(trigger_id): axum::extract::Path<String>,
) -> Result<Json<TriggerDefinition>, StatusCode> {
    if let Some(job_key) = trigger_id.strip_prefix("dsl:")
        && let Some(dsl) = state.dsl_jobs.as_ref()
    {
        let guard = dsl.read().await;
        if let Some(cfg) = guard.iter().find(|j| j.key == job_key) {
            return Ok(Json(synth_trigger_def_from_dsl(cfg, Utc::now())));
        }
        return Err(StatusCode::NOT_FOUND);
    }

    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    store
        .get_trigger(&trigger_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

/// `POST /v1/schedules`
pub async fn handle_create(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<CreateTriggerRequest>,
) -> Result<(StatusCode, Json<TriggerDefinition>), StatusCode> {
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    // Refuse to shadow a DSL trigger — the Croniqfile owns scheduling for this job.
    if let Some(dsl) = state.dsl_jobs.as_ref()
        && dsl.read().await.iter().any(|j| j.key == req.job_key)
    {
        return Err(StatusCode::CONFLICT);
    }

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
    store
        .create_trigger(&trigger)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Push to live scheduler if possible
    if let Some(ref tx) = state.scheduler_tx
        && let Some(rt_trigger) = crate::loader::trigger_from_definition(&trigger, now)
    {
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
    // DSL triggers carry a synthetic prefix — refuse to delete them.
    if trigger_id.starts_with("dsl:") {
        return StatusCode::CONFLICT;
    }

    let Some(store) = state.store.as_ref() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    match store.delete_trigger(&trigger_id) {
        Ok(_) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{ServerState, server_router};
    use crate::store::{DynStore, sqlite_store};
    use axum::body::Body;
    use axum::http::Request;
    use croniq_config::compile::JobConfig;
    use croniq_runner::AppState;
    use croniq_store::sqlite::SqliteStore;
    use http_body_util::BodyExt;
    use tokio::sync::{RwLock, mpsc};
    use tower::util::ServiceExt;

    fn make_store() -> DynStore {
        sqlite_store(SqliteStore::in_memory().unwrap())
    }

    fn dsl_job(key: &str) -> JobConfig {
        crate::loader::load_str(&format!("job {key} {{ every 5 minutes }}"))
            .unwrap()
            .runtime
            .jobs
            .pop()
            .unwrap()
    }

    fn make_state(dsl: Vec<JobConfig>, store: DynStore) -> Arc<ServerState> {
        let runner = AppState::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut state = ServerState::with_auth(runner, tx, None, Some(store));
        {
            let s = Arc::get_mut(&mut state).unwrap();
            s.dsl_jobs = Some(Arc::new(RwLock::new(dsl)));
        }
        state
    }

    async fn body_json(app: axum::Router, method: &str, uri: &str) -> (u16, serde_json::Value) {
        let resp = app
            .oneshot(
                Request::builder()
                    .method(method)
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

    async fn status_of(app: axum::Router, method: &str, uri: &str) -> u16 {
        app.oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
        .as_u16()
    }

    #[tokio::test]
    async fn list_schedules_unions_dsl_with_store() {
        let store = make_store();
        store
            .create_trigger(&TriggerDefinition {
                trigger_id: Uuid::new_v4().to_string(),
                job_key: "api:job".into(),
                cron_expression: Some("30s".into()),
                timezone: None,
                calendar: None,
                window: None,
                not_before: None,
                not_after: None,
                enabled: true,
                managed_by: "api".into(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            })
            .unwrap();

        let state = make_state(vec![dsl_job("dsl:only")], store);
        let (status, body) = body_json(server_router(state), "GET", "/v1/schedules").await;

        assert_eq!(status, 200);
        let arr = body.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        let dsl_row = arr
            .iter()
            .find(|t| t["job_key"] == "dsl:only")
            .expect("dsl row");
        assert_eq!(dsl_row["managed_by"], "dsl");
        assert_eq!(dsl_row["trigger_id"], "dsl:dsl:only");
    }

    #[tokio::test]
    async fn get_dsl_schedule_by_synthetic_id() {
        let state = make_state(vec![dsl_job("dsl:one")], make_store());
        let (status, body) =
            body_json(server_router(state), "GET", "/v1/schedules/dsl:dsl:one").await;
        assert_eq!(status, 200);
        assert_eq!(body["managed_by"], "dsl");
        assert_eq!(body["job_key"], "dsl:one");
    }

    #[tokio::test]
    async fn delete_dsl_schedule_returns_409() {
        let state = make_state(vec![dsl_job("dsl:one")], make_store());
        let status = status_of(server_router(state), "DELETE", "/v1/schedules/dsl:dsl:one").await;
        assert_eq!(status, 409);
    }
}

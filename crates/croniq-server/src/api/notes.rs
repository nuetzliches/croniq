//! Operator notes on jobs and their runs: "seen and checked", questions,
//! ideas. A note always names its job and may also name one run; it outlives
//! that run when retention deletes it (migration 031).
//!
//! Reading needs `executions:read`, writing `notes:write`. A note can be
//! deleted by whoever wrote it, or by an admin.

use std::collections::HashMap;
use std::sync::Arc;

use axum::response::{IntoResponse, Response};
use axum::{Extension, Json, extract::State, http::StatusCode};
use chrono::Utc;
use croniq_auth::CallerContext;
use croniq_auth::context::Scope;
use croniq_store::models::{JobNote, NoteFilter, NoteKind};
use croniq_store::traits::StoreError;
use serde::Deserialize;
use uuid::Uuid;

use super::ServerState;
use crate::api::audit;
use crate::api::auth_middleware::require_scope;
use crate::api::calendars::ValidationError;
use crate::store::DynStore;

/// Longest note body accepted, in characters. Notes are a sentence or a
/// paragraph; a pasted log belongs in the run's log, not here.
const MAX_BODY_CHARS: usize = 4000;
/// Most run ids one list request may name — a page of the Runs screen.
const MAX_EXECUTION_IDS: usize = 200;
const DEFAULT_LIMIT: u32 = 100;
const MAX_LIMIT: u32 = 500;

/// A refusal: a bare status, or a `400` that says what was wrong so the
/// dashboard can show the server's own wording.
#[derive(Debug)]
pub enum NoteError {
    Status(StatusCode),
    Invalid(String),
}

impl From<StatusCode> for NoteError {
    fn from(status: StatusCode) -> Self {
        Self::Status(status)
    }
}

impl IntoResponse for NoteError {
    fn into_response(self) -> Response {
        match self {
            Self::Status(status) => status.into_response(),
            Self::Invalid(message) => (
                StatusCode::BAD_REQUEST,
                Json(ValidationError {
                    error: "invalid_note",
                    message,
                }),
            )
                .into_response(),
        }
    }
}

fn bad_request(message: impl Into<String>) -> NoteError {
    NoteError::Invalid(message.into())
}

fn store_of(state: &ServerState) -> Result<&DynStore, NoteError> {
    Ok(state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?)
}

fn internal<E>(_: E) -> NoteError {
    NoteError::Status(StatusCode::INTERNAL_SERVER_ERROR)
}

/// `GET /v1/notes?job_key=…&execution_ids=a,b&limit=…`
///
/// Newest first. `job_key` and `execution_ids` combine with AND; at least one
/// of them is required, so the endpoint never answers "every note there is".
pub async fn handle_list(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Result<Json<Vec<JobNote>>, NoteError> {
    require_scope(&ctx, Scope::EXECUTIONS_READ)?;
    let store = store_of(&state)?;

    let job_key = params
        .get("job_key")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let mut execution_ids = Vec::new();
    for raw in params
        .get("execution_ids")
        .map(String::as_str)
        .unwrap_or("")
        .split(',')
    {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let id = Uuid::parse_str(raw)
            .map_err(|_| bad_request(format!("execution_ids: '{raw}' is not a run id")))?;
        execution_ids.push(id);
    }
    if execution_ids.len() > MAX_EXECUTION_IDS {
        return Err(bad_request(format!(
            "execution_ids: at most {MAX_EXECUTION_IDS} per request"
        )));
    }
    if job_key.is_none() && execution_ids.is_empty() {
        return Err(bad_request("name a job_key, execution_ids, or both"));
    }
    let limit = match params.get("limit") {
        Some(raw) => raw
            .parse::<u32>()
            .map_err(|_| bad_request("limit must be a positive number"))?
            .clamp(1, MAX_LIMIT),
        None => DEFAULT_LIMIT,
    };

    let notes = store
        .list_notes(&NoteFilter {
            job_key,
            execution_ids,
            limit: Some(limit),
        })
        .map_err(internal)?;
    Ok(Json(notes))
}

#[derive(Debug, Deserialize)]
pub struct CreateNoteRequest {
    pub job_key: String,
    #[serde(default)]
    pub execution_id: Option<Uuid>,
    pub kind: String,
    #[serde(default)]
    pub body: String,
}

/// `POST /v1/notes`
///
/// An `ack` may have an empty body — the mark itself is the message. Every
/// other kind needs text. With `execution_id` the run must exist and belong
/// to `job_key`; without it the job must exist.
pub async fn handle_create(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Json(req): Json<CreateNoteRequest>,
) -> Result<(StatusCode, Json<JobNote>), NoteError> {
    require_scope(&ctx, Scope::NOTES_WRITE)?;
    let store = store_of(&state)?;

    let kind = NoteKind::parse(req.kind.trim()).ok_or_else(|| {
        bad_request(format!(
            "kind '{}' is not one of: {}",
            req.kind,
            NoteKind::allowed_values()
        ))
    })?;
    let body = req.body.trim().to_string();
    if body.is_empty() && kind != NoteKind::Ack {
        return Err(bad_request(format!("a {} needs some text", kind.as_str())));
    }
    if body.chars().count() > MAX_BODY_CHARS {
        return Err(bad_request(format!(
            "a note is at most {MAX_BODY_CHARS} characters"
        )));
    }
    let job_key = req.job_key.trim().to_string();
    if job_key.is_empty() {
        return Err(bad_request("job_key is required"));
    }

    match req.execution_id {
        Some(execution_id) => {
            let execution = store
                .get_execution(execution_id)
                .map_err(internal)?
                .ok_or(StatusCode::NOT_FOUND)?;
            if execution.job_key != job_key {
                return Err(bad_request(format!(
                    "run {execution_id} belongs to job '{}', not '{job_key}'",
                    execution.job_key
                )));
            }
        }
        None => {
            if !job_exists(&state, store, &job_key)
                .await
                .map_err(internal)?
            {
                return Err(StatusCode::NOT_FOUND.into());
            }
        }
    }

    let note = JobNote {
        id: Uuid::new_v4(),
        job_key,
        execution_id: req.execution_id,
        kind,
        body,
        author_id: ctx.user_id.clone().unwrap_or_else(|| ctx.caller_id.clone()),
        author_name: author_name(store, &ctx),
        created_at: Utc::now(),
    };
    store.create_note(&note).map_err(internal)?;
    audit::record(
        store,
        &ctx,
        "note.created",
        audit_target(&note),
        Some(&note.id.to_string()),
        None,
    );
    Ok((StatusCode::CREATED, Json(note)))
}

/// `DELETE /v1/notes/{id}` — the author or an admin.
pub async fn handle_delete(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<StatusCode, NoteError> {
    require_scope(&ctx, Scope::NOTES_WRITE)?;
    let store = store_of(&state)?;
    let id = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;

    let note = store
        .get_note(id)
        .map_err(internal)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let caller = ctx.user_id.as_deref().unwrap_or(&ctx.caller_id);
    if note.author_id != caller && !ctx.is_admin() {
        return Err(StatusCode::FORBIDDEN.into());
    }

    store.delete_note(id).map_err(internal)?;
    audit::record(
        store,
        &ctx,
        "note.deleted",
        audit_target(&note),
        Some(&id.to_string()),
        None,
    );
    Ok(StatusCode::NO_CONTENT)
}

fn audit_target(note: &JobNote) -> &'static str {
    if note.execution_id.is_some() {
        "execution"
    } else {
        "job"
    }
}

/// Whether `job_key` names a job the API knows: a stored definition or one
/// the Croniqfile declares. Also used by the favorites endpoints.
pub(crate) async fn job_exists(
    state: &ServerState,
    store: &DynStore,
    job_key: &str,
) -> Result<bool, StoreError> {
    if store.get_job_definition(job_key)?.is_some() {
        return Ok(true);
    }
    if let Some(dsl) = state.dsl_jobs.as_ref() {
        return Ok(dsl.read().await.iter().any(|j| j.key == job_key));
    }
    Ok(false)
}

/// The name shown next to a note: the user's display name, else their
/// username, else — for an API key, which has no user row — its client id.
fn author_name(store: &DynStore, ctx: &CallerContext) -> String {
    if let Some(user_id) = ctx.user_id.as_deref()
        && let Ok(Some(user)) = store.users_get_by_id(user_id)
    {
        return user
            .display_name
            .filter(|n| !n.trim().is_empty())
            .unwrap_or(user.username);
    }
    ctx.client_id.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{ServerState, server_router};
    use crate::store::sqlite_store;
    use axum::body::Body;
    use axum::http::Request;
    use croniq_auth::jwt::issue_token_pair;
    use croniq_auth::{AuthMethod, CallerType};
    use croniq_runner::AppState;
    use croniq_store::models::{Execution, ExecutionState, JobDefinition};
    use croniq_store::sqlite::SqliteStore;
    use http_body_util::BodyExt;
    use tokio::sync::mpsc;
    use tower::util::ServiceExt;

    fn make_state(store: DynStore) -> Arc<ServerState> {
        let runner = AppState::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        ServerState::with_auth(runner, tx, Some(crate::api::test_auth::jwt()), Some(store))
    }

    /// An API-client token with `scopes`, so each test caller has its own
    /// `caller_id` without seeding a user row.
    fn bearer(caller_id: &str, scopes: &[&str]) -> String {
        let scopes: Vec<String> = scopes.iter().map(|s| s.to_string()).collect();
        let pair = issue_token_pair(
            &crate::api::test_auth::jwt(),
            caller_id,
            caller_id,
            CallerType::ApiKey,
            None,
            None,
            AuthMethod::ApiKey,
            &scopes,
            None,
        )
        .unwrap();
        format!("Bearer {}", pair.access_token)
    }

    fn operator(caller_id: &str) -> String {
        bearer(caller_id, &[Scope::EXECUTIONS_READ, Scope::NOTES_WRITE])
    }

    fn viewer() -> String {
        bearer("viewer-client", &[Scope::EXECUTIONS_READ])
    }

    fn seed_job(store: &DynStore, job_key: &str) {
        let def: JobDefinition = serde_json::from_value(serde_json::json!({
            "job_key": job_key,
            "description": null,
            "assigned_runner_id": null,
            "is_active": true,
            "metadata": {},
            "created_at": Utc::now(),
            "updated_at": Utc::now(),
            "timeout": null,
            "max_retries": null,
            "dead_letter_enabled": null,
        }))
        .unwrap();
        store.create_job_definition(&def).unwrap();
    }

    fn seed_failed_run(store: &DynStore, job_key: &str) -> Uuid {
        let id = Uuid::new_v4();
        let now = Utc::now();
        store
            .create_execution(&Execution {
                id,
                job_key: job_key.into(),
                fire_at: now,
                scheduled_for: now,
                attempt: 1,
                state: ExecutionState::Queued,
                runner_id: None,
                claimed_at: None,
                started_at: None,
                completed_at: None,
                duration_ms: None,
                error: None,
                dead_reason: None,
                idempotency_key: None,
                metadata: HashMap::new(),
                created_at: now,
            })
            .unwrap();
        id
    }

    async fn call(
        app: &axum::Router,
        auth: &str,
        method: &str,
        uri: &str,
        body: Option<serde_json::Value>,
    ) -> (u16, serde_json::Value) {
        let mut req = Request::builder()
            .header("authorization", auth)
            .method(method)
            .uri(uri);
        let body = match body {
            Some(json) => {
                req = req.header("content-type", "application/json");
                Body::from(json.to_string())
            }
            None => Body::empty(),
        };
        let resp = app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
        let status = resp.status().as_u16();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, json)
    }

    fn setup() -> (axum::Router, DynStore, Uuid) {
        let store = sqlite_store(SqliteStore::in_memory().unwrap());
        seed_job(&store, "mail:send");
        let run = seed_failed_run(&store, "mail:send");
        let app = server_router(make_state(store.clone()));
        (app, store, run)
    }

    #[tokio::test]
    async fn an_ack_needs_no_text_and_is_listed_for_its_run() {
        let (app, _store, run) = setup();
        let (status, note) = call(
            &app,
            &operator("op-a"),
            "POST",
            "/v1/notes",
            Some(serde_json::json!({"job_key": "mail:send", "execution_id": run, "kind": "ack"})),
        )
        .await;
        assert_eq!(status, 201, "{note}");
        assert_eq!(note["kind"], "ack");
        assert_eq!(note["body"], "");
        assert_eq!(note["author_name"], "op-a");

        let (status, list) = call(
            &app,
            &viewer(),
            "GET",
            &format!("/v1/notes?execution_ids={run}"),
            None,
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(list.as_array().unwrap().len(), 1);
        assert_eq!(list[0]["execution_id"], run.to_string());

        let (_, by_job) = call(&app, &viewer(), "GET", "/v1/notes?job_key=mail:send", None).await;
        assert_eq!(by_job.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_question_without_text_is_rejected() {
        let (app, _store, run) = setup();
        let (status, body) = call(
            &app,
            &operator("op-a"),
            "POST",
            "/v1/notes",
            Some(serde_json::json!({
                "job_key": "mail:send", "execution_id": run, "kind": "question", "body": "  "
            })),
        )
        .await;
        assert_eq!(status, 400);
        assert_eq!(body["error"], "invalid_note");
    }

    #[tokio::test]
    async fn an_unknown_kind_is_rejected() {
        let (app, _store, _run) = setup();
        let (status, _) = call(
            &app,
            &operator("op-a"),
            "POST",
            "/v1/notes",
            Some(serde_json::json!({"job_key": "mail:send", "kind": "shrug", "body": "x"})),
        )
        .await;
        assert_eq!(status, 400);
    }

    #[tokio::test]
    async fn a_viewer_cannot_write() {
        let (app, _store, _run) = setup();
        let (status, _) = call(
            &app,
            &viewer(),
            "POST",
            "/v1/notes",
            Some(
                serde_json::json!({"job_key": "mail:send", "kind": "idea", "body": "retry later"}),
            ),
        )
        .await;
        assert_eq!(status, 403);
    }

    #[tokio::test]
    async fn a_run_of_another_job_is_rejected() {
        let (app, store, _run) = setup();
        seed_job(&store, "billing:invoice");
        let other_run = seed_failed_run(&store, "billing:invoice");
        let (status, body) = call(
            &app,
            &operator("op-a"),
            "POST",
            "/v1/notes",
            Some(serde_json::json!({
                "job_key": "mail:send", "execution_id": other_run, "kind": "ack"
            })),
        )
        .await;
        assert_eq!(status, 400, "{body}");
    }

    #[tokio::test]
    async fn an_unknown_job_or_run_is_not_found() {
        let (app, _store, _run) = setup();
        let (status, _) = call(
            &app,
            &operator("op-a"),
            "POST",
            "/v1/notes",
            Some(serde_json::json!({"job_key": "nope", "kind": "note", "body": "x"})),
        )
        .await;
        assert_eq!(status, 404);

        let (status, _) = call(
            &app,
            &operator("op-a"),
            "POST",
            "/v1/notes",
            Some(serde_json::json!({
                "job_key": "mail:send", "execution_id": Uuid::new_v4(), "kind": "ack"
            })),
        )
        .await;
        assert_eq!(status, 404);
    }

    #[tokio::test]
    async fn only_the_author_or_an_admin_deletes_a_note() {
        let (app, _store, _run) = setup();
        let create = |author: &'static str| {
            let app = app.clone();
            async move {
                let (status, note) = call(
                    &app,
                    &operator(author),
                    "POST",
                    "/v1/notes",
                    Some(serde_json::json!({
                        "job_key": "mail:send", "kind": "note", "body": "checked the SMTP relay"
                    })),
                )
                .await;
                assert_eq!(status, 201);
                note["id"].as_str().unwrap().to_string()
            }
        };

        let note_a = create("op-a").await;
        let (status, _) = call(
            &app,
            &operator("op-b"),
            "DELETE",
            &format!("/v1/notes/{note_a}"),
            None,
        )
        .await;
        assert_eq!(status, 403, "another operator may not delete it");

        let (status, _) = call(
            &app,
            &operator("op-a"),
            "DELETE",
            &format!("/v1/notes/{note_a}"),
            None,
        )
        .await;
        assert_eq!(status, 204, "the author may");

        let note_b = create("op-b").await;
        let (status, _) = call(
            &app,
            &crate::api::test_auth::admin_bearer(),
            "DELETE",
            &format!("/v1/notes/{note_b}"),
            None,
        )
        .await;
        assert_eq!(status, 204, "an admin may");

        let (status, _) = call(
            &app,
            &operator("op-b"),
            "DELETE",
            &format!("/v1/notes/{note_b}"),
            None,
        )
        .await;
        assert_eq!(status, 404);
    }

    #[tokio::test]
    async fn listing_needs_a_filter() {
        let (app, _store, _run) = setup();
        let (status, _) = call(&app, &viewer(), "GET", "/v1/notes", None).await;
        assert_eq!(status, 400);
        let (status, _) = call(&app, &viewer(), "GET", "/v1/notes?execution_ids=nope", None).await;
        assert_eq!(status, 400);
    }
}

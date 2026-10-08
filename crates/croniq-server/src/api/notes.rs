//! Operator notes on jobs and their runs: "seen and checked", questions,
//! ideas. A note always names its job and may also name one run; it outlives
//! that run when retention deletes it (migration 031).
//!
//! Layout:
//!   GET    /v1/notes                  notes on a job and/or some runs
//!   POST   /v1/notes                  write one
//!   DELETE /v1/notes/{id}             the author, or an admin
//!   GET    /v1/notes/threads          the inbox: every thread, newest first
//!   GET    /v1/users/me/notes-seen    self — how far the inbox has been read
//!   PUT    /v1/users/me/notes-seen    self — move that forward
//!
//! Reading needs `executions:read`, writing `notes:write`. A note can be
//! deleted by whoever wrote it, or by an admin.
//!
//! The inbox groups notes into threads — one run's notes, or a job's own — and
//! tells each reader which they wrote in and which someone else has written in
//! since `seen_at`. That marker is one instant per user (migration 033), kept
//! on the server so it follows them between browsers, and only a user has one:
//! an API key gets `403` on `notes-seen`, as on every `/v1/users/me/*` route.
//! Moving it is the reader's own view state, so — like favorites — it is not
//! audit-logged.

use std::collections::HashMap;
use std::sync::Arc;

use axum::response::{IntoResponse, Response};
use axum::{Extension, Json, extract::State, http::StatusCode};
use chrono::{DateTime, Utc};
use croniq_auth::CallerContext;
use croniq_auth::context::Scope;
use croniq_store::models::{JobNote, NoteFilter, NoteKind, NoteThread, NoteThreadQuery};
use croniq_store::traits::StoreError;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::ServerState;
use crate::api::audit;
use crate::api::auth_middleware::require_scope;
use crate::api::calendars::ValidationError;
use crate::api::users::require_self_user;
use crate::store::DynStore;

/// Longest note body accepted, in characters. Notes are a sentence or a
/// paragraph; a pasted log belongs in the run's log, not here.
const MAX_BODY_CHARS: usize = 4000;
/// Most run ids one list request may name — a page of the Runs screen.
const MAX_EXECUTION_IDS: usize = 200;
const DEFAULT_LIMIT: u32 = 100;
const MAX_LIMIT: u32 = 500;
/// Threads per inbox page, unless the request asks for more ("Show more").
const DEFAULT_THREAD_LIMIT: u32 = 50;
/// The longest inbox page. Paging is by asking again with a larger `limit`
/// rather than by cursor: threads are ordered by their newest note, so a
/// reply moves one to the top, and a cursor would show it twice or skip
/// another.
const MAX_THREAD_LIMIT: u32 = 200;
/// Newest notes carried per thread: enough to summarize a row; the whole
/// thread is one `GET /v1/notes` away, and `note_count` says how many there are.
const NOTES_PER_THREAD: u32 = 10;

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

/// Who the caller is as an author: their user, or — for a credential with no
/// user behind it — its caller id. What a note's `author_id` records, what
/// "mine" compares against, and what decides who may delete.
fn author_of(ctx: &CallerContext) -> &str {
    ctx.user_id.as_deref().unwrap_or(&ctx.caller_id)
}

/// A `limit` parameter, clamped to `1..=max`; `default` when absent.
fn limit_param(raw: Option<&String>, default: u32, max: u32) -> Result<u32, NoteError> {
    match raw {
        Some(raw) => Ok(raw
            .parse::<u32>()
            .map_err(|_| bad_request("limit must be a positive number"))?
            .clamp(1, max)),
        None => Ok(default),
    }
}

/// A yes/no query parameter: `1`/`true` or `0`/`false`/empty/absent.
///
/// Anything else is refused rather than read as "no": a misspelled
/// `unread=yes` would otherwise quietly widen the inbox to every thread.
fn flag(params: &HashMap<String, String>, name: &str) -> Result<bool, NoteError> {
    match params.get(name).map(|v| v.trim()) {
        None | Some("") | Some("0") => Ok(false),
        Some("1") => Ok(true),
        Some(v) if v.eq_ignore_ascii_case("false") => Ok(false),
        Some(v) if v.eq_ignore_ascii_case("true") => Ok(true),
        Some(v) => Err(bad_request(format!("{name} must be 1 or 0, not '{v}'"))),
    }
}

/// The optional `seen_at` of the inbox, as an instant.
///
/// Parsed here rather than handed to the store as text: a caller's
/// `…05.1Z` and a stored `…05.100+00:00` name the same instant and do not
/// compare as strings. A `+` offset arrives as a space unless it was
/// percent-encoded, which is the likeliest way to get this wrong.
fn seen_at_param(params: &HashMap<String, String>) -> Result<Option<DateTime<Utc>>, NoteError> {
    let Some(raw) = params
        .get("seen_at")
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
    else {
        return Ok(None);
    };
    DateTime::parse_from_rfc3339(raw)
        .map(|at| Some(at.with_timezone(&Utc)))
        .map_err(|_| {
            bad_request(format!(
                "seen_at: '{raw}' is not an RFC 3339 time — use a `Z` offset, or percent-encode the `+`"
            ))
        })
}

/// `GET /v1/notes?job_key=…&execution_ids=a,b&limit=…`
///
/// Newest first. `job_key` and `execution_ids` combine with AND; at least one
/// of them is required, so the endpoint never answers "every note there is".
/// The view across every job is `GET /v1/notes/threads`, which pages threads
/// and carries only each thread's newest notes.
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
    let limit = limit_param(params.get("limit"), DEFAULT_LIMIT, MAX_LIMIT)?;

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
        author_id: author_of(&ctx).to_string(),
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
    if note.author_id != author_of(&ctx) && !ctx.is_admin() {
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

/// One page of the inbox.
#[derive(Debug, Serialize)]
pub struct NoteThreadsView {
    pub threads: Vec<NoteThread>,
    /// The newest note there is, whatever the filters: what the reader has
    /// seen up to by opening the inbox, to hand back to `PUT notes-seen`.
    pub latest_note_at: Option<DateTime<Utc>>,
    /// A larger `limit` would list more threads.
    pub has_more: bool,
}

/// `GET /v1/notes/threads?mine=1&unread=1&seen_at=…&limit=…`
///
/// Every thread that carries a note, newest activity first. `mine` keeps the
/// threads the caller has written in; `unread` those with a note by someone
/// else after `seen_at`. Without `seen_at` nothing counts as read — the stored
/// marker is deliberately not substituted, because the dashboard asks with the
/// value it read when the inbox was opened and moves the marker meanwhile.
pub async fn handle_list_threads(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Result<Json<NoteThreadsView>, NoteError> {
    require_scope(&ctx, Scope::EXECUTIONS_READ)?;
    let store = store_of(&state)?;
    let mine_only = flag(&params, "mine")?;
    let unread_only = flag(&params, "unread")?;
    let seen_at = seen_at_param(&params)?;
    let limit = limit_param(params.get("limit"), DEFAULT_THREAD_LIMIT, MAX_THREAD_LIMIT)?;

    // One more than asked, to learn whether there is more without counting.
    let mut page = store
        .list_note_threads(&NoteThreadQuery {
            reader_id: author_of(&ctx).to_string(),
            seen_at,
            mine_only,
            unread_only,
            limit: limit + 1,
            notes_per_thread: NOTES_PER_THREAD,
        })
        .map_err(internal)?;
    let has_more = page.threads.len() > limit as usize;
    page.threads.truncate(limit as usize);
    Ok(Json(NoteThreadsView {
        threads: page.threads,
        latest_note_at: page.latest_note_at,
        has_more,
    }))
}

/// How far a user has read the inbox, and what is new since.
#[derive(Debug, Serialize)]
pub struct NotesSeenView {
    /// The newest note the inbox has shown them; `null` if never opened.
    pub seen_at: Option<DateTime<Utc>>,
    /// Threads with a note by someone else after `seen_at` — the badge.
    pub unread_threads: u64,
}

#[derive(Debug, Deserialize)]
pub struct NotesSeenUpdate {
    pub seen_at: DateTime<Utc>,
}

/// `GET /v1/users/me/notes-seen` — what the navigation badge shows.
pub async fn handle_get_seen(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
) -> Result<Json<NotesSeenView>, NoteError> {
    require_scope(&ctx, Scope::EXECUTIONS_READ)?;
    let user_id = require_self_user(&ctx)?;
    let store = store_of(&state)?;
    let seen_at = store.get_notes_seen(user_id).map_err(internal)?;
    seen_view(store, user_id, seen_at)
}

/// `PUT /v1/users/me/notes-seen` — `{"seen_at": …}`, the `latest_note_at` of
/// an inbox page the user was shown. `200` with the new view, so the badge
/// can update from the answer.
///
/// Forward only: an older value — a second tab, a page loaded earlier —
/// leaves the marker where it is. And never past now, so a clock-skewed or
/// made-up value cannot hide notes not yet written.
pub async fn handle_set_seen(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Json(req): Json<NotesSeenUpdate>,
) -> Result<Json<NotesSeenView>, NoteError> {
    require_scope(&ctx, Scope::EXECUTIONS_READ)?;
    let user_id = require_self_user(&ctx)?;
    let store = store_of(&state)?;
    let seen_at = store
        .advance_notes_seen(user_id, req.seen_at.min(Utc::now()))
        .map_err(internal)?;
    seen_view(store, user_id, Some(seen_at))
}

fn seen_view(
    store: &DynStore,
    user_id: &str,
    seen_at: Option<DateTime<Utc>>,
) -> Result<Json<NotesSeenView>, NoteError> {
    let unread_threads = store
        .count_unread_note_threads(user_id, seen_at)
        .map_err(internal)?;
    Ok(Json(NotesSeenView {
        seen_at,
        unread_threads,
    }))
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
/// username. An API key has no user row, so it is its API client's name, else
/// — no client row, or a blank name — the client id, which `croniq init` and
/// the dashboard mint as a UUID nobody would recognise.
///
/// Stored with the note, so renaming the user or client later does not
/// rewrite what was signed; `author_id` is the identity, this is a label.
fn author_name(store: &DynStore, ctx: &CallerContext) -> String {
    if let Some(user_id) = ctx.user_id.as_deref() {
        if let Ok(Some(user)) = store.users_get_by_id(user_id) {
            return user
                .display_name
                .filter(|n| !n.trim().is_empty())
                .unwrap_or(user.username);
        }
    } else if let Ok(Some(client)) = store.get_client(&ctx.client_id)
        && !client.name.trim().is_empty()
    {
        return client.name;
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
    use croniq_auth::api_key::hash_api_key;
    use croniq_auth::jwt::issue_token_pair;
    use croniq_auth::{AuthMethod, CallerType, Role, default_scopes_for_role};
    use croniq_runner::AppState;
    use croniq_store::models::{ApiClient, ApiKey, Execution, ExecutionState, JobDefinition, User};
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

    // ─── Who signed it ───

    /// An API client named `name`, with a UUID id as `croniq init` mints one,
    /// and a raw key for it. Returns `(client_id, key_id, raw_key)`.
    fn seed_api_client(store: &DynStore, name: &str) -> (String, String, String) {
        let client_id = Uuid::new_v4().to_string();
        let raw_key = format!("croniq_test_{}", Uuid::new_v4().simple());
        let key_id = Uuid::new_v4().to_string();
        let now = Utc::now();
        store
            .create_client(&ApiClient {
                client_id: client_id.clone(),
                name: name.into(),
                scopes: vec![Scope::EXECUTIONS_READ.into(), Scope::NOTES_WRITE.into()],
                is_active: true,
                created_at: now,
                managed_by: "api".into(),
            })
            .unwrap();
        store
            .create_api_key(&ApiKey {
                key_id: key_id.clone(),
                client_id: client_id.clone(),
                key_hash: hash_api_key(&raw_key),
                key_prefix: raw_key.chars().take(12).collect(),
                expires_at: None,
                revoked_at: None,
                created_at: now,
            })
            .unwrap();
        (client_id, key_id, raw_key)
    }

    async fn write_ack(app: &axum::Router, auth: &str) -> serde_json::Value {
        let (status, note) = call(
            app,
            auth,
            "POST",
            "/v1/notes",
            Some(serde_json::json!({"job_key": "mail:send", "kind": "ack"})),
        )
        .await;
        assert_eq!(status, 201, "{note}");
        note
    }

    #[tokio::test]
    async fn an_api_key_signs_with_its_clients_name() {
        let (app, store, _run) = setup();
        let (client_id, key_id, raw_key) = seed_api_client(&store, "nightly-ci");

        let note = write_ack(&app, &format!("ApiKey {raw_key}")).await;
        assert_eq!(note["author_name"], "nightly-ci", "not {client_id}");
        assert_eq!(
            note["author_id"], key_id,
            "the identity behind \"mine\" and delete is still the key"
        );
    }

    #[tokio::test]
    async fn an_api_key_without_a_named_client_signs_with_the_client_id() {
        let (app, store, _run) = setup();

        // No client row at all: a token minted for a client since deleted.
        let note = write_ack(&app, &operator("op-a")).await;
        assert_eq!(note["author_name"], "op-a");

        // A row whose name is blank says no more than its id.
        let (client_id, _key_id, raw_key) = seed_api_client(&store, "  ");
        let note = write_ack(&app, &format!("ApiKey {raw_key}")).await;
        assert_eq!(note["author_name"], client_id);
    }

    // ─── The inbox ───

    fn seed_user(store: &DynStore, user_id: &str, role: Role) {
        let now = Utc::now();
        store
            .users_create(&User {
                user_id: user_id.into(),
                username: user_id.into(),
                email: None,
                display_name: None,
                role,
                is_active: true,
                created_at: now,
                updated_at: now,
                last_login_at: None,
            })
            .unwrap();
    }

    /// A signed-in user's access token, with the scopes their role grants.
    fn user_token(user_id: &str, role: Role) -> String {
        let pair = issue_token_pair(
            &crate::api::test_auth::jwt(),
            user_id,
            user_id,
            CallerType::User,
            Some(user_id),
            Some(role),
            AuthMethod::Password,
            &default_scopes_for_role(role),
            None,
        )
        .unwrap();
        format!("Bearer {}", pair.access_token)
    }

    /// A note written `minutes_ago` — always in the past, so a marker clamped
    /// to now can still reach it.
    fn seed_note(
        store: &DynStore,
        author: &str,
        execution_id: Option<Uuid>,
        minutes_ago: i64,
    ) -> JobNote {
        let note = JobNote {
            id: Uuid::new_v4(),
            job_key: "mail:send".into(),
            execution_id,
            kind: NoteKind::Question,
            body: "why does this fail on Mondays?".into(),
            author_id: author.into(),
            author_name: author.into(),
            created_at: Utc::now() - chrono::Duration::minutes(minutes_ago),
        };
        store.create_note(&note).unwrap();
        note
    }

    #[tokio::test]
    async fn the_inbox_lists_run_and_job_threads_with_their_run() {
        let (app, store, run) = setup();
        seed_note(&store, "op-b", Some(run), 2);
        seed_note(&store, "op-b", None, 1);

        let (status, page) = call(&app, &viewer(), "GET", "/v1/notes/threads", None).await;
        assert_eq!(status, 200, "{page}");
        let threads = page["threads"].as_array().unwrap();
        assert_eq!(threads.len(), 2);

        assert_eq!(threads[0]["job_key"], "mail:send");
        assert!(threads[0]["execution_id"].is_null(), "the job's own thread");
        assert!(threads[0]["execution"].is_null());

        assert_eq!(threads[1]["execution_id"], run.to_string());
        assert_eq!(threads[1]["execution"]["id"], run.to_string());
        assert_eq!(threads[1]["note_count"], 1);
        assert_eq!(threads[1]["notes"][0]["author_name"], "op-b");
        assert_eq!(page["has_more"], false);
    }

    #[tokio::test]
    async fn a_thread_whose_run_was_pruned_has_no_execution() {
        let (app, store, _run) = setup();
        let gone = Uuid::new_v4();
        seed_note(&store, "op-b", Some(gone), 1);

        let (_, page) = call(&app, &viewer(), "GET", "/v1/notes/threads", None).await;
        let thread = &page["threads"][0];
        assert_eq!(thread["execution_id"], gone.to_string());
        assert!(thread["execution"].is_null(), "{thread}");
    }

    #[tokio::test]
    async fn mine_and_unread_are_the_callers_own_view() {
        let (app, store, run) = setup();
        // An API key has no user, but it writes as its caller id — and that is
        // whose "mine" it gets.
        seed_note(&store, "op-a", Some(run), 2);
        seed_note(&store, "op-b", None, 1);

        let runs_in = |page: &serde_json::Value| -> Vec<serde_json::Value> {
            page["threads"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| t["execution_id"].clone())
                .collect()
        };

        let (_, mine_a) = call(
            &app,
            &operator("op-a"),
            "GET",
            "/v1/notes/threads?mine=1",
            None,
        )
        .await;
        assert_eq!(runs_in(&mine_a), vec![serde_json::json!(run)]);
        assert_eq!(mine_a["threads"][0]["mine"], true);
        let (_, mine_b) = call(
            &app,
            &operator("op-b"),
            "GET",
            "/v1/notes/threads?mine=true",
            None,
        )
        .await;
        assert_eq!(runs_in(&mine_b), vec![serde_json::Value::Null]);

        // Nothing read yet: only op-b's thread is new to op-a.
        let (_, unread_a) = call(
            &app,
            &operator("op-a"),
            "GET",
            "/v1/notes/threads?unread=1",
            None,
        )
        .await;
        assert_eq!(runs_in(&unread_a), vec![serde_json::Value::Null]);

        // Read up to the newest note: nothing is new any more.
        let latest = unread_a["latest_note_at"].as_str().unwrap().to_string();
        let (status, after) = call(
            &app,
            &operator("op-a"),
            "GET",
            &format!("/v1/notes/threads?unread=1&seen_at={latest}"),
            None,
        )
        .await;
        assert_eq!(status, 200, "{after}");
        assert!(after["threads"].as_array().unwrap().is_empty(), "{after}");
    }

    #[tokio::test]
    async fn the_inbox_reports_the_newest_note_whatever_the_filter() {
        let (app, store, run) = setup();
        let (_, empty) = call(&app, &viewer(), "GET", "/v1/notes/threads", None).await;
        assert!(empty["latest_note_at"].is_null());

        seed_note(&store, "op-a", Some(run), 2);
        let newest = seed_note(&store, "op-b", None, 1);

        // Opened on "mine", the inbox has still seen up to the newest note —
        // what moves the marker, whichever filter is showing.
        let (_, mine) = call(
            &app,
            &operator("op-a"),
            "GET",
            "/v1/notes/threads?mine=1",
            None,
        )
        .await;
        assert_eq!(mine["threads"].as_array().unwrap().len(), 1);
        let latest: DateTime<Utc> = serde_json::from_value(mine["latest_note_at"].clone()).unwrap();
        assert_eq!(latest, newest.created_at);

        let (_, first) = call(&app, &viewer(), "GET", "/v1/notes/threads?limit=1", None).await;
        assert_eq!(first["threads"].as_array().unwrap().len(), 1);
        assert_eq!(first["has_more"], true);
        let (_, both) = call(&app, &viewer(), "GET", "/v1/notes/threads?limit=2", None).await;
        assert_eq!(both["has_more"], false);
    }

    #[tokio::test]
    async fn bad_inbox_parameters_are_rejected() {
        let (app, _store, _run) = setup();
        for query in [
            "seen_at=yesterday",
            // An unencoded `+` arrives as a space.
            "seen_at=2026-10-08T10:00:00+00:00",
            "limit=lots",
            "mine=maybe",
            "unread=yes",
        ] {
            let (status, body) = call(
                &app,
                &viewer(),
                "GET",
                &format!("/v1/notes/threads?{query}"),
                None,
            )
            .await;
            assert_eq!(status, 400, "{query}: {body}");
            assert_eq!(body["error"], "invalid_note", "{query}");
        }
        let (status, _) = call(
            &app,
            &viewer(),
            "GET",
            "/v1/notes/threads?seen_at=2026-10-08T10:00:00%2B00:00&mine=0&unread=false",
            None,
        )
        .await;
        assert_eq!(
            status, 200,
            "a percent-encoded offset and explicit no's are fine"
        );
    }

    #[tokio::test]
    async fn the_inbox_needs_executions_read() {
        let (app, _store, _run) = setup();
        let (status, _) = call(
            &app,
            &bearer("jobs-only", &[Scope::JOBS_READ]),
            "GET",
            "/v1/notes/threads",
            None,
        )
        .await;
        assert_eq!(status, 403);
    }

    #[tokio::test]
    async fn notes_seen_needs_a_user() {
        let (app, _store, _run) = setup();
        let (status, _) = call(&app, &viewer(), "GET", "/v1/users/me/notes-seen", None).await;
        assert_eq!(status, 403, "an API key has no marker");
        let (status, _) = call(
            &app,
            &viewer(),
            "PUT",
            "/v1/users/me/notes-seen",
            Some(serde_json::json!({"seen_at": Utc::now()})),
        )
        .await;
        assert_eq!(status, 403);
    }

    #[tokio::test]
    async fn a_viewer_reads_and_marks_notes_seen() {
        let (app, store, _run) = setup();
        seed_user(&store, "vic", Role::Viewer);
        let vic = user_token("vic", Role::Viewer);

        let (status, seen) = call(&app, &vic, "GET", "/v1/users/me/notes-seen", None).await;
        assert_eq!(status, 200, "{seen}");
        assert!(seen["seen_at"].is_null(), "never opened");
        assert_eq!(seen["unread_threads"], 0);

        let at = Utc::now() - chrono::Duration::hours(1);
        let (status, seen) = call(
            &app,
            &vic,
            "PUT",
            "/v1/users/me/notes-seen",
            Some(serde_json::json!({ "seen_at": at })),
        )
        .await;
        assert_eq!(status, 200, "reading is all marking needs: {seen}");
        let stored: DateTime<Utc> = serde_json::from_value(seen["seen_at"].clone()).unwrap();
        assert_eq!(stored, at);
    }

    #[tokio::test]
    async fn the_marker_moves_forward_only_and_never_past_now() {
        let (app, store, _run) = setup();
        seed_user(&store, "ana", Role::Operator);
        let ana = user_token("ana", Role::Operator);
        let put = |at: DateTime<Utc>| {
            let (app, ana) = (app.clone(), ana.clone());
            async move {
                let (status, seen) = call(
                    &app,
                    &ana,
                    "PUT",
                    "/v1/users/me/notes-seen",
                    Some(serde_json::json!({ "seen_at": at })),
                )
                .await;
                assert_eq!(status, 200, "{seen}");
                serde_json::from_value::<DateTime<Utc>>(seen["seen_at"].clone()).unwrap()
            }
        };

        let tomorrow = Utc::now() + chrono::Duration::days(1);
        let stored = put(tomorrow).await;
        assert!(stored <= Utc::now(), "clamped to now, not {stored}");

        let stored_again = put(Utc::now() - chrono::Duration::days(1)).await;
        assert_eq!(stored_again, stored, "an older value leaves it where it is");
    }

    #[tokio::test]
    async fn opening_the_inbox_clears_the_badge_until_someone_writes() {
        let (app, store, run) = setup();
        seed_user(&store, "ana", Role::Operator);
        let ana = user_token("ana", Role::Operator);
        let badge = || {
            let (app, ana) = (app.clone(), ana.clone());
            async move {
                let (status, seen) = call(&app, &ana, "GET", "/v1/users/me/notes-seen", None).await;
                assert_eq!(status, 200, "{seen}");
                seen["unread_threads"].as_u64().unwrap()
            }
        };

        seed_note(&store, "op-b", Some(run), 1);
        assert_eq!(badge().await, 1);

        // What the dashboard does on opening the inbox: hand back the newest
        // note the page carried.
        let (_, page) = call(&app, &ana, "GET", "/v1/notes/threads", None).await;
        assert_eq!(page["threads"][0]["unread"], true);
        let (status, seen) = call(
            &app,
            &ana,
            "PUT",
            "/v1/users/me/notes-seen",
            Some(serde_json::json!({ "seen_at": page["latest_note_at"] })),
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(
            seen["unread_threads"], 0,
            "the answer carries the new count"
        );
        assert_eq!(badge().await, 0);

        // Her own note is never news to her.
        let (status, _) = call(
            &app,
            &ana,
            "POST",
            "/v1/notes",
            Some(serde_json::json!({
                "job_key": "mail:send", "execution_id": run, "kind": "note", "body": "on it"
            })),
        )
        .await;
        assert_eq!(status, 201);
        assert_eq!(badge().await, 0);

        let (status, _) = call(
            &app,
            &operator("op-b"),
            "POST",
            "/v1/notes",
            Some(serde_json::json!({
                "job_key": "mail:send", "execution_id": run, "kind": "note", "body": "thanks"
            })),
        )
        .await;
        assert_eq!(status, 201);
        assert_eq!(badge().await, 1, "a colleague's reply is");
    }
}

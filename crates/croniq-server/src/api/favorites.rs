//! Jobs a user has starred — `/v1/users/me/favorites`. The dashboard sorts
//! them first on the live timeline and the Runs screen can narrow to them
//! (`GET /v1/executions?favorites=1`).
//!
//! Layout:
//!   GET    /v1/users/me/favorites             self — the starred job keys
//!   PUT    /v1/users/me/favorites/{job_key}   self — star (idempotent)
//!   DELETE /v1/users/me/favorites/{job_key}   self — unstar (idempotent)
//!
//! Favorites belong to a user, so only a user can have them: an API key or an
//! API-client token gets `403`, as on every other `/v1/users/me/*` route. All
//! three need `jobs:read` — the list is a list of job keys.
//!
//! Not audit-logged: a star is the operator's own view of the dashboard, like
//! the timeline order the browser remembers, and changes nothing anyone else
//! sees.

use std::sync::Arc;

use axum::response::{IntoResponse, Response};
use axum::{Extension, Json, extract::State, http::StatusCode};
use chrono::Utc;
use croniq_auth::CallerContext;
use croniq_auth::context::Scope;
use serde::Serialize;

use super::ServerState;
use crate::api::auth_middleware::require_scope;
use crate::api::calendars::ValidationError;
use crate::api::notes::job_exists;
use crate::api::users::require_self_user;
use crate::store::DynStore;

/// Most jobs one user may star. A favorite is a job someone keeps an eye on,
/// which is a handful; the cap bounds the `IN` list the Runs filter sends to
/// the store.
pub(crate) const MAX_FAVORITES: usize = 500;

#[derive(Debug, Serialize)]
pub struct FavoritesView {
    pub job_keys: Vec<String>,
}

/// A refusal: a bare status, or the `409` that says the user has starred as
/// many jobs as one may.
#[derive(Debug)]
pub enum FavoriteError {
    Status(StatusCode),
    TooMany,
}

impl From<StatusCode> for FavoriteError {
    fn from(status: StatusCode) -> Self {
        Self::Status(status)
    }
}

impl IntoResponse for FavoriteError {
    fn into_response(self) -> Response {
        match self {
            Self::Status(status) => status.into_response(),
            Self::TooMany => (
                StatusCode::CONFLICT,
                Json(ValidationError {
                    error: "too_many_favorites",
                    message: format!("at most {MAX_FAVORITES} favorite jobs per user"),
                }),
            )
                .into_response(),
        }
    }
}

fn store_of(state: &ServerState) -> Result<&DynStore, FavoriteError> {
    Ok(state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?)
}

fn internal<E>(_: E) -> FavoriteError {
    FavoriteError::Status(StatusCode::INTERNAL_SERVER_ERROR)
}

/// `GET /v1/users/me/favorites`
pub async fn handle_list(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
) -> Result<Json<FavoritesView>, FavoriteError> {
    require_scope(&ctx, Scope::JOBS_READ)?;
    let user_id = require_self_user(&ctx)?;
    let store = store_of(&state)?;
    let job_keys = store.list_favorites(user_id).map_err(internal)?;
    Ok(Json(FavoritesView { job_keys }))
}

/// `PUT /v1/users/me/favorites/{job_key}` — `204`, whether or not the job was
/// starred already. A new star needs the job to exist; a star on a job that is
/// later deleted stays, and shows again if the job comes back.
pub async fn handle_add(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(job_key): axum::extract::Path<String>,
) -> Result<StatusCode, FavoriteError> {
    require_scope(&ctx, Scope::JOBS_READ)?;
    let user_id = require_self_user(&ctx)?;
    let store = store_of(&state)?;

    let current = store.list_favorites(user_id).map_err(internal)?;
    if current.contains(&job_key) {
        return Ok(StatusCode::NO_CONTENT);
    }
    if !job_exists(&state, store, &job_key)
        .await
        .map_err(internal)?
    {
        return Err(StatusCode::NOT_FOUND.into());
    }
    if current.len() >= MAX_FAVORITES {
        return Err(FavoriteError::TooMany);
    }
    store
        .add_favorite(user_id, &job_key, Utc::now())
        .map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /v1/users/me/favorites/{job_key}` — `204`, whether or not the job
/// was starred. No existence check, so a star on a deleted job can be removed.
pub async fn handle_remove(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(job_key): axum::extract::Path<String>,
) -> Result<StatusCode, FavoriteError> {
    require_scope(&ctx, Scope::JOBS_READ)?;
    let user_id = require_self_user(&ctx)?;
    let store = store_of(&state)?;
    store.remove_favorite(user_id, &job_key).map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{ServerState, server_router};
    use crate::store::sqlite_store;
    use axum::body::Body;
    use axum::http::Request;
    use croniq_auth::jwt::issue_token_pair;
    use croniq_auth::{AuthMethod, CallerType, Role};
    use croniq_runner::AppState;
    use croniq_store::models::{Execution, ExecutionState, JobDefinition, User};
    use croniq_store::sqlite::SqliteStore;
    use http_body_util::BodyExt;
    use std::collections::HashMap;
    use tokio::sync::mpsc;
    use tower::util::ServiceExt;
    use uuid::Uuid;

    fn make_state(store: DynStore) -> Arc<ServerState> {
        let runner = AppState::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        ServerState::with_auth(runner, tx, Some(crate::api::test_auth::jwt()), Some(store))
    }

    fn seed_user(store: &DynStore, user_id: &str) {
        let now = Utc::now();
        store
            .users_create(&User {
                user_id: user_id.into(),
                username: user_id.into(),
                email: None,
                display_name: None,
                role: Role::Operator,
                is_active: true,
                created_at: now,
                updated_at: now,
                last_login_at: None,
            })
            .unwrap();
    }

    /// A logged-in user's access token.
    fn user(user_id: &str) -> String {
        let scopes = vec![Scope::JOBS_READ.to_string(), Scope::EXECUTIONS_READ.into()];
        let pair = issue_token_pair(
            &crate::api::test_auth::jwt(),
            user_id,
            user_id,
            CallerType::User,
            Some(user_id),
            Some(Role::Operator),
            AuthMethod::Password,
            &scopes,
            None,
        )
        .unwrap();
        format!("Bearer {}", pair.access_token)
    }

    /// An API-client token with the same scopes but no user behind it.
    fn api_client() -> String {
        let scopes = vec![Scope::JOBS_READ.to_string(), Scope::EXECUTIONS_READ.into()];
        let pair = issue_token_pair(
            &crate::api::test_auth::jwt(),
            "deploy-bot",
            "deploy-bot",
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

    fn seed_run(store: &DynStore, job_key: &str) -> Uuid {
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
    ) -> (u16, serde_json::Value) {
        let req = Request::builder()
            .header("authorization", auth)
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status().as_u16();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, json)
    }

    fn setup() -> (axum::Router, DynStore) {
        let store = sqlite_store(SqliteStore::in_memory().unwrap());
        seed_user(&store, "anna");
        seed_user(&store, "ben");
        seed_job(&store, "mail:send");
        seed_job(&store, "report:daily");
        let app = server_router(make_state(store.clone()));
        (app, store)
    }

    #[tokio::test]
    async fn starring_is_idempotent_and_per_user() {
        let (app, _store) = setup();
        let anna = user("anna");

        for _ in 0..2 {
            let (status, _) = call(&app, &anna, "PUT", "/v1/users/me/favorites/mail:send").await;
            assert_eq!(status, 204);
        }
        let (status, body) = call(&app, &anna, "GET", "/v1/users/me/favorites").await;
        assert_eq!(status, 200);
        assert_eq!(body, serde_json::json!({"job_keys": ["mail:send"]}));

        // Ben's view is his own.
        let (_, body) = call(&app, &user("ben"), "GET", "/v1/users/me/favorites").await;
        assert_eq!(body, serde_json::json!({"job_keys": []}));

        for _ in 0..2 {
            let (status, _) = call(&app, &anna, "DELETE", "/v1/users/me/favorites/mail:send").await;
            assert_eq!(status, 204);
        }
        let (_, body) = call(&app, &anna, "GET", "/v1/users/me/favorites").await;
        assert_eq!(body, serde_json::json!({"job_keys": []}));
    }

    #[tokio::test]
    async fn an_unknown_job_cannot_be_starred() {
        let (app, _store) = setup();
        let (status, _) = call(&app, &user("anna"), "PUT", "/v1/users/me/favorites/nope").await;
        assert_eq!(status, 404);
    }

    #[tokio::test]
    async fn the_star_count_is_capped() {
        let (app, store) = setup();
        for i in 0..MAX_FAVORITES {
            store
                .add_favorite("anna", &format!("job-{i}"), Utc::now())
                .unwrap();
        }
        let anna = user("anna");
        let (status, body) = call(&app, &anna, "PUT", "/v1/users/me/favorites/mail:send").await;
        assert_eq!(status, 409);
        assert_eq!(body["error"], "too_many_favorites");
        // Re-starring one already there is still fine, even though `job-0`
        // names no job (any more).
        let (status, _) = call(&app, &anna, "PUT", "/v1/users/me/favorites/job-0").await;
        assert_eq!(status, 204);
    }

    #[tokio::test]
    async fn favorites_need_a_user() {
        let (app, _store) = setup();
        let bot = api_client();
        for (method, uri) in [
            ("GET", "/v1/users/me/favorites"),
            ("PUT", "/v1/users/me/favorites/mail:send"),
            ("DELETE", "/v1/users/me/favorites/mail:send"),
        ] {
            let (status, _) = call(&app, &bot, method, uri).await;
            assert_eq!(status, 403, "{method} {uri}");
        }
    }

    #[tokio::test]
    async fn the_runs_list_narrows_to_the_callers_favorites() {
        let (app, store) = setup();
        let mail = seed_run(&store, "mail:send");
        seed_run(&store, "report:daily");
        let anna = user("anna");

        // No favorites yet: nothing, not everything.
        let (status, body) = call(&app, &anna, "GET", "/v1/executions?favorites=1").await;
        assert_eq!(status, 200);
        assert_eq!(body, serde_json::json!([]));

        call(&app, &anna, "PUT", "/v1/users/me/favorites/mail:send").await;
        let (_, body) = call(&app, &anna, "GET", "/v1/executions?favorites=1").await;
        let ids: Vec<&str> = body
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec![mail.to_string()]);

        // Without the flag, or with it off, the list is unchanged.
        for uri in ["/v1/executions", "/v1/executions?favorites=0"] {
            let (_, body) = call(&app, &anna, "GET", uri).await;
            assert_eq!(body.as_array().unwrap().len(), 2, "{uri}");
        }
    }

    #[tokio::test]
    async fn the_favorites_filter_is_refused_without_a_user() {
        let (app, store) = setup();
        seed_run(&store, "mail:send");
        let (status, body) = call(&app, &api_client(), "GET", "/v1/executions?favorites=1").await;
        assert_eq!(status, 400);
        assert_eq!(body["error"], "favorites_requires_user");
    }
}

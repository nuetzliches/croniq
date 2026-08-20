//! The API refuses to edit a client the environment owns (issue #471).
//!
//! Ownership is only worth storing if it is enforced. An accepted edit to an
//! env-declared client would survive until the next reconcile and then revert
//! with no trace — the operator sees their change in the dashboard, and later
//! sees it gone, with nothing connecting the two. Each refusal here names the
//! variable to edit instead, so the 409 is a redirect rather than a wall.

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use chrono::Utc;
use croniq_auth::context::Scope;
use croniq_auth::jwt::JwtConfig;
use croniq_auth::password::hash_password;
use croniq_runner::AppState;
use croniq_server::api::{ServerState, server_router};
use croniq_server::sqlite_store;
use croniq_server::store::DynStore;
use croniq_store::models::{ApiClient, PasswordCredential, Role, User};
use croniq_store::sqlite::SqliteStore;
use http_body_util::BodyExt;
use tokio::sync::mpsc;
use tower::util::ServiceExt;

const PASSWORD: &str = "correct-horse-battery";
const JWT_SECRET: &str = "env-managed-clients-test-secret";
/// Declared by `CRONIQ_API_CLIENT_RUNNER_POLL_KEY`, so the refusal must name
/// exactly that variable back to the operator.
const ENV_CLIENT: &str = "runner-poll";
const API_CLIENT: &str = "hand-made";

fn fixture() -> (axum::Router, DynStore) {
    let store = sqlite_store(SqliteStore::in_memory().unwrap());
    let now = Utc::now();
    store
        .users_create(&User {
            user_id: "u-admin".into(),
            username: "admin".into(),
            email: None,
            display_name: None,
            role: Role::Admin,
            is_active: true,
            created_at: now,
            updated_at: now,
            last_login_at: None,
        })
        .unwrap();
    store
        .upsert_credentials(&PasswordCredential {
            user_id: "u-admin".into(),
            username: "admin".into(),
            password_hash: hash_password(PASSWORD).unwrap(),
            failed_attempts: 0,
            locked_until: None,
            created_at: now,
        })
        .unwrap();
    seed(&store, ENV_CLIENT, "env");
    seed(&store, API_CLIENT, "api");

    let (tx, _rx) = mpsc::unbounded_channel();
    let state = ServerState::with_auth(
        AppState::new(),
        tx,
        Some(JwtConfig::new(JWT_SECRET)),
        Some(store.clone()),
    );
    (server_router(state), store)
}

fn seed(store: &DynStore, name: &str, managed_by: &str) {
    store
        .create_client(&ApiClient {
            client_id: name.into(),
            name: name.into(),
            scopes: vec![Scope::JOBS_READ.to_string()],
            is_active: true,
            created_at: Utc::now(),
            managed_by: managed_by.into(),
        })
        .unwrap();
}

async fn admin_token(app: axum::Router) -> String {
    let req = Request::builder()
        .method("POST")
        .uri("/v1/auth/login")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({ "username": "admin", "password": PASSWORD }).to_string(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "admin login failed");
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    body["access_token"].as_str().unwrap().to_string()
}

async fn send(
    app: axum::Router,
    method: &str,
    uri: &str,
    token: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"));
    let body = match body {
        Some(v) => {
            req = req.header("content-type", "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    let resp = app.oneshot(req.body(body).unwrap()).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let value = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    };
    (status, value)
}

/// Every refusal must name the variable that owns the row — a bare 409 leaves
/// the operator with nowhere to go.
fn assert_env_managed(status: StatusCode, body: &serde_json::Value) {
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "env_managed", "{body}");
    let message = body["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("CRONIQ_API_CLIENT_RUNNER_POLL_KEY"),
        "refusal must name the owning variable: {message}"
    );
}

#[tokio::test]
async fn editing_an_env_managed_client_is_refused() {
    let (app, store) = fixture();
    let admin = admin_token(app.clone()).await;

    let (status, body) = send(
        app,
        "PUT",
        &format!("/v1/api-clients/{ENV_CLIENT}"),
        &admin,
        Some(serde_json::json!({ "scopes": ["admin"] })),
    )
    .await;
    assert_env_managed(status, &body);

    // And the row is untouched — refusing but writing anyway would be worse
    // than either alternative.
    let stored = store.get_client(ENV_CLIENT).unwrap().unwrap();
    assert_eq!(stored.scopes, vec![Scope::JOBS_READ]);
}

#[tokio::test]
async fn deleting_an_env_managed_client_is_refused() {
    let (app, store) = fixture();
    let admin = admin_token(app.clone()).await;

    // Allowing this would be worse than a no-op: the next reconcile recreates
    // the client, so the operator watches it come back with no explanation.
    let (status, body) = send(
        app,
        "DELETE",
        &format!("/v1/api-clients/{ENV_CLIENT}"),
        &admin,
        None,
    )
    .await;
    assert_env_managed(status, &body);
    assert!(store.get_client(ENV_CLIENT).unwrap().is_some());
}

#[tokio::test]
async fn minting_a_key_for_an_env_managed_client_is_refused() {
    let (app, store) = fixture();
    let admin = admin_token(app.clone()).await;

    // The reconciler retires every key that is not the declared one, so a key
    // minted here would carry a silent expiry date.
    let (status, body) = send(
        app,
        "POST",
        "/v1/api-keys",
        &admin,
        Some(serde_json::json!({ "client_id": ENV_CLIENT })),
    )
    .await;
    assert_env_managed(status, &body);
    assert!(store.list_api_keys(ENV_CLIENT).unwrap().is_empty());
}

#[tokio::test]
async fn api_managed_clients_are_still_fully_editable() {
    // The guard must be scoped to env ownership — everything else keeps
    // working exactly as before.
    let (app, store) = fixture();
    let admin = admin_token(app.clone()).await;

    let (status, body) = send(
        app.clone(),
        "PUT",
        &format!("/v1/api-clients/{API_CLIENT}"),
        &admin,
        Some(serde_json::json!({ "scopes": ["jobs:read", "jobs:trigger"] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        store.get_client(API_CLIENT).unwrap().unwrap().scopes,
        vec!["jobs:read", "jobs:trigger"]
    );

    let (status, body) = send(
        app.clone(),
        "POST",
        "/v1/api-keys",
        &admin,
        Some(serde_json::json!({ "client_id": API_CLIENT })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let (status, _) = send(
        app,
        "DELETE",
        &format!("/v1/api-clients/{API_CLIENT}"),
        &admin,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(store.get_client(API_CLIENT).unwrap().is_none());
}

#[tokio::test]
async fn deleting_a_client_that_does_not_exist_stays_idempotent() {
    // The ownership lookup must not turn a 204 into a 404: delete was
    // idempotent before the guard existed.
    let (app, _store) = fixture();
    let admin = admin_token(app.clone()).await;
    let (status, _) = send(app, "DELETE", "/v1/api-clients/ghost", &admin, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn listing_clients_exposes_who_owns_each_row() {
    // Without this the dashboard cannot tell the operator why the edit button
    // is going to fail.
    let (app, _store) = fixture();
    let admin = admin_token(app.clone()).await;
    let (status, body) = send(app, "GET", "/v1/api-clients", &admin, None).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let rows = body.as_array().unwrap();
    let env_row = rows.iter().find(|r| r["name"] == ENV_CLIENT).unwrap();
    let api_row = rows.iter().find(|r| r["name"] == API_CLIENT).unwrap();
    assert_eq!(env_row["managed_by"], "env");
    assert_eq!(api_row["managed_by"], "api");
}

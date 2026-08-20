//! An API-key token pair is redeemable end to end (issue #463).
//!
//! `POST /v1/api-clients/{id}/tokens` mints an access/refresh pair for a
//! machine client. It used to hand out the refresh half without ever writing
//! a `refresh_tokens` row, so the token was cosmetic: `POST /v1/auth/refresh`
//! hashes what it is given, finds nothing, and answers 401. The API-key branch
//! of the refresh handler — the one that re-resolves scopes from the owning
//! client — was therefore unreachable.
//!
//! These tests drive the real router: provision a client as an admin, redeem
//! the refresh token, and check that the rotated credential tracks the client
//! row the way the user path tracks the user row.

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use chrono::Utc;
use croniq_auth::context::{CallerType, Scope};
use croniq_auth::jwt::{JwtConfig, validate_token};
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
const JWT_SECRET: &str = "api-key-refresh-test-secret";
const USER_ID: &str = "u-admin";
/// The machine client the tokens are minted for. Deliberately narrow: a
/// refresh that widened it back to `admin` would be an escalation.
const CLIENT_ID: &str = "worker";

fn fixture() -> (axum::Router, DynStore) {
    let store = sqlite_store(SqliteStore::in_memory().unwrap());
    let now = Utc::now();
    store
        .users_create(&User {
            user_id: USER_ID.into(),
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
            user_id: USER_ID.into(),
            username: "admin".into(),
            password_hash: hash_password(PASSWORD).unwrap(),
            failed_attempts: 0,
            locked_until: None,
            created_at: now,
        })
        .unwrap();
    seed_client(&store, &[Scope::JOBS_READ], true);

    let (tx, _rx) = mpsc::unbounded_channel();
    let state = ServerState::with_auth(
        AppState::new(),
        tx,
        Some(JwtConfig::new(JWT_SECRET)),
        Some(store.clone()),
    );
    (server_router(state), store)
}

/// `create_client` upserts, so this doubles as the "an admin edited the
/// client" step in the scope/deactivation tests.
fn seed_client(store: &DynStore, scopes: &[&str], is_active: bool) {
    store
        .create_client(&ApiClient {
            client_id: CLIENT_ID.into(),
            name: "worker".into(),
            scopes: scopes.iter().map(|s| s.to_string()).collect(),
            is_active,
            created_at: Utc::now(),
            managed_by: "api".into(),
        })
        .unwrap();
}

async fn post(
    app: axum::Router,
    uri: &str,
    token: Option<&str>,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let mut req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(t) = token {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    let resp = app
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let value = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    };
    (status, value)
}

async fn admin_token(app: axum::Router) -> String {
    let (status, body) = post(
        app,
        "/v1/auth/login",
        None,
        serde_json::json!({ "username": "admin", "password": PASSWORD }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "admin login failed: {body}");
    body["access_token"].as_str().unwrap().to_string()
}

/// Provision the machine credential the way an operator's tooling would, and
/// return the access + refresh halves.
async fn issue_client_token(app: axum::Router) -> (String, String) {
    let admin = admin_token(app.clone()).await;
    let (status, body) = post(
        app,
        &format!("/v1/api-clients/{CLIENT_ID}/tokens"),
        Some(&admin),
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "issuing a client token failed: {body}"
    );
    (
        body["access_token"].as_str().unwrap().to_string(),
        body["refresh_token"]
            .as_str()
            .expect("the endpoint advertises a refresh token")
            .to_string(),
    )
}

async fn refresh(app: axum::Router, token: &str) -> (StatusCode, serde_json::Value) {
    post(
        app,
        "/v1/auth/refresh",
        None,
        serde_json::json!({ "refresh_token": token }),
    )
    .await
}

async fn list_jobs_status(app: axum::Router, token: &str) -> StatusCode {
    app.oneshot(
        Request::builder()
            .method("GET")
            .uri("/v1/jobs")
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .unwrap()
    .status()
}

#[tokio::test]
async fn an_issued_client_token_can_actually_be_refreshed() {
    let (app, _store) = fixture();
    let (_access, refresh_token) = issue_client_token(app.clone()).await;

    let (status, body) = refresh(app.clone(), &refresh_token).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the refresh token this endpoint hands out must be redeemable — a 401 \
         here is issue #463: {body}"
    );

    let access = body["access_token"].as_str().unwrap();
    let ctx = validate_token(&JwtConfig::new(JWT_SECRET), access).unwrap();
    assert_eq!(ctx.caller_type, CallerType::ApiKey);
    assert_eq!(ctx.client_id, CLIENT_ID);
    assert!(
        ctx.user_id.is_none(),
        "a machine credential must not acquire a user identity on refresh"
    );
    assert_eq!(ctx.scopes, vec![Scope::JOBS_READ]);
    assert_eq!(
        list_jobs_status(app.clone(), access).await,
        StatusCode::OK,
        "the refreshed access token has to work on the client's own scope"
    );

    // Rotation, same as the user path: the new token differs and the old one
    // is spent.
    let rotated = body["refresh_token"].as_str().unwrap();
    assert_ne!(rotated, refresh_token, "the refresh token must rotate");
    let (replay, _) = refresh(app.clone(), &refresh_token).await;
    assert_eq!(
        replay,
        StatusCode::UNAUTHORIZED,
        "the consumed refresh token must not be redeemable twice"
    );
    let (again, body) = refresh(app, rotated).await;
    assert_eq!(
        again,
        StatusCode::OK,
        "the rotated token is the live one: {body}"
    );
}

#[tokio::test]
async fn a_client_token_refresh_picks_up_narrowed_scopes() {
    let (app, store) = fixture();
    let (_access, refresh_token) = issue_client_token(app.clone()).await;

    // An operator takes `jobs:read` away and grants only `runners:read`.
    seed_client(&store, &[Scope::RUNNERS_READ], true);

    let (status, body) = refresh(app.clone(), &refresh_token).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let access = body["access_token"].as_str().unwrap();
    let ctx = validate_token(&JwtConfig::new(JWT_SECRET), access).unwrap();
    assert_eq!(
        ctx.scopes,
        vec![Scope::RUNNERS_READ],
        "refresh re-reads the client row, so a revoked scope stays revoked"
    );
    assert_eq!(
        list_jobs_status(app, access).await,
        StatusCode::FORBIDDEN,
        "the rotated token must not carry a scope the client no longer has"
    );
}

#[tokio::test]
async fn a_deactivated_client_cannot_refresh() {
    let (app, store) = fixture();
    let (_access, refresh_token) = issue_client_token(app.clone()).await;

    seed_client(&store, &[Scope::JOBS_READ], false);

    let (status, _) = refresh(app, &refresh_token).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "deactivating a client has to stop its refresh token, exactly like \
         deactivating a user does"
    );
}

#[tokio::test]
async fn a_deleted_client_cannot_refresh() {
    let (app, store) = fixture();
    let (_access, refresh_token) = issue_client_token(app.clone()).await;

    store.delete_client(CLIENT_ID).unwrap();

    let (status, _) = refresh(app, &refresh_token).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "a refresh token whose client is gone must fail, not rotate into a \
         scope-less token"
    );
}

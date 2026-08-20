//! Rotation with a grace window, end to end (issue #471).
//!
//! Two claims carry the whole design and neither is provable from the
//! reconciler's own unit tests:
//!
//! 1. `expires_at` is *enforced* — a retired key stops authenticating once
//!    its deadline passes, and keeps working until then. If it were not, the
//!    grace window would be a permanent second live credential rather than a
//!    handover period.
//! 2. `GET /v1/api-keys` makes the window inspectable. An operator has to be
//!    able to see that a second key is live and when it dies — and to grab
//!    the `key_id` for the break-glass `DELETE` that a leak calls for.

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use chrono::{Duration, Utc};
use croniq_auth::api_key::hash_api_key;
use croniq_auth::context::Scope;
use croniq_auth::jwt::JwtConfig;
use croniq_auth::password::hash_password;
use croniq_runner::AppState;
use croniq_server::api::{ServerState, server_router};
use croniq_server::sqlite_store;
use croniq_server::store::DynStore;
use croniq_store::models::{ApiClient, ApiKey, PasswordCredential, Role, User};
use croniq_store::sqlite::SqliteStore;
use http_body_util::BodyExt;
use tokio::sync::mpsc;
use tower::util::ServiceExt;
use uuid::Uuid;

const PASSWORD: &str = "correct-horse-battery";
const JWT_SECRET: &str = "api-key-rotation-test-secret";
const CLIENT_ID: &str = "runner-poll";

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
    store
        .create_client(&ApiClient {
            client_id: CLIENT_ID.into(),
            name: "runner-poll".into(),
            scopes: vec![Scope::JOBS_READ.to_string()],
            is_active: true,
            created_at: now,
            managed_by: "api".into(),
        })
        .unwrap();

    let (tx, _rx) = mpsc::unbounded_channel();
    let state = ServerState::with_auth(
        AppState::new(),
        tx,
        Some(JwtConfig::new(JWT_SECRET)),
        Some(store.clone()),
    );
    (server_router(state), store)
}

/// Install a raw key for the fixture client, optionally pre-retired with an
/// expiry — the state a rotation leaves the superseded key in.
fn seed_key(store: &DynStore, raw: &str, expires_at: Option<chrono::DateTime<Utc>>) -> String {
    let key_id = Uuid::new_v4().to_string();
    store
        .create_api_key(&ApiKey {
            key_id: key_id.clone(),
            client_id: CLIENT_ID.into(),
            key_hash: hash_api_key(raw),
            key_prefix: raw.chars().take(12).collect(),
            expires_at,
            revoked_at: None,
            created_at: Utc::now(),
        })
        .unwrap();
    key_id
}

async fn get(app: axum::Router, uri: &str, auth: Option<&str>) -> (StatusCode, serde_json::Value) {
    let mut req = Request::builder().method("GET").uri(uri);
    if let Some(a) = auth {
        req = req.header("authorization", a);
    }
    let resp = app.oneshot(req.body(Body::empty()).unwrap()).await.unwrap();
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

// ─── 1. The grace window is real, and it ends ────────────────────────────────

#[tokio::test]
async fn a_key_inside_its_grace_window_still_authenticates() {
    let (app, store) = fixture();
    seed_key(
        &store,
        "croniq_retired_but_live",
        Some(Utc::now() + Duration::minutes(15)),
    );

    let (status, _) = get(app, "/v1/jobs", Some("ApiKey croniq_retired_but_live")).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a retired key must keep working until its deadline — that is the entire point of the \
         handover window"
    );
}

#[tokio::test]
async fn a_key_past_its_grace_window_is_rejected() {
    let (app, store) = fixture();
    seed_key(
        &store,
        "croniq_grace_elapsed",
        Some(Utc::now() - Duration::seconds(1)),
    );

    let (status, _) = get(app, "/v1/jobs", Some("ApiKey croniq_grace_elapsed")).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "once the deadline passes the old credential must be dead"
    );
}

// ─── 2. The window is inspectable ────────────────────────────────────────────

#[tokio::test]
async fn listing_keys_shows_the_retired_one_with_its_deadline_and_never_the_hash() {
    let (app, store) = fixture();
    let deadline = Utc::now() + Duration::minutes(15);
    let retired = seed_key(&store, "croniq_old_value", Some(deadline));
    let current = seed_key(&store, "croniq_new_value", None);

    let admin = admin_token(app.clone()).await;
    let (status, body) = get(
        app,
        &format!("/v1/api-keys?client_id={CLIENT_ID}"),
        Some(&format!("Bearer {admin}")),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let rows = body.as_array().expect("an array of keys");
    assert_eq!(
        rows.len(),
        2,
        "both halves of the handover are listed: {body}"
    );

    let find = |id: &str| {
        rows.iter()
            .find(|r| r["key_id"] == id)
            .unwrap_or_else(|| panic!("key {id} missing from {body}"))
            .clone()
    };
    let old = find(&retired);
    let new = find(&current);

    assert!(
        old["expires_at"].is_string(),
        "the operator has to be able to see when the old key dies: {old}"
    );
    assert!(
        new["expires_at"].is_null(),
        "the incoming key carries no deadline: {new}"
    );
    // The prefix identifies a row; the hash would make the listing itself a
    // credential leak.
    assert_eq!(old["key_prefix"], "croniq_old_v");
    for row in rows {
        assert!(
            row.get("key_hash").is_none(),
            "a key listing must never expose the hash: {row}"
        );
    }
}

#[tokio::test]
async fn listing_an_unknown_client_is_404_not_an_empty_list() {
    let (app, _store) = fixture();
    let admin = admin_token(app.clone()).await;
    let (status, _) = get(
        app,
        "/v1/api-keys?client_id=no-such-client",
        Some(&format!("Bearer {admin}")),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "'no keys' and 'wrong id' are different answers to an operator chasing a broken \
         credential"
    );
}

#[tokio::test]
async fn listing_keys_requires_the_admin_scope() {
    let (app, store) = fixture();
    // The fixture client holds jobs:read only — exactly the narrow machine
    // credential that must not be able to enumerate credentials.
    seed_key(&store, "croniq_narrow_key", None);

    let (status, _) = get(
        app,
        &format!("/v1/api-keys?client_id={CLIENT_ID}"),
        Some("ApiKey croniq_narrow_key"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

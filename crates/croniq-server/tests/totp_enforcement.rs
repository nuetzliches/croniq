//! Enforced-2FA login behaviour (`auth { totp { required true } }`).
//!
//! Covers the security-critical gate: when enforcement is on, an account
//! with no confirmed TOTP secret must be refused at `/v1/auth/login` (it
//! cannot satisfy the requirement and must enrol first), while the same
//! account logs in normally when enforcement is off. Also asserts the flag
//! is surfaced on `/v1/auth/config` so the login UI can show the code field
//! up-front.

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use chrono::Utc;
use croniq_auth::jwt::JwtConfig;
use croniq_auth::password::hash_password;
use croniq_runner::AppState;
use croniq_server::api::{ServerState, server_router};
use croniq_server::sqlite_store;
use croniq_store::models::{PasswordCredential, Role, User};
use croniq_store::sqlite::SqliteStore;
use http_body_util::BodyExt;
use tokio::sync::mpsc;
use tower::util::ServiceExt;

const PASSWORD: &str = "correct-horse-battery";

/// Build a store-backed, JWT-backed app seeded with one active password user
/// that has NO TOTP secret. `require_totp` toggles enforced 2FA.
fn app_with_user(require_totp: bool) -> axum::Router {
    let store = sqlite_store(SqliteStore::in_memory().unwrap());
    let now = Utc::now();
    let user = User {
        user_id: "u-admin".into(),
        username: "admin".into(),
        email: None,
        display_name: None,
        role: Role::Admin,
        is_active: true,
        created_at: now,
        updated_at: now,
        last_login_at: None,
    };
    store.users_create(&user).unwrap();
    store
        .upsert_credentials(&PasswordCredential {
            user_id: user.user_id.clone(),
            username: user.username.clone(),
            password_hash: hash_password(PASSWORD).unwrap(),
            failed_attempts: 0,
            locked_until: None,
            created_at: now,
        })
        .unwrap();

    let runner = AppState::new();
    let (tx, _rx) = mpsc::unbounded_channel();
    let jwt = JwtConfig {
        secret: "test-secret-not-for-prod".into(),
        ..Default::default()
    };
    let mut state = ServerState::with_auth(runner, tx, Some(jwt), Some(store));
    Arc::get_mut(&mut state).unwrap().require_totp = require_totp;
    server_router(state)
}

async fn post_json(app: axum::Router, uri: &str, body: serde_json::Value) -> (StatusCode, Vec<u8>) {
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    (status, bytes)
}

async fn get_json(app: axum::Router, uri: &str) -> (StatusCode, Vec<u8>) {
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    (status, bytes)
}

#[tokio::test]
async fn enforced_totp_rejects_account_without_secret() {
    let app = app_with_user(true);
    let (status, body) = post_json(
        app,
        "/v1/auth/login",
        serde_json::json!({ "username": "admin", "password": PASSWORD }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        v["error"],
        serde_json::json!("totp_required_not_configured")
    );
    assert!(v["message"].is_string());
}

#[tokio::test]
async fn non_enforced_login_succeeds_without_totp() {
    let app = app_with_user(false);
    let (status, body) = post_json(
        app,
        "/v1/auth/login",
        serde_json::json!({ "username": "admin", "password": PASSWORD }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(
        v["access_token"].is_string(),
        "expected a token pair, got {v}"
    );
}

#[tokio::test]
async fn enforced_totp_still_rejects_wrong_password() {
    // Enforcement must not change the password gate: a wrong password is a
    // 401, not the 403 "not configured" envelope.
    let app = app_with_user(true);
    let (status, _body) = post_json(
        app,
        "/v1/auth/login",
        serde_json::json!({ "username": "admin", "password": "wrong" }),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_config_exposes_totp_required() {
    let app = app_with_user(true);
    let (status, body) = get_json(app, "/v1/auth/config").await;
    assert_eq!(status, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["totp"]["required"], serde_json::json!(true));
}

#[tokio::test]
async fn auth_config_totp_not_required_by_default() {
    let app = app_with_user(false);
    let (_status, body) = get_json(app, "/v1/auth/config").await;
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["totp"]["required"], serde_json::json!(false));
}

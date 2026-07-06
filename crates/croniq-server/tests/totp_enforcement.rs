//! Enforced-2FA login behaviour (`auth { totp { required true } }`).
//!
//! Covers the security-critical gate: when enforcement is on, an account with
//! no confirmed TOTP secret is sent into inline enrolment at `/v1/auth/login`
//! (instead of being locked out), while the same account logs in normally when
//! enforcement is off. Also exercises the enrolment endpoints and asserts the
//! flag is surfaced on `/v1/auth/config` so the login UI can show the code
//! field up-front.

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

/// A 6-digit code guaranteed to be rejected for `secret_b32` — a hardcoded
/// "000000" would collide with a genuinely valid code in ~3e-6 of runs (3
/// accepted skew windows out of 10^6 codes) and flake. Recomputes the valid
/// codes with the same parameters croniq-auth uses (SHA1, 6 digits, 30 s
/// step, skew 1) for the windows around now — padded a few steps into the
/// future so a stall between this call and the server's check can't promote
/// the picked code to valid — and returns a code outside that set.
fn guaranteed_wrong_code(secret_b32: &str) -> String {
    let secret = totp_rs::Secret::Encoded(secret_b32.to_string())
        .to_bytes()
        .unwrap();
    let totp = totp_rs::TOTP::new(totp_rs::Algorithm::SHA1, 6, 1, 30, secret).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let valid: Vec<String> = (0..=5u64)
        .map(|i| totp.generate(now - 30 + i * 30))
        .collect();
    let mut candidate: u64 = valid[1].parse().unwrap();
    loop {
        candidate = (candidate + 1) % 1_000_000;
        let code = format!("{candidate:06}");
        if !valid.contains(&code) {
            return code;
        }
    }
}

/// Log in (enforced 2FA, no secret) and return the issued `enroll_token`.
async fn login_enroll_token(app: &axum::Router) -> String {
    let (status, body) = post_json(
        app.clone(),
        "/v1/auth/login",
        serde_json::json!({ "username": "admin", "password": PASSWORD }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    v["enroll_token"]
        .as_str()
        .expect("enroll_token in EnrollmentRequired response")
        .to_string()
}

#[tokio::test]
async fn enforced_totp_without_secret_starts_enrolment() {
    // Instead of a 403 lockout, the account is handed an enrolment token.
    let app = app_with_user(true);
    let (status, body) = post_json(
        app,
        "/v1/auth/login",
        serde_json::json!({ "username": "admin", "password": PASSWORD }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["enrollment_required"], serde_json::json!(true));
    assert!(
        v["enroll_token"].is_string(),
        "expected an enroll_token, got {v}"
    );
}

#[tokio::test]
async fn enroll_begin_returns_setup_material() {
    let app = app_with_user(true);
    let token = login_enroll_token(&app).await;
    let (status, body) = post_json(
        app,
        "/v1/auth/login/enroll/totp/begin",
        serde_json::json!({ "enroll_token": token }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(v["secret"].is_string());
    assert!(v["otpauth_url"].is_string());
    assert_eq!(v["recovery_codes"].as_array().unwrap().len(), 10);
}

#[tokio::test]
async fn enroll_confirm_rejects_wrong_code() {
    let app = app_with_user(true);
    let token = login_enroll_token(&app).await;
    // begin to persist a pending secret
    let (s1, body) = post_json(
        app.clone(),
        "/v1/auth/login/enroll/totp/begin",
        serde_json::json!({ "enroll_token": token.clone() }),
    )
    .await;
    assert_eq!(s1, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let wrong = guaranteed_wrong_code(v["secret"].as_str().unwrap());
    // a wrong code must not enable TOTP
    let (s2, _) = post_json(
        app,
        "/v1/auth/login/enroll/totp/confirm",
        serde_json::json!({ "enroll_token": token, "code": wrong }),
    )
    .await;
    assert_eq!(s2, StatusCode::UNAUTHORIZED);
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

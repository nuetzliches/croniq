//! Tests for the issue #138 "disable password login" feature.
//!
//! Covers:
//!   * `GET /v1/auth/config` shape, default and disabled
//!   * 403 + `{"error":"…"}` envelope on `/v1/auth/login`,
//!     `/v1/auth/login/totp`, and the password-reset endpoints
//!   * `/v1/auth/oidc/config` stays unchanged (back-compat)

use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use croniq_runner::AppState;
use croniq_server::api::{ServerState, server_router};
use http_body_util::BodyExt;
use tokio::sync::mpsc;
use tower::util::ServiceExt;

/// Build a router on top of a `ServerState` whose `password_login_enabled`
/// flag is set via the closure. The state has no store/JWT — these tests
/// only exercise the public probe + gate paths, which short-circuit before
/// hitting either.
fn router_with(password_login_enabled: bool) -> axum::Router {
    let runner = AppState::new();
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut state = ServerState::with_timeout(runner, tx, Duration::from_millis(50));
    Arc::get_mut(&mut state).unwrap().password_login_enabled = password_login_enabled;
    server_router(state)
}

async fn get(app: axum::Router, uri: &str) -> (StatusCode, Vec<u8>) {
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

// ─── /v1/auth/config ─────────────────────────────────────────────────────────

#[tokio::test]
async fn auth_config_defaults_have_password_enabled_oidc_disabled() {
    let app = router_with(true);
    let (status, body) = get(app, "/v1/auth/config").await;
    assert_eq!(status, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["password"]["enabled"], serde_json::json!(true));
    assert_eq!(v["oidc"]["enabled"], serde_json::json!(false));
    // No login_url leaks when OIDC isn't configured.
    assert!(v["oidc"]["login_url"].is_null());
    assert!(v["oidc"]["provider_name"].is_null());
}

#[tokio::test]
async fn auth_config_reflects_password_disabled() {
    let app = router_with(false);
    let (status, body) = get(app, "/v1/auth/config").await;
    assert_eq!(status, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["password"]["enabled"], serde_json::json!(false));
}

#[tokio::test]
async fn oidc_config_endpoint_stays_backwards_compatible() {
    // The legacy `/v1/auth/oidc/config` payload shape must not change — pre-138
    // clients still probe it and would break if we added the password flag here.
    let app = router_with(true);
    let (status, body) = get(app, "/v1/auth/oidc/config").await;
    assert_eq!(status, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let obj = v.as_object().expect("oidc/config returns an object");
    let mut keys: Vec<_> = obj.keys().collect();
    keys.sort();
    assert_eq!(
        keys,
        vec!["enabled", "login_url", "provider_name"],
        "oidc/config response keys must not change"
    );
}

// ─── Password-flow gating ────────────────────────────────────────────────────

#[tokio::test]
async fn login_returns_403_envelope_when_password_disabled() {
    let app = router_with(false);
    let (status, body) = post_json(
        app,
        "/v1/auth/login",
        serde_json::json!({"username": "admin", "password": "irrelevant"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["error"], serde_json::json!("password_login_disabled"));
    assert!(v["message"].is_string());
}

#[tokio::test]
async fn totp_login_returns_403_envelope_when_password_disabled() {
    let app = router_with(false);
    let (status, body) = post_json(
        app,
        "/v1/auth/login/totp",
        serde_json::json!({"mfa_token": "x", "code": "000000"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["error"], serde_json::json!("password_login_disabled"));
}

#[tokio::test]
async fn password_reset_request_returns_403_envelope_when_disabled() {
    let app = router_with(false);
    let (status, body) = post_json(
        app,
        "/v1/auth/password-reset/request",
        serde_json::json!({"username": "admin"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["error"], serde_json::json!("password_login_disabled"));
}

#[tokio::test]
async fn password_reset_confirm_returns_403_envelope_when_disabled() {
    let app = router_with(false);
    let (status, body) = post_json(
        app,
        "/v1/auth/password-reset/confirm",
        serde_json::json!({"token": "x", "new_password": "very-strong-pw"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["error"], serde_json::json!("password_login_disabled"));
}

//! Integration tests for the operational-override API (issue #231).
//!
//! Covers the write surface gated by `alerts:write`:
//!   * snooze / disable / throttle set-actions, note-required (400),
//!     unknown-rule (404), scope enforcement (403)
//!   * GET / DELETE override
//!   * overrides surfaced inline on GET /v1/alerts/config

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use croniq_auth::CallerType;
use croniq_auth::jwt::{JwtConfig, issue_token_pair};
use croniq_config::compile::{AlertsConfig, RuleConfig, RuleTrigger};
use croniq_runner::AppState;
use croniq_server::api::{ServerState, server_router};
use croniq_store::sqlite::SqliteStore;
use http_body_util::BodyExt;
use tokio::sync::mpsc;
use tower::util::ServiceExt;

const TEST_JWT_SECRET: &str = "alert-overrides-api-test-secret-please-do-not-use-in-prod";

fn rule(name: &str) -> RuleConfig {
    RuleConfig {
        name: name.into(),
        trigger: RuleTrigger::JobFailed,
        job_key_glob: "*".into(),
        min_attempts: 1,
        dead_letter_only: false,
        throttle: Some("10m".into()),
        expected_within: None,
        channels: vec!["ops".into()],
    }
}

fn make_state() -> Arc<ServerState> {
    let store = croniq_server::store::sqlite_store(SqliteStore::in_memory().unwrap());
    let runner = AppState::new();
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut state = ServerState::with_auth(
        runner,
        tx,
        Some(JwtConfig {
            secret: TEST_JWT_SECRET.into(),
            ..Default::default()
        }),
        Some(store),
    );
    {
        let s = Arc::get_mut(&mut state).unwrap();
        let mut alerts = AlertsConfig::default();
        alerts.rules.push(rule("billing-fail"));
        s.alerts = alerts;
    }
    state
}

fn token(state: &ServerState, scopes: &[&str]) -> String {
    let cfg = state.jwt_config.as_ref().unwrap();
    let scopes: Vec<String> = scopes.iter().map(|s| (*s).into()).collect();
    issue_token_pair(
        cfg,
        "test-user",
        "test-client",
        CallerType::User,
        Some("test-user"),
        Some(croniq_auth::Role::Admin),
        croniq_auth::AuthMethod::Password,
        &scopes,
    )
    .unwrap()
    .access_token
}

async fn send(
    app: axum::Router,
    method: &str,
    uri: &str,
    token: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, Vec<u8>) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("Authorization", format!("Bearer {token}"));
    let body = match body {
        Some(v) => {
            builder = builder.header("Content-Type", "application/json");
            Body::from(serde_json::to_vec(&v).unwrap())
        }
        None => Body::empty(),
    };
    let resp = app.oneshot(builder.body(body).unwrap()).await.unwrap();
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
async fn snooze_requires_write_scope() {
    let state = make_state();
    let app = server_router(Arc::clone(&state));
    // alerts:read is not enough for a mutation.
    let tok = token(&state, &["alerts:read"]);
    let (status, _) = send(
        app,
        "POST",
        "/v1/alerts/rules/billing-fail/snooze",
        &tok,
        Some(serde_json::json!({"until": "2026-06-04T16:00:00Z", "note": "x"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn snooze_without_note_is_400() {
    let state = make_state();
    let app = server_router(Arc::clone(&state));
    let tok = token(&state, &["alerts:write"]);
    let (status, _) = send(
        app,
        "POST",
        "/v1/alerts/rules/billing-fail/snooze",
        &tok,
        Some(serde_json::json!({"until": "2026-06-04T16:00:00Z", "note": "  "})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn snooze_unknown_rule_is_404() {
    let state = make_state();
    let app = server_router(Arc::clone(&state));
    let tok = token(&state, &["alerts:write"]);
    let (status, _) = send(
        app,
        "POST",
        "/v1/alerts/rules/does-not-exist/snooze",
        &tok,
        Some(serde_json::json!({"until": "2026-06-04T16:00:00Z", "note": "x"})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn snooze_sets_override_and_config_surfaces_it() {
    let state = make_state();
    let tok = token(&state, &["alerts:write", "alerts:read"]);

    // Set the snooze.
    let (status, body) = send(
        server_router(Arc::clone(&state)),
        "POST",
        "/v1/alerts/rules/billing-fail/snooze",
        &tok,
        Some(serde_json::json!({"until": "2026-06-04T16:00:00Z", "note": "maint window"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let ov: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(ov["rule_name"], "billing-fail");
    assert_eq!(ov["note"], "maint window");
    assert_eq!(ov["snooze_until"], "2026-06-04T16:00:00Z");
    // snooze auto-clears at the snooze end.
    assert_eq!(ov["expires_at"], "2026-06-04T16:00:00Z");
    assert_eq!(ov["set_by_user_id"], "test-user");

    // GET /v1/alerts/config surfaces it inline.
    let (status, body) = send(
        server_router(Arc::clone(&state)),
        "GET",
        "/v1/alerts/config",
        &tok,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let cfg: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(cfg["rules"].is_array(), "original shape preserved");
    let overrides = cfg["overrides"].as_array().unwrap();
    assert_eq!(overrides.len(), 1);
    assert_eq!(overrides[0]["rule_name"], "billing-fail");
}

#[tokio::test]
async fn disable_then_get_then_clear() {
    let state = make_state();
    let tok = token(&state, &["alerts:write", "alerts:read"]);

    let (status, body) = send(
        server_router(Arc::clone(&state)),
        "POST",
        "/v1/alerts/rules/billing-fail/disable",
        &tok,
        Some(serde_json::json!({"note": "debugging false positives"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let ov: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(ov["enabled"], false);

    // GET override returns it.
    let (status, _) = send(
        server_router(Arc::clone(&state)),
        "GET",
        "/v1/alerts/rules/billing-fail/override",
        &tok,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // DELETE clears it (204), second DELETE is 404.
    let (status, _) = send(
        server_router(Arc::clone(&state)),
        "DELETE",
        "/v1/alerts/rules/billing-fail/override",
        &tok,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = send(
        server_router(Arc::clone(&state)),
        "DELETE",
        "/v1/alerts/rules/billing-fail/override",
        &tok,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // GET override now 404.
    let (status, _) = send(
        server_router(Arc::clone(&state)),
        "GET",
        "/v1/alerts/rules/billing-fail/override",
        &tok,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn throttle_rejects_unparseable_duration() {
    let state = make_state();
    let app = server_router(Arc::clone(&state));
    let tok = token(&state, &["alerts:write"]);
    let (status, _) = send(
        app,
        "POST",
        "/v1/alerts/rules/billing-fail/throttle",
        &tok,
        Some(serde_json::json!({"throttle": "soon", "note": "too noisy"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn throttle_stores_parsed_seconds() {
    let state = make_state();
    let app = server_router(Arc::clone(&state));
    let tok = token(&state, &["alerts:write"]);
    let (status, body) = send(
        app,
        "POST",
        "/v1/alerts/rules/billing-fail/throttle",
        &tok,
        Some(serde_json::json!({"throttle": "30m", "note": "too noisy"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let ov: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(ov["throttle_secs"], 1800);
}

//! Integration tests for the read-only alerts API (issue #140 PR-5).
//!
//! Covers:
//!   * `GET /v1/alerts/config` — auth-gated, returns the merged
//!     config, never leaks `signing_key`.
//!   * `GET /v1/alerts/deliveries` — filter by job_key + state.
//!   * `GET /v1/alerts/deliveries/{id}` — single-row lookup, 404.

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use chrono::Utc;
use croniq_auth::CallerType;
use croniq_auth::jwt::{JwtConfig, issue_token_pair};
use croniq_config::compile::{AlertsConfig, ChannelConfig, ChannelKind};
use croniq_runner::AppState;
use croniq_server::api::{ServerState, server_router};
use croniq_store::models::{AlertDelivery, AlertDeliveryState};
use croniq_store::sqlite::SqliteStore;
use http_body_util::BodyExt;
use tokio::sync::mpsc;
use tower::util::ServiceExt;
use uuid::Uuid;

/// Insert the user the test tokens are minted for.
///
/// Since issue #431 the auth middleware checks every user-typed JWT against
/// `users.token_generation`, so a token naming a user that does not exist is
/// rejected — which is the point: a deleted user's tokens must stop working.
/// The fixture therefore has to create the user it authenticates as.
fn seed_user(store: &croniq_server::store::DynStore, user_id: &str, role: croniq_auth::Role) {
    let now = chrono::Utc::now();
    store
        .users_create(&croniq_store::models::User {
            user_id: user_id.to_string(),
            username: user_id.to_string(),
            email: None,
            display_name: None,
            role,
            is_active: true,
            created_at: now,
            updated_at: now,
            last_login_at: None,
        })
        .expect("seeding the test user cannot fail");
}

const TEST_JWT_SECRET: &str = "alerts-read-api-test-secret-please-do-not-use-in-prod";

/// Build a ServerState with auth + an SQLite store, a custom
/// AlertsConfig snapshot, and (optionally) some seeded delivery rows.
fn make_state(alerts: AlertsConfig, seeded: &[AlertDelivery]) -> Arc<ServerState> {
    let store = croniq_server::store::sqlite_store(SqliteStore::in_memory().unwrap());
    seed_user(
        &store,
        ("test-user", croniq_auth::Role::Viewer).0,
        ("test-user", croniq_auth::Role::Viewer).1,
    );
    for d in seeded {
        store.record_alert_delivery(d).unwrap();
    }
    let runner = AppState::new();
    let (tx, _rx) = mpsc::unbounded_channel();

    let mut state = ServerState::with_auth(
        runner,
        tx,
        Some(JwtConfig::new(TEST_JWT_SECRET)),
        Some(store),
    );
    {
        let s = Arc::get_mut(&mut state).unwrap();
        s.alerts = alerts;
    }
    state
}

fn token_with_scopes(state: &ServerState, scopes: &[&str]) -> String {
    let cfg = state.jwt_config.as_ref().unwrap();
    let scopes: Vec<String> = scopes.iter().map(|s| (*s).into()).collect();
    issue_token_pair(
        cfg,
        "test-user",
        "test-client",
        CallerType::User,
        Some("test-user"),
        Some(croniq_auth::Role::Viewer),
        croniq_auth::AuthMethod::Password,
        &scopes,
        None,
    )
    .unwrap()
    .access_token
}

async fn get_authenticated(app: axum::Router, uri: &str, token: &str) -> (StatusCode, Vec<u8>) {
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .header("Authorization", format!("Bearer {token}"))
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

fn shell_channel(name: &str, cmd: &str) -> (String, ChannelConfig) {
    (
        name.into(),
        ChannelConfig {
            name: name.into(),
            kind: ChannelKind::Shell {
                command: cmd.into(),
            },
        },
    )
}

fn webhook_channel(name: &str, url: &str, secret: Option<&str>) -> (String, ChannelConfig) {
    (
        name.into(),
        ChannelConfig {
            name: name.into(),
            kind: ChannelKind::Webhook {
                url: url.into(),
                signing_key: secret.map(String::from),
                timeout_secs: 5,
            },
        },
    )
}

fn delivery(id: &str, job_key: &str, rule: &str, state: AlertDeliveryState) -> AlertDelivery {
    AlertDelivery {
        delivery_id: id.into(),
        rule_name: rule.into(),
        channel_name: "ops".into(),
        job_key: job_key.into(),
        execution_id: Some(Uuid::new_v4().to_string()),
        state,
        error: if state == AlertDeliveryState::Failed {
            Some("simulated".into())
        } else {
            None
        },
        fired_at: Utc::now(),
        delivered_at: if state == AlertDeliveryState::Delivered {
            Some(Utc::now())
        } else {
            None
        },
    }
}

// ─── /v1/alerts/config ───────────────────────────────────────────────────────

#[tokio::test]
async fn config_requires_alerts_read_scope() {
    let state = make_state(AlertsConfig::default(), &[]);
    let app = server_router(Arc::clone(&state));

    // No scopes ⇒ 403 (require_scope rejects).
    let token = token_with_scopes(&state, &[]);
    let (status, _) = get_authenticated(app, "/v1/alerts/config", &token).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn config_returns_channels_and_rules() {
    let mut alerts = AlertsConfig::default();
    let (k, v) = shell_channel("ops", "/bin/true");
    alerts.channels.insert(k, v);
    let state = make_state(alerts, &[]);
    let app = server_router(Arc::clone(&state));

    let token = token_with_scopes(&state, &["alerts:read"]);
    let (status, body) = get_authenticated(app, "/v1/alerts/config", &token).await;
    assert_eq!(status, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(v["channels"]["ops"].is_object());
    assert_eq!(v["channels"]["ops"]["kind"]["type"], "shell");
    assert_eq!(
        v["channels"]["ops"]["kind"]["command"],
        serde_json::json!("/bin/true")
    );
}

#[tokio::test]
async fn config_strips_webhook_signing_key() {
    // The whole point of `#[serde(skip_serializing)]` on
    // ChannelKind::Webhook::signing_key — verify it never appears in
    // the JSON shape served at /v1/alerts/config.
    let mut alerts = AlertsConfig::default();
    let secret = "DO-NOT-LEAK-pwabc123";
    let (k, v) = webhook_channel("slack", "https://hooks.slack.com/x", Some(secret));
    alerts.channels.insert(k, v);
    let state = make_state(alerts, &[]);
    let app = server_router(Arc::clone(&state));

    let token = token_with_scopes(&state, &["alerts:read"]);
    let (status, body) = get_authenticated(app, "/v1/alerts/config", &token).await;
    assert_eq!(status, StatusCode::OK);
    let body_str = String::from_utf8(body).unwrap();
    assert!(
        !body_str.contains(secret),
        "HMAC secret must never appear in /v1/alerts/config response: {body_str}"
    );
    // But the channel + URL ARE visible — those aren't secrets.
    assert!(body_str.contains("slack"));
    assert!(body_str.contains("hooks.slack.com"));
    // And the variant kind is preserved so the UI can show the type.
    assert!(body_str.contains("\"webhook\""));
}

// ─── /v1/alerts/deliveries ───────────────────────────────────────────────────

#[tokio::test]
async fn deliveries_list_filters_by_job_key() {
    let seeded = vec![
        delivery(
            "d1",
            "billing:invoice",
            "fail",
            AlertDeliveryState::Delivered,
        ),
        delivery("d2", "ops:cleanup", "fail", AlertDeliveryState::Delivered),
        delivery("d3", "billing:invoice", "sla", AlertDeliveryState::Failed),
    ];
    let state = make_state(AlertsConfig::default(), &seeded);
    let app = server_router(Arc::clone(&state));
    let token = token_with_scopes(&state, &["alerts:read"]);

    let (status, body) =
        get_authenticated(app, "/v1/alerts/deliveries?job_key=billing:invoice", &token).await;
    assert_eq!(status, StatusCode::OK);
    let rows: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(rows.len(), 2, "two seeded billing rows");
    for r in &rows {
        assert_eq!(r["job_key"], "billing:invoice");
    }
}

#[tokio::test]
async fn deliveries_list_filters_by_state_string() {
    let seeded = vec![
        delivery("d1", "j", "r", AlertDeliveryState::Delivered),
        delivery("d2", "j", "r", AlertDeliveryState::Failed),
        delivery("d3", "j", "r", AlertDeliveryState::Throttled),
    ];
    let state = make_state(AlertsConfig::default(), &seeded);
    let app = server_router(Arc::clone(&state));
    let token = token_with_scopes(&state, &["alerts:read"]);

    let (status, body) = get_authenticated(app, "/v1/alerts/deliveries?state=failed", &token).await;
    assert_eq!(status, StatusCode::OK);
    let rows: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["state"], "failed");
}

#[tokio::test]
async fn deliveries_list_rejects_bad_state_filter() {
    let state = make_state(AlertsConfig::default(), &[]);
    let app = server_router(Arc::clone(&state));
    let token = token_with_scopes(&state, &["alerts:read"]);
    // `state=garbage` is invalid — must 400, not silently return all
    // rows (which would mislead the operator into thinking the
    // filter applied).
    let (status, _) = get_authenticated(app, "/v1/alerts/deliveries?state=garbage", &token).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn deliveries_list_caps_limit() {
    // Seed 600 rows; ask for limit=9999; expect at most 500 (the
    // server-side cap).
    let seeded: Vec<_> = (0..600)
        .map(|i| delivery(&format!("d{i:04}"), "j", "r", AlertDeliveryState::Delivered))
        .collect();
    let state = make_state(AlertsConfig::default(), &seeded);
    let app = server_router(Arc::clone(&state));
    let token = token_with_scopes(&state, &["alerts:read"]);

    let (status, body) = get_authenticated(app, "/v1/alerts/deliveries?limit=9999", &token).await;
    assert_eq!(status, StatusCode::OK);
    let rows: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert!(rows.len() <= 500, "limit cap: got {}", rows.len());
}

#[tokio::test]
async fn deliveries_list_requires_scope() {
    let state = make_state(AlertsConfig::default(), &[]);
    let app = server_router(Arc::clone(&state));
    let token = token_with_scopes(&state, &[]);
    let (status, _) = get_authenticated(app, "/v1/alerts/deliveries", &token).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

// ─── /v1/alerts/deliveries/{id} ──────────────────────────────────────────────

#[tokio::test]
async fn delivery_get_by_id_returns_row() {
    let d = delivery("known-id-123", "j:k", "r", AlertDeliveryState::Delivered);
    let state = make_state(AlertsConfig::default(), &[d]);
    let app = server_router(Arc::clone(&state));
    let token = token_with_scopes(&state, &["alerts:read"]);

    let (status, body) = get_authenticated(app, "/v1/alerts/deliveries/known-id-123", &token).await;
    assert_eq!(status, StatusCode::OK);
    let row: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(row["delivery_id"], "known-id-123");
    assert_eq!(row["job_key"], "j:k");
}

#[tokio::test]
async fn delivery_get_unknown_id_404s() {
    let state = make_state(AlertsConfig::default(), &[]);
    let app = server_router(Arc::clone(&state));
    let token = token_with_scopes(&state, &["alerts:read"]);
    let (status, _) = get_authenticated(app, "/v1/alerts/deliveries/missing-id", &token).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

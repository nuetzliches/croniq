//! Integration tests for per-job alert acknowledgements.
//!
//! Covers the HTTP surface:
//!   * `alerts:ack` to set or clear, `alerts:read` to look (403 otherwise)
//!   * a note is required, the deadline defaults and is capped (400)
//!   * an unknown job is 404, a job whose latest run did not fail is 409
//!   * acknowledging again replaces, DELETE ends it, both audit-logged
//!
//! What an acknowledgement does to alerts is tested next to the evaluator
//! (`alerts.rs`), the completion processor and the watchdog.

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use chrono::{Duration, Utc};
use croniq_auth::CallerType;
use croniq_auth::jwt::{JwtConfig, issue_token_pair};
use croniq_runner::AppState;
use croniq_server::api::{ServerState, server_router};
use croniq_server::store::DynStore;
use croniq_store::models::{AuditFilter, Execution, ExecutionState, JobDefinition};
use croniq_store::sqlite::SqliteStore;
use http_body_util::BodyExt;
use tokio::sync::mpsc;
use tower::util::ServiceExt;
use uuid::Uuid;

const TEST_JWT_SECRET: &str = "job-alert-ack-api-test-secret-please-do-not-use-in-prod";

fn seed_user(store: &DynStore) {
    let now = Utc::now();
    store
        .users_create(&croniq_store::models::User {
            user_id: "op-1".into(),
            username: "op-1".into(),
            email: None,
            display_name: Some("Olga Operator".into()),
            role: croniq_auth::Role::Operator,
            is_active: true,
            created_at: now,
            updated_at: now,
            last_login_at: None,
        })
        .unwrap();
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

/// A finished run of `job_key` in `state`, `minutes_ago` old.
fn seed_run(store: &DynStore, job_key: &str, state: ExecutionState, minutes_ago: i64) {
    let at = Utc::now() - Duration::minutes(minutes_ago);
    let id = Uuid::new_v4();
    store
        .create_execution(&Execution {
            id,
            job_key: job_key.into(),
            fire_at: at,
            scheduled_for: at,
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
            metadata: Default::default(),
            created_at: at,
        })
        .unwrap();
    store.claim_execution(id, "runner-1", at).unwrap();
    store
        .complete_execution(id, Some("runner-1"), state, Some(10), None, None, at)
        .unwrap();
}

fn make_state() -> (Arc<ServerState>, DynStore) {
    let store = croniq_server::store::sqlite_store(SqliteStore::in_memory().unwrap());
    seed_user(&store);
    seed_job(&store, "billing:sync");
    seed_job(&store, "billing:healthy");
    seed_run(&store, "billing:sync", ExecutionState::Completed, 30);
    seed_run(&store, "billing:sync", ExecutionState::Dead, 5);
    seed_run(&store, "billing:healthy", ExecutionState::Failed, 30);
    seed_run(&store, "billing:healthy", ExecutionState::Completed, 5);
    let (tx, _rx) = mpsc::unbounded_channel();
    let state = ServerState::with_auth(
        AppState::new(),
        tx,
        Some(JwtConfig::new(TEST_JWT_SECRET)),
        Some(Arc::clone(&store)),
    );
    (state, store)
}

fn token(state: &ServerState, scopes: &[&str]) -> String {
    let cfg = state.jwt_config.as_ref().unwrap();
    let scopes: Vec<String> = scopes.iter().map(|s| (*s).into()).collect();
    issue_token_pair(
        cfg,
        "op-1",
        "op-1",
        CallerType::User,
        Some("op-1"),
        Some(croniq_auth::Role::Operator),
        croniq_auth::AuthMethod::Password,
        &scopes,
        None,
    )
    .unwrap()
    .access_token
}

async fn send(
    state: &Arc<ServerState>,
    method: &str,
    uri: &str,
    token: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
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
    let resp = server_router(Arc::clone(state))
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

#[tokio::test]
async fn acknowledging_needs_alerts_ack() {
    let (state, _) = make_state();
    let tok = token(&state, &["alerts:read", "notes:write"]);
    let (status, _) = send(
        &state,
        "POST",
        "/v1/jobs/billing:sync/alert-ack",
        &tok,
        Some(serde_json::json!({"note": "provider down"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn an_acknowledgement_needs_a_note_and_a_near_deadline() {
    let (state, _) = make_state();
    let tok = token(&state, &["alerts:ack"]);
    let uri = "/v1/jobs/billing:sync/alert-ack";

    for body in [
        serde_json::json!({}),
        serde_json::json!({"note": "   "}),
        serde_json::json!({"note": "x", "until": Utc::now() - Duration::minutes(1)}),
        serde_json::json!({"note": "x", "until": Utc::now() + Duration::days(8)}),
    ] {
        let (status, json) = send(&state, "POST", uri, &tok, Some(body.clone())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(json["error"], "invalid_ack");
    }
}

#[tokio::test]
async fn only_a_failing_job_can_be_acknowledged() {
    let (state, _) = make_state();
    let tok = token(&state, &["alerts:ack"]);
    let body = serde_json::json!({"note": "provider down"});

    let (status, _) = send(
        &state,
        "POST",
        "/v1/jobs/nope:missing/alert-ack",
        &tok,
        Some(body.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, json) = send(
        &state,
        "POST",
        "/v1/jobs/billing:healthy/alert-ack",
        &tok,
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(json["error"], "job_not_failing");
}

#[tokio::test]
async fn acknowledge_read_replace_and_clear() {
    let (state, store) = make_state();
    let ack = token(&state, &["alerts:ack"]);
    let read = token(&state, &["alerts:read"]);
    let uri = "/v1/jobs/billing:sync/alert-ack";

    let (status, _) = send(&state, "GET", uri, &read, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let before = Utc::now();
    let (status, json) = send(
        &state,
        "POST",
        uri,
        &ack,
        Some(serde_json::json!({"note": "  provider down, ETA unknown "})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["job_key"], "billing:sync");
    assert_eq!(json["note"], "provider down, ETA unknown");
    assert_eq!(json["acked_by"], "op-1");
    assert_eq!(json["acked_by_name"], "Olga Operator");
    assert_eq!(json["suppressed_count"], 0);
    let expires: chrono::DateTime<Utc> =
        serde_json::from_value(json["expires_at"].clone()).unwrap();
    assert!(expires >= before + Duration::hours(72), "defaults to 72h");
    assert!(expires <= Utc::now() + Duration::hours(72));

    let (status, json) = send(&state, "GET", uri, &read, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["note"], "provider down, ETA unknown");

    // Again: replaced, with the deadline asked for.
    let until = Utc::now() + Duration::hours(6);
    let (status, json) = send(
        &state,
        "POST",
        uri,
        &ack,
        Some(serde_json::json!({"note": "still down", "until": until})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["note"], "still down");
    let (_, list) = send(&state, "GET", "/v1/alerts/acks", &read, None).await;
    assert_eq!(list.as_array().unwrap().len(), 1);

    // Reading does not let you clear.
    let (status, _) = send(&state, "DELETE", uri, &read, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = send(&state, "DELETE", uri, &ack, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(&state, "DELETE", uri, &ack, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(store.get_job_alert_ack("billing:sync").unwrap().is_none());

    let actions: Vec<String> = store
        .audit_list(&AuditFilter {
            target_id: Some("billing:sync".into()),
            ..Default::default()
        })
        .unwrap()
        .into_iter()
        .map(|e| e.action)
        .collect();
    assert_eq!(actions.iter().filter(|a| *a == "alerts.ack.set").count(), 2);
    assert_eq!(
        actions
            .iter()
            .filter(|a| *a == "alerts.ack.cleared")
            .count(),
        1
    );
}

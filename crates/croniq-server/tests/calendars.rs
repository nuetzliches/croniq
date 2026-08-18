//! End-to-end tests for the `/v1/calendars` endpoints.
//!
//! Phase 1: DSL calendars are surfaced as read-only with `managed_by="dsl"`
//! and a synthetic `dsl:{name}` ID. Mutations on DSL-managed calendars return
//! 409 Conflict; API-managed calendars stay editable.

use std::sync::Arc;
use std::time::Duration;

use axum::{body::Body, http::Request};
use croniq_runner::AppState;
use croniq_server::{
    api::{ServerState, server_router},
    loader::load_str,
    store::{DynStore, sqlite_store},
};
use croniq_store::sqlite::SqliteStore;
use http_body_util::BodyExt;
use tokio::sync::{RwLock, mpsc};
use tower::util::ServiceExt;

// ─── Auth fixtures (issue #431) ───────────────────────────────────────────────
//
// `require_auth` fails closed: a ServerState without a jwt_config rejects every
// authenticated route with 401 rather than injecting a synthetic admin caller.
// These tests therefore configure a real signing key and send a real admin
// token, so they exercise the middleware instead of bypassing it.

const TEST_JWT_SECRET: &str = "calendars-integration-test-secret";

fn test_jwt() -> croniq_auth::jwt::JwtConfig {
    croniq_auth::jwt::JwtConfig::new(TEST_JWT_SECRET)
}

fn admin_bearer() -> String {
    let pair = croniq_auth::jwt::issue_token_pair(
        &test_jwt(),
        "test-admin",
        "test-admin",
        // API-client shaped: a user token is checked against
        // users.token_generation on every request (issue #431), which would
        // mean seeding a user row into every fixture here.
        croniq_auth::CallerType::ApiKey,
        None,
        None,
        croniq_auth::AuthMethod::ApiKey,
        &[croniq_auth::context::Scope::ADMIN.to_string()],
        None,
    )
    .expect("minting a test admin token cannot fail");
    format!("Bearer {}", pair.access_token)
}

const DSL_WITH_CALENDAR: &str = r#"
calendar business-days {
  timezone Europe/Vienna
  include weekly monday tuesday wednesday thursday friday
  exclude annual 12-25 12-26
}

job billing:invoice {
  every weekday at 02:00 { calendar business-days }
}
"#;

fn build_state(src: &str) -> Arc<ServerState> {
    build_state_with_policy(src, false)
}

fn build_state_with_policy(src: &str, adopt_on_mutate: bool) -> Arc<ServerState> {
    let loaded = load_str(src).unwrap();
    let store: DynStore = sqlite_store(SqliteStore::in_memory().unwrap());
    let runner = AppState::new();
    let (tx, _rx) = mpsc::unbounded_channel();

    let dsl_jobs = Arc::new(RwLock::new(loaded.runtime.jobs.clone()));
    let dsl_calendars = Arc::new(RwLock::new(loaded.runtime.calendars.clone()));

    let mut state = ServerState::with_timeout(runner, tx, Duration::from_millis(50));
    {
        let s = Arc::get_mut(&mut state).unwrap();
        s.jwt_config = Some(test_jwt());
        s.store = Some(store);
        s.dsl_jobs = Some(dsl_jobs);
        s.dsl_calendars = Some(dsl_calendars);
        s.policy_dsl_adopt_on_mutate
            .store(adopt_on_mutate, std::sync::atomic::Ordering::Relaxed);
    }
    state
}

async fn get_json(app: axum::Router, uri: &str) -> (axum::http::StatusCode, serde_json::Value) {
    let resp = app
        .oneshot(
            Request::builder()
                .header("authorization", admin_bearer())
                .method("GET")
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let value: serde_json::Value = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    };
    (status, value)
}

async fn send_json(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: serde_json::Value,
) -> (axum::http::StatusCode, serde_json::Value) {
    let resp = app
        .oneshot(
            Request::builder()
                .header("authorization", admin_bearer())
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let value: serde_json::Value = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    };
    (status, value)
}

#[tokio::test]
async fn dsl_calendar_appears_in_list_with_dsl_managed_by() {
    let state = build_state(DSL_WITH_CALENDAR);
    let app = server_router(Arc::clone(&state));

    let (status, body) = get_json(app, "/v1/calendars").await;
    assert_eq!(status, 200);

    let arr = body.as_array().expect("array");
    assert_eq!(arr.len(), 1, "exactly one DSL calendar surfaces");
    let cal = &arr[0];
    assert_eq!(cal["name"], "business-days");
    assert_eq!(cal["managed_by"], "dsl");
    assert_eq!(cal["calendar_id"], "dsl:business-days");
    assert_eq!(cal["timezone"], "Europe/Vienna");
    let rules = cal["rules"].as_str().expect("rules string");
    // Rules are re-emitted in canonical `croniq fmt` form: the five
    // spelled-out days collapse to the `weekday` alias.
    assert!(rules.contains("include weekly weekday"), "got: {rules}");
    assert!(rules.contains("exclude annual 12-25"), "got: {rules}");
}

#[tokio::test]
async fn dsl_calendar_addressable_by_synthetic_id() {
    let state = build_state(DSL_WITH_CALENDAR);
    let app = server_router(Arc::clone(&state));

    let (status, body) = get_json(app, "/v1/calendars/dsl:business-days").await;
    assert_eq!(status, 200);
    assert_eq!(body["name"], "business-days");
    assert_eq!(body["managed_by"], "dsl");
}

#[tokio::test]
async fn put_on_dsl_calendar_id_returns_409() {
    let state = build_state(DSL_WITH_CALENDAR);
    let app = server_router(Arc::clone(&state));

    let (status, body) = send_json(
        app,
        "PUT",
        "/v1/calendars/dsl:business-days",
        serde_json::json!({"name": "renamed"}),
    )
    .await;
    assert_eq!(status, 409);
    assert_eq!(body["error"], "dsl_managed");
}

#[tokio::test]
async fn delete_on_dsl_calendar_id_returns_409() {
    let state = build_state(DSL_WITH_CALENDAR);
    let app = server_router(Arc::clone(&state));

    let (status, body) = send_json(
        app,
        "DELETE",
        "/v1/calendars/dsl:business-days",
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(status, 409);
    assert_eq!(body["error"], "dsl_managed");
}

#[tokio::test]
async fn create_with_dsl_name_returns_409() {
    let state = build_state(DSL_WITH_CALENDAR);
    let app = server_router(Arc::clone(&state));

    let (status, body) = send_json(
        app,
        "POST",
        "/v1/calendars",
        serde_json::json!({
            "name": "business-days",
            "timezone": "UTC",
            "rules": "include weekly monday"
        }),
    )
    .await;
    assert_eq!(status, 409);
    assert_eq!(body["error"], "dsl_managed");
}

#[tokio::test]
async fn dsl_and_api_calendars_coexist_in_list() {
    let state = build_state(DSL_WITH_CALENDAR);
    let app = server_router(Arc::clone(&state));

    // Create an API calendar with a different name.
    let (status, _body) = send_json(
        app,
        "POST",
        "/v1/calendars",
        serde_json::json!({
            "name": "api-holidays",
            "timezone": "UTC",
            "rules": "exclude annual 01-01"
        }),
    )
    .await;
    assert_eq!(status, 201);

    let app2 = server_router(Arc::clone(&state));
    let (status, body) = get_json(app2, "/v1/calendars").await;
    assert_eq!(status, 200);
    let arr = body.as_array().expect("array");
    assert_eq!(arr.len(), 2);
    let dsl = arr
        .iter()
        .find(|c| c["name"] == "business-days")
        .expect("dsl present");
    let api = arr
        .iter()
        .find(|c| c["name"] == "api-holidays")
        .expect("api present");
    assert_eq!(dsl["managed_by"], "dsl");
    assert_eq!(api["managed_by"], "api");
}

#[tokio::test]
async fn api_calendar_with_dsl_name_collision_dsl_wins() {
    // Even if a stray API calendar exists with the same name as a DSL
    // calendar (e.g. created before the DSL block was added), the list
    // response shows the DSL entry. This protects the scheduler-resolved
    // identity from drifting under the user's feet.
    let state = build_state(DSL_WITH_CALENDAR);

    // Manually persist a colliding API calendar into the store.
    let store = state.store.as_ref().unwrap();
    store
        .create_calendar(&croniq_store::models::CalendarDefinition {
            calendar_id: "manual-collision".into(),
            name: "business-days".into(),
            timezone: Some("UTC".into()),
            rules: "include weekly tuesday".into(),
            managed_by: "api".into(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        })
        .unwrap();

    let app = server_router(Arc::clone(&state));
    let (status, body) = get_json(app, "/v1/calendars").await;
    assert_eq!(status, 200);
    let arr = body.as_array().expect("array");
    assert_eq!(arr.len(), 1, "DSL precedence drops the API duplicate");
    assert_eq!(arr[0]["managed_by"], "dsl");
    assert_eq!(arr[0]["calendar_id"], "dsl:business-days");
}

#[tokio::test]
async fn api_calendar_unaffected_by_dsl_protection() {
    let state = build_state(""); // no DSL calendars
    let app = server_router(Arc::clone(&state));

    let (status, created) = send_json(
        app,
        "POST",
        "/v1/calendars",
        serde_json::json!({
            "name": "team-holidays",
            "timezone": "UTC",
            "rules": "exclude annual 01-01"
        }),
    )
    .await;
    assert_eq!(status, 201);
    let id = created["calendar_id"].as_str().unwrap().to_string();

    let app2 = server_router(Arc::clone(&state));
    let (status, _) = send_json(
        app2,
        "PUT",
        &format!("/v1/calendars/{id}"),
        serde_json::json!({"name": "renamed"}),
    )
    .await;
    assert_eq!(status, 200, "API-managed calendars stay editable");

    let app3 = server_router(Arc::clone(&state));
    let (status, _) = send_json(
        app3,
        "DELETE",
        &format!("/v1/calendars/{id}"),
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(status, 204, "API-managed calendars stay deletable");
}

// ─── Phase 2: adoption flow ───────────────────────────────────────────────────

#[tokio::test]
async fn adopt_returns_409_when_policy_off() {
    let state = build_state(DSL_WITH_CALENDAR); // policy default = false
    let app = server_router(Arc::clone(&state));
    let (status, body) = send_json(
        app,
        "POST",
        "/v1/calendars/dsl:business-days/adopt",
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(status, 409);
    assert_eq!(body["error"], "adoption_disabled");
}

#[tokio::test]
async fn adopt_succeeds_when_policy_on() {
    let state = build_state_with_policy(DSL_WITH_CALENDAR, true);
    let app = server_router(Arc::clone(&state));
    let (status, body) = send_json(
        app,
        "POST",
        "/v1/calendars/dsl:business-days/adopt",
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(status, 200);
    let cal = &body["calendar"];
    assert_eq!(cal["name"], "business-days");
    assert_eq!(cal["managed_by"], "api");
    assert_ne!(cal["calendar_id"], "dsl:business-days");
    assert_eq!(body["dsl_key"], "business-days");

    // List now shows only the API row.
    let app2 = server_router(Arc::clone(&state));
    let (status, list) = get_json(app2, "/v1/calendars").await;
    assert_eq!(status, 200);
    let arr = list.as_array().expect("array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["managed_by"], "api");

    // Store-level adoption record exists.
    let store = state.store.as_ref().unwrap();
    assert!(store.is_adopted("calendar", "business-days").unwrap());
}

#[tokio::test]
async fn unadopt_restores_dsl_visibility() {
    let state = build_state_with_policy(DSL_WITH_CALENDAR, true);

    // Adopt first.
    let app = server_router(Arc::clone(&state));
    let (status, body) = send_json(
        app,
        "POST",
        "/v1/calendars/dsl:business-days/adopt",
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(status, 200);
    let api_id = body["calendar"]["calendar_id"]
        .as_str()
        .unwrap()
        .to_string();

    // Unadopt by API UUID.
    let app2 = server_router(Arc::clone(&state));
    let (status, _) = send_json(
        app2,
        "POST",
        &format!("/v1/calendars/{api_id}/unadopt"),
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(status, 204);

    // The dsl_adoptions row + the API copy are gone.
    let store = state.store.as_ref().unwrap();
    assert!(!store.is_adopted("calendar", "business-days").unwrap());
    assert!(store.get_calendar(&api_id).unwrap().is_none());

    // The DSL snapshot in ServerState would be repopulated by the next
    // reload. For this unit-style test, manually push the DSL entry back
    // (mirroring what apply_plan_direct does) and verify it surfaces.
    {
        let dsl = state.dsl_calendars.as_ref().unwrap();
        let mut guard = dsl.write().await;
        guard.push(croniq_config::compile::CalendarConfig {
            name: "business-days".into(),
            timezone: Some("Europe/Vienna".into()),
            rules: vec![],
        });
    }

    let app3 = server_router(Arc::clone(&state));
    let (_status, list) = get_json(app3, "/v1/calendars").await;
    let arr = list.as_array().expect("array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["managed_by"], "dsl");
    assert_eq!(arr[0]["calendar_id"], "dsl:business-days");
}

#[tokio::test]
async fn adopt_rejects_non_dsl_id() {
    let state = build_state_with_policy(DSL_WITH_CALENDAR, true);
    let app = server_router(Arc::clone(&state));
    let (status, body) = send_json(
        app,
        "POST",
        "/v1/calendars/abc-not-a-dsl-id/adopt",
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(status, 400);
    assert_eq!(body["error"], "not_dsl_id");
}

#[tokio::test]
async fn unadopt_rejects_non_adopted_calendar() {
    let state = build_state_with_policy("", true); // no DSL
    let app = server_router(Arc::clone(&state));

    // Create a regular API calendar, not adopted from DSL.
    let (status, created) = send_json(
        app,
        "POST",
        "/v1/calendars",
        serde_json::json!({"name": "regular", "timezone": "UTC", "rules": ""}),
    )
    .await;
    assert_eq!(status, 201);
    let id = created["calendar_id"].as_str().unwrap().to_string();

    let app2 = server_router(Arc::clone(&state));
    let (status, body) = send_json(
        app2,
        "POST",
        &format!("/v1/calendars/{id}/unadopt"),
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(status, 409);
    assert_eq!(body["error"], "not_adopted");
}

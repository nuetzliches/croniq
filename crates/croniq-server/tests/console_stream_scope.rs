//! Scope enforcement for the Live Console stream, `GET /v1/events/stream`.
//!
//! The stream carries the raw server tracing feed — audit lines, auth
//! failure detail, job stderr — plus a replay buffer of recent events, so
//! it is an admin-only surface. Role defaults hand `executions:read` to
//! every viewer, which must NOT be enough to open it.

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use croniq_auth::CallerType;
use croniq_auth::context::default_scopes_for_role;
use croniq_auth::jwt::{JwtConfig, issue_token_pair};
use croniq_runner::AppState;
use croniq_server::api::{ServerState, server_router};
use croniq_server::live_console::ConsoleHub;
use croniq_store::models::Role;
use croniq_store::sqlite::SqliteStore;
use tokio::sync::mpsc;
use tower::util::ServiceExt;

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

const TEST_JWT_SECRET: &str = "console-stream-scope-test-secret-please-do-not-use-in-prod";

fn make_state() -> Arc<ServerState> {
    let store = croniq_server::store::sqlite_store(SqliteStore::in_memory().unwrap());
    seed_user(
        &store,
        ("test-user", croniq_auth::Role::Admin).0,
        ("test-user", croniq_auth::Role::Admin).1,
    );
    let runner = AppState::new();
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut state = ServerState::with_auth(
        runner,
        tx,
        Some(JwtConfig::new(TEST_JWT_SECRET)),
        Some(store),
    );
    // Without a hub the handler answers 503, which would mask whether the
    // scope check passed — wire one up so the admin case reaches 200.
    Arc::get_mut(&mut state).unwrap().console_hub = Some(ConsoleHub::new());
    state
}

fn token(state: &ServerState, role: Role) -> String {
    let cfg = state.jwt_config.as_ref().unwrap();
    issue_token_pair(
        cfg,
        "test-user",
        "test-client",
        CallerType::User,
        Some("test-user"),
        Some(role),
        croniq_auth::AuthMethod::Password,
        &default_scopes_for_role(role),
        None,
    )
    .unwrap()
    .access_token
}

/// Status only — the SSE body is an endless stream, so it is never collected.
async fn stream_status(app: axum::Router, token: &str) -> StatusCode {
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/events/stream?snapshot=0")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    resp.status()
}

#[tokio::test]
async fn viewer_role_cannot_open_the_console_stream() {
    let state = make_state();
    let tok = token(&state, Role::Viewer);
    // Guard the premise: the viewer default set really does carry
    // `executions:read` and really does not carry `admin`.
    let scopes = default_scopes_for_role(Role::Viewer);
    assert!(scopes.iter().any(|s| s == "executions:read"));
    assert!(!scopes.iter().any(|s| s == "admin"));

    let status = stream_status(server_router(Arc::clone(&state)), &tok).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn operator_role_cannot_open_the_console_stream() {
    let state = make_state();
    let tok = token(&state, Role::Operator);
    let status = stream_status(server_router(Arc::clone(&state)), &tok).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn admin_role_can_open_the_console_stream() {
    let state = make_state();
    let tok = token(&state, Role::Admin);
    let status = stream_status(server_router(Arc::clone(&state)), &tok).await;
    assert_eq!(status, StatusCode::OK);
}

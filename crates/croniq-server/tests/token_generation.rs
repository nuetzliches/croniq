//! Access tokens do not survive a credential change (issue #431).
//!
//! Access tokens are stateless JWTs valid until `exp`, up to an hour. Refresh
//! was already blocked after a deactivation — it re-checks `is_active` — but
//! access tokens minted beforehand kept working, so "I reset the password to
//! lock an attacker out" did not hold for up to an hour.
//!
//! `users.token_generation` closes that: it is stamped into every access token
//! as a claim, bumped on password change / password reset / deactivation, and
//! compared against the row on every authenticated request.
//!
//! These tests drive the real router end-to-end — log in, use the token,
//! change the credential, use the same token again.

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
use croniq_server::store::DynStore;
use croniq_store::models::{PasswordCredential, Role, User};
use croniq_store::sqlite::SqliteStore;
use http_body_util::BodyExt;
use tokio::sync::mpsc;
use tower::util::ServiceExt;

const PASSWORD: &str = "correct-horse-battery";
const NEW_PASSWORD: &str = "battery-staple-horse";
const JWT_SECRET: &str = "token-generation-test-secret";
const USER_ID: &str = "u-admin";

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

    let (tx, _rx) = mpsc::unbounded_channel();
    let state = ServerState::with_auth(
        AppState::new(),
        tx,
        Some(JwtConfig::new(JWT_SECRET)),
        Some(store.clone()),
    );
    (server_router(state), store)
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

async fn get_status(app: axum::Router, uri: &str, token: &str) -> StatusCode {
    app.oneshot(
        Request::builder()
            .method("GET")
            .uri(uri)
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .unwrap()
    .status()
}

async fn login(app: axum::Router, password: &str) -> String {
    let (status, body) = post(
        app,
        "/v1/auth/login",
        None,
        serde_json::json!({ "username": "admin", "password": password }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "login failed: {body}");
    body["access_token"]
        .as_str()
        .expect("login returns an access token")
        .to_string()
}

#[tokio::test]
async fn a_password_change_invalidates_existing_access_tokens() {
    let (app, _store) = fixture();
    let token = login(app.clone(), PASSWORD).await;

    // The freshly minted token works.
    assert_eq!(
        get_status(app.clone(), "/v1/users", &token).await,
        StatusCode::OK
    );

    let (status, body) = post(
        app.clone(),
        "/v1/users/me/change-password",
        Some(&token),
        serde_json::json!({
            "old_password": PASSWORD,
            "new_password": NEW_PASSWORD,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "change-password: {body}");

    // Same token, one request later: the generation it was minted under is
    // superseded. Before #431 this stayed 200 until the token expired.
    assert_eq!(
        get_status(app.clone(), "/v1/users", &token).await,
        StatusCode::UNAUTHORIZED,
        "an access token minted before the password change must stop working"
    );

    // …and a fresh login under the new password works, at the new generation.
    let new_token = login(app.clone(), NEW_PASSWORD).await;
    assert_eq!(
        get_status(app, "/v1/users", &new_token).await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn deactivating_a_user_invalidates_their_access_tokens() {
    let (app, store) = fixture();
    // A second admin, so deactivating the first does not trip the
    // last-admin guard.
    let now = Utc::now();
    store
        .users_create(&User {
            user_id: "u-other".into(),
            username: "other".into(),
            email: None,
            display_name: None,
            role: Role::Admin,
            is_active: true,
            created_at: now,
            updated_at: now,
            last_login_at: None,
        })
        .unwrap();

    let token = login(app.clone(), PASSWORD).await;
    assert_eq!(
        get_status(app.clone(), "/v1/users", &token).await,
        StatusCode::OK
    );

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/v1/users/{USER_ID}"))
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(
                    serde_json::json!({ "is_active": false }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    assert_eq!(
        get_status(app, "/v1/users", &token).await,
        StatusCode::UNAUTHORIZED,
        "a deactivated user's access token must stop working immediately"
    );
}

#[tokio::test]
async fn a_profile_edit_does_not_sign_the_user_out() {
    // Bumping the generation signs someone out, which is a real cost. It is
    // spent on credential changes and deactivation, not on editing a display
    // name — this pins that boundary.
    let (app, _store) = fixture();
    let token = login(app.clone(), PASSWORD).await;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/v1/users/me")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(
                    serde_json::json!({ "display_name": "Renamed" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    assert_eq!(
        get_status(app, "/v1/users", &token).await,
        StatusCode::OK,
        "editing a profile field must not invalidate the caller's own session"
    );
}

#[tokio::test]
async fn a_token_naming_a_deleted_user_is_rejected() {
    let (app, store) = fixture();
    let now = Utc::now();
    store
        .users_create(&User {
            user_id: "u-other".into(),
            username: "other".into(),
            email: None,
            display_name: None,
            role: Role::Admin,
            is_active: true,
            created_at: now,
            updated_at: now,
            last_login_at: None,
        })
        .unwrap();

    let token = login(app.clone(), PASSWORD).await;
    store.users_delete(USER_ID).unwrap();

    // No row means no generation to match, which is the same answer as a
    // superseded one: the credential is gone.
    assert_eq!(
        get_status(app, "/v1/users", &token).await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn tokens_minted_before_the_upgrade_still_validate() {
    // Rolling-restart guarantee: a token issued by an older binary carries no
    // `token_generation` claim at all, and every existing user row was
    // backfilled to 0. Those must keep working until the account's first bump,
    // or an upgrade would sign the entire estate out at once.
    let (app, _store) = fixture();

    let legacy = croniq_auth::jwt::issue_token_pair(
        &JwtConfig::new(JWT_SECRET),
        USER_ID,
        USER_ID,
        croniq_auth::CallerType::User,
        Some(USER_ID),
        Some(Role::Admin),
        croniq_auth::AuthMethod::Password,
        &["admin".to_string()],
        // The pre-#431 shape: no claim.
        None,
    )
    .unwrap()
    .access_token;

    assert_eq!(
        get_status(app, "/v1/users", &legacy).await,
        StatusCode::OK,
        "a claimless token must read as generation 0 and still validate"
    );
}

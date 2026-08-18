//! The refresh token lives in an `HttpOnly` cookie, not in `localStorage`
//! (issue #454).
//!
//! Two delivery modes share one endpoint set, and the whole security argument
//! rests on them staying disjoint:
//!
//! * **Body** — every client that existed before #454. `refresh_token` in the
//!   login response, `refresh_token` in the refresh request. Untouched.
//! * **Cookie** — the dashboard SPA opts in with `refresh_cookie: true`. The
//!   token goes out as `Set-Cookie` and never appears in a response body
//!   again, so an XSS that POSTs to `/v1/auth/refresh` (with the browser
//!   attaching the cookie for it) learns an access token it could have had
//!   anyway, and nothing durable.
//!
//! These tests drive the real router end-to-end and assert on the actual
//! `Set-Cookie` header, because every attribute on it is load-bearing.

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
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
const JWT_SECRET: &str = "refresh-cookie-test-secret";
const USER_ID: &str = "u-admin";
const ORIGIN: &str = "https://cron.example.com";
const HOST: &str = "cron.example.com";

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

struct Reply {
    status: StatusCode,
    body: serde_json::Value,
    set_cookie: Option<String>,
}

impl Reply {
    /// The `croniq_refresh` value out of `Set-Cookie`, if one was set to a
    /// non-empty value.
    fn cookie_value(&self) -> Option<String> {
        let raw = self.set_cookie.as_deref()?;
        let v = raw
            .split(';')
            .next()?
            .trim()
            .strip_prefix("croniq_refresh=")?;
        (!v.is_empty()).then(|| v.to_string())
    }
}

/// POST as the dashboard SPA would: same-origin `Origin`, optional cookie.
async fn post(
    app: axum::Router,
    uri: &str,
    body: serde_json::Value,
    cookie: Option<&str>,
    browser: bool,
) -> Reply {
    let mut req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .header("host", HOST);
    if browser {
        req = req.header("origin", ORIGIN);
    }
    if let Some(c) = cookie {
        req = req.header("cookie", format!("croniq_refresh={c}"));
    }
    let resp = app
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let set_cookie = resp
        .headers()
        .get(header::SET_COOKIE)
        .map(|v| v.to_str().unwrap().to_string());
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    };
    Reply {
        status,
        body,
        set_cookie,
    }
}

async fn login_with_cookie(app: axum::Router) -> Reply {
    let reply = post(
        app,
        "/v1/auth/login",
        serde_json::json!({
            "username": "admin",
            "password": PASSWORD,
            "refresh_cookie": true,
        }),
        None,
        true,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "login failed: {}", reply.body);
    reply
}

#[tokio::test]
async fn cookie_login_sets_an_httponly_cookie_and_no_body_token() {
    let (app, _store) = fixture();
    let reply = login_with_cookie(app).await;

    assert!(
        reply.body["access_token"].is_string(),
        "the access token still comes back in the body — it is what the SPA \
         keeps in memory: {}",
        reply.body
    );
    assert!(
        reply.body.get("refresh_token").is_none(),
        "the refresh token must not appear in the body in cookie mode: {}",
        reply.body
    );

    let cookie = reply.set_cookie.expect("cookie mode sets a cookie");
    assert!(cookie.starts_with("croniq_refresh="), "{cookie}");
    assert!(
        cookie.contains("HttpOnly"),
        "HttpOnly is the entire point — without it the token is readable by \
         any injected script: {cookie}"
    );
    assert!(
        cookie.contains("SameSite=Strict"),
        "SameSite=Strict is what keeps the CSRF surface at zero: {cookie}"
    );
    assert!(
        cookie.contains("Path=/v1/auth"),
        "the cookie must not ride along on job/runner/execution calls: {cookie}"
    );
    assert!(
        cookie.contains("Secure"),
        "an https Origin means Secure is safe to set: {cookie}"
    );
}

#[tokio::test]
async fn body_login_is_unchanged_for_pre_454_clients() {
    let (app, _store) = fixture();
    // No `refresh_cookie`, no `Origin` — a CLI or curl caller.
    let reply = post(
        app,
        "/v1/auth/login",
        serde_json::json!({ "username": "admin", "password": PASSWORD }),
        None,
        false,
    )
    .await;

    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert!(
        reply.body["refresh_token"].is_string(),
        "body delivery stays the default: {}",
        reply.body
    );
    assert!(
        reply.set_cookie.is_none(),
        "a client that did not ask for a cookie must not get one: {:?}",
        reply.set_cookie
    );
}

#[tokio::test]
async fn cookie_refresh_rotates_the_cookie_and_never_leaks_the_token() {
    let (app, _store) = fixture();
    let first = login_with_cookie(app.clone()).await;
    let cookie = first.cookie_value().expect("login set a cookie value");

    // The SPA's refresh: empty body, cookie attached by the browser.
    let refreshed = post(
        app.clone(),
        "/v1/auth/refresh",
        serde_json::json!({}),
        Some(&cookie),
        true,
    )
    .await;

    assert_eq!(refreshed.status, StatusCode::OK, "{}", refreshed.body);
    assert!(refreshed.body["access_token"].is_string());
    assert!(
        refreshed.body.get("refresh_token").is_none(),
        "THE invariant: a cookie-sourced refresh must not return the rotated \
         token in a body an XSS could read: {}",
        refreshed.body
    );

    let rotated = refreshed
        .cookie_value()
        .expect("a cookie refresh re-sets the cookie");
    assert_ne!(rotated, cookie, "the refresh token must rotate");

    // The old token is revoked: replaying it fails even though the cookie
    // shape is identical.
    let replay = post(
        app.clone(),
        "/v1/auth/refresh",
        serde_json::json!({}),
        Some(&cookie),
        true,
    )
    .await;
    assert_eq!(replay.status, StatusCode::UNAUTHORIZED);

    // The rotated one works, so a second tab picking up the new cookie
    // recovers rather than being logged out.
    let again = post(
        app,
        "/v1/auth/refresh",
        serde_json::json!({}),
        Some(&rotated),
        true,
    )
    .await;
    assert_eq!(again.status, StatusCode::OK, "{}", again.body);
}

#[tokio::test]
async fn body_refresh_still_returns_a_body_token() {
    let (app, _store) = fixture();
    let login = post(
        app.clone(),
        "/v1/auth/login",
        serde_json::json!({ "username": "admin", "password": PASSWORD }),
        None,
        false,
    )
    .await;
    let refresh_token = login.body["refresh_token"].as_str().unwrap().to_string();

    let refreshed = post(
        app,
        "/v1/auth/refresh",
        serde_json::json!({ "refresh_token": refresh_token }),
        None,
        false,
    )
    .await;

    assert_eq!(refreshed.status, StatusCode::OK, "{}", refreshed.body);
    assert!(
        refreshed.body["refresh_token"].is_string(),
        "delivery mirrors the source: a body token yields a body token: {}",
        refreshed.body
    );
    assert!(
        refreshed.set_cookie.is_none(),
        "a body-mode refresh must not start setting cookies: {:?}",
        refreshed.set_cookie
    );
}

#[tokio::test]
async fn a_cookie_cannot_be_laundered_into_a_body_token() {
    // The attack the invariant exists for: script runs on the page, cannot
    // read the cookie, but *can* make the browser send it. It must not be
    // able to trade that for the durable credential — not by asking for body
    // delivery, and not by sending a decoy body token alongside the cookie.
    let (app, _store) = fixture();
    let cookie = login_with_cookie(app.clone()).await.cookie_value().unwrap();

    let decoy = post(
        app.clone(),
        "/v1/auth/refresh",
        serde_json::json!({ "refresh_token": "not-a-real-token" }),
        Some(&cookie),
        true,
    )
    .await;
    assert_eq!(
        decoy.status,
        StatusCode::UNAUTHORIZED,
        "a body token takes precedence and is validated on its own merits, so \
         a bogus one fails instead of silently falling back to the cookie: {}",
        decoy.body
    );

    // …and the cookie survived that attempt unrotated, so the real session is
    // not collateral damage.
    let still_good = post(
        app,
        "/v1/auth/refresh",
        serde_json::json!({}),
        Some(&cookie),
        true,
    )
    .await;
    assert_eq!(still_good.status, StatusCode::OK, "{}", still_good.body);
    assert!(still_good.body.get("refresh_token").is_none());
}

#[tokio::test]
async fn refresh_without_any_token_is_unauthorized() {
    // What the SPA's bootstrap refresh hits on a first-ever visit.
    let (app, _store) = fixture();
    let reply = post(app, "/v1/auth/refresh", serde_json::json!({}), None, true).await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn logout_revokes_the_cookie_token_and_clears_the_cookie() {
    let (app, _store) = fixture();
    let cookie = login_with_cookie(app.clone()).await.cookie_value().unwrap();

    let out = post(
        app.clone(),
        "/v1/auth/logout",
        serde_json::json!({}),
        Some(&cookie),
        true,
    )
    .await;
    assert_eq!(out.status, StatusCode::NO_CONTENT);

    let cleared = out.set_cookie.clone().expect("logout clears the cookie");
    assert!(cleared.contains("Max-Age=0"), "{cleared}");
    assert_eq!(
        out.cookie_value(),
        None,
        "the cleared cookie carries no value: {cleared}"
    );

    // Revoked server-side too, not just dropped by the browser.
    let after = post(
        app,
        "/v1/auth/refresh",
        serde_json::json!({}),
        Some(&cookie),
        true,
    )
    .await;
    assert_eq!(after.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn logout_clears_the_cookie_even_when_the_token_is_already_unknown() {
    let (app, _store) = fixture();
    let out = post(
        app,
        "/v1/auth/logout",
        serde_json::json!({}),
        Some("11111111-2222-3333-4444-555555555555"),
        true,
    )
    .await;
    assert_eq!(out.status, StatusCode::NO_CONTENT);
    assert!(
        out.set_cookie.unwrap().contains("Max-Age=0"),
        "a browser holding an unusable cookie must still be cleaned up"
    );
}

#[tokio::test]
async fn a_foreign_origin_asking_for_a_cookie_gets_one_and_no_body_token() {
    // Deliberate: the server does not police same-origin here. It cannot tell
    // the two cases apart — a proxy that rewrites `Host` makes a working
    // same-origin deployment look foreign, and in a real cross-origin setup
    // `app_url` *is* the dashboard's origin, so the comparison passes exactly
    // when it should fail. See the `refresh_cookie` module docs; the gate that
    // works is `ui/vite.config.ts` refusing to build such a bundle.
    //
    // What matters for security is that this path leaks nothing: the token goes
    // into a cookie scoped to *this* origin, which the foreign page can neither
    // read nor send. It gets a session it cannot refresh, not a credential.
    let (app, _store) = fixture();
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/auth/login")
                .header("content-type", "application/json")
                .header("host", HOST)
                .header("origin", "https://dashboard.elsewhere.test")
                .body(Body::from(
                    serde_json::json!({
                        "username": "admin",
                        "password": PASSWORD,
                        "refresh_cookie": true,
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let cookie = resp
        .headers()
        .get(header::SET_COOKIE)
        .expect("cookie mode was requested")
        .to_str()
        .unwrap()
        .to_string();
    assert!(cookie.contains("HttpOnly"), "{cookie}");
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(
        body.get("refresh_token").is_none(),
        "cookie mode still means no body token, whoever asked: {body}"
    );
}

#[tokio::test]
async fn plain_http_gets_a_cookie_without_the_secure_flag() {
    // A `Secure` cookie is never sent back over plain HTTP, so stamping it on
    // a plain-HTTP deployment would lock the operator out instead of
    // hardening anything.
    let (app, _store) = fixture();
    let reply = post(
        app,
        "/v1/auth/login",
        serde_json::json!({
            "username": "admin",
            "password": PASSWORD,
            "refresh_cookie": true,
        }),
        None,
        false,
    )
    .await;

    let cookie = reply.set_cookie.expect("cookie mode sets a cookie");
    assert!(cookie.contains("HttpOnly"), "{cookie}");
    assert!(
        !cookie.contains("Secure"),
        "no evidence of https means no Secure flag: {cookie}"
    );
}

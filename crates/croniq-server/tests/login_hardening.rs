//! Hardening of the public login surface (issue #428).
//!
//! Four independent measures, each pinned end-to-end through the router:
//!   1. the second factor has a failure budget per `mfa_token`, and the
//!      token dies once it is spent (forcing a fresh password login);
//!   2. a verified TOTP code cannot be replayed inside its skew window;
//!   3. an unknown username and a locked account answer exactly like a
//!      wrong password — same status, same bcrypt cost;
//!   4. both login endpoints are throttled per source address.

use std::net::SocketAddr;

use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode},
};
use chrono::Utc;
use croniq_auth::jwt::JwtConfig;
use croniq_auth::password::hash_password;
use croniq_runner::AppState;
use croniq_server::api::ServerState;
use croniq_server::api::login_throttle::IP_MAX_ATTEMPTS;
use croniq_server::api::server_router;
use croniq_server::sqlite_store;
use croniq_store::models::{PasswordCredential, Role, TotpSecret, User};
use croniq_store::sqlite::SqliteStore;
use http_body_util::BodyExt;
use tokio::sync::mpsc;
use tower::util::ServiceExt;

const PASSWORD: &str = "correct-horse-battery";
const JWT_SECRET: &str = "test-secret-not-for-prod";
/// Fixed base32 seed so the test can compute currently-valid codes.
/// 32 base32 characters = 160 bits, the RFC 4226 recommendation and the
/// minimum `totp-rs` accepts.
const SEED_B32: &str = "JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP";

// ─── Fixtures ────────────────────────────────────────────────────────────────

/// Seed one active admin user with a password credential. `totp` enables a
/// confirmed TOTP secret wrapped with [`JWT_SECRET`]; `locked_until` locks
/// the credential.
fn app_with(totp: bool, locked_until: Option<chrono::DateTime<Utc>>) -> axum::Router {
    let store = sqlite_store(SqliteStore::in_memory().unwrap());
    let now = Utc::now();
    store
        .users_create(&User {
            user_id: "u-admin".into(),
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
            user_id: "u-admin".into(),
            username: "admin".into(),
            password_hash: hash_password(PASSWORD).unwrap(),
            failed_attempts: 0,
            locked_until,
            created_at: now,
        })
        .unwrap();
    if totp {
        store
            .totp_upsert(&TotpSecret {
                user_id: "u-admin".into(),
                secret_enc: croniq_auth::crypto::wrap_totp_secret(JWT_SECRET, SEED_B32.as_bytes())
                    .unwrap(),
                enabled: true,
                confirmed_at: Some(now),
                created_at: now,
            })
            .unwrap();
    }

    let (tx, _rx) = mpsc::unbounded_channel();
    let jwt = JwtConfig {
        secret: JWT_SECRET.into(),
        ..Default::default()
    };
    server_router(ServerState::with_auth(
        AppState::new(),
        tx,
        Some(jwt),
        Some(store),
    ))
}

/// A router with auth configured but **no store**, so every login attempt
/// short-circuits to 503 after the throttle has already counted it. Keeps
/// the rate-limit test free of ~30 bcrypt verifications.
fn app_without_store() -> axum::Router {
    let (tx, _rx) = mpsc::unbounded_channel();
    let jwt = JwtConfig {
        secret: JWT_SECRET.into(),
        ..Default::default()
    };
    server_router(ServerState::with_auth(AppState::new(), tx, Some(jwt), None))
}

async fn post_from(
    app: axum::Router,
    uri: &str,
    body: serde_json::Value,
    peer: Option<SocketAddr>,
) -> (StatusCode, serde_json::Value) {
    let mut req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    if let Some(addr) = peer {
        // What `into_make_service_with_connect_info::<SocketAddr>()` inserts
        // in production; the `ClientIp` extractor reads it from here.
        req.extensions_mut().insert(ConnectInfo(addr));
    }
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

async fn post(
    app: axum::Router,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    post_from(app, uri, body, None).await
}

/// The currently valid 6-digit code for [`SEED_B32`].
fn current_code() -> String {
    let secret = totp_rs::Secret::Encoded(SEED_B32.to_string())
        .to_bytes()
        .unwrap();
    totp_rs::TOTP::new(totp_rs::Algorithm::SHA1, 6, 1, 30, secret)
        .unwrap()
        .generate_current()
        .unwrap()
}

/// A 6-digit code guaranteed to be rejected — a hardcoded "000000" would
/// collide with a genuinely valid code in ~3e-6 of runs and flake.
fn wrong_code() -> String {
    let secret = totp_rs::Secret::Encoded(SEED_B32.to_string())
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

/// Password step only — returns the `mfa_token` handed back for the
/// second step.
async fn mfa_token(app: &axum::Router) -> String {
    let (status, v) = post(
        app.clone(),
        "/v1/auth/login",
        serde_json::json!({ "username": "admin", "password": PASSWORD }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    v["mfa_token"]
        .as_str()
        .unwrap_or_else(|| panic!("expected an mfa_token, got {v}"))
        .to_string()
}

// ─── 1. Second-factor failure budget ─────────────────────────────────────────

#[tokio::test]
async fn mfa_token_is_invalidated_after_repeated_wrong_codes() {
    let app = app_with(true, None);
    let token = mfa_token(&app).await;

    // Five wrong codes. Each is a plain 401 — the budget is not visible to
    // the caller, who cannot tell the last failure from the first.
    for attempt in 1..=5 {
        let (status, _) = post(
            app.clone(),
            "/v1/auth/login/totp",
            serde_json::json!({ "mfa_token": token, "code": wrong_code() }),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "attempt {attempt}");
    }

    // The token is now dead: even a genuinely valid code is refused, so an
    // attacker holding a password + mfa_token cannot keep guessing at the
    // ~3 live codes for the token's full 5-minute TTL.
    let (status, _) = post(
        app.clone(),
        "/v1/auth/login/totp",
        serde_json::json!({ "mfa_token": token, "code": current_code() }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "a spent mfa_token must not accept even a valid code"
    );

    // Redoing the password step mints a fresh token, which works — the
    // budget is per token, not a lockout of the account.
    let fresh = mfa_token(&app).await;
    let (status, v) = post(
        app,
        "/v1/auth/login/totp",
        serde_json::json!({ "mfa_token": fresh, "code": current_code() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "a fresh mfa_token still works: {v}");
    assert!(v["access_token"].is_string());
}

#[tokio::test]
async fn a_malformed_second_factor_does_not_burn_the_budget() {
    // Neither code nor recovery_code is a 400 — a client bug, not a guess.
    // Six of them must leave the token usable.
    let app = app_with(true, None);
    let token = mfa_token(&app).await;
    for _ in 0..6 {
        let (status, _) = post(
            app.clone(),
            "/v1/auth/login/totp",
            serde_json::json!({ "mfa_token": token }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    let (status, _) = post(
        app,
        "/v1/auth/login/totp",
        serde_json::json!({ "mfa_token": token, "code": current_code() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

// ─── 2. TOTP replay ──────────────────────────────────────────────────────────

#[tokio::test]
async fn a_verified_totp_code_cannot_be_replayed() {
    let app = app_with(true, None);
    let code = current_code();

    let (status, v) = post(
        app.clone(),
        "/v1/auth/login",
        serde_json::json!({ "username": "admin", "password": PASSWORD, "code": code }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(v["access_token"].is_string());

    // Same code again, still inside its ±1-step window: refused, because
    // the consumed time step is recorded per user.
    let (status, _) = post(
        app.clone(),
        "/v1/auth/login",
        serde_json::json!({ "username": "admin", "password": PASSWORD, "code": code }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "replaying a consumed code must fail"
    );

    // The same replay guard covers the two-step exchange.
    let token = mfa_token(&app).await;
    let (status, _) = post(
        app,
        "/v1/auth/login/totp",
        serde_json::json!({ "mfa_token": token, "code": code }),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// ─── 3. No username oracle ───────────────────────────────────────────────────

#[tokio::test]
async fn unknown_user_wrong_password_and_locked_account_are_indistinguishable() {
    let app = app_with(false, None);

    let (unknown, _) = post(
        app.clone(),
        "/v1/auth/login",
        serde_json::json!({ "username": "nobody", "password": PASSWORD }),
    )
    .await;
    let (wrong_pw, _) = post(
        app,
        "/v1/auth/login",
        serde_json::json!({ "username": "admin", "password": "not-the-password" }),
    )
    .await;

    // A locked account previously answered 403, which confirmed the account
    // exists (only existing accounts can be locked).
    let locked_app = app_with(false, Some(Utc::now() + chrono::Duration::minutes(15)));
    let (locked, _) = post(
        locked_app,
        "/v1/auth/login",
        serde_json::json!({ "username": "admin", "password": PASSWORD }),
    )
    .await;

    assert_eq!(unknown, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong_pw, StatusCode::UNAUTHORIZED);
    assert_eq!(
        locked,
        StatusCode::UNAUTHORIZED,
        "a locked account must not be distinguishable from a wrong password"
    );
}

#[tokio::test]
async fn an_expired_lockout_lets_the_right_password_through() {
    // The dummy-verify branch must not swallow legitimate logins once the
    // lock has expired.
    let app = app_with(false, Some(Utc::now() - chrono::Duration::minutes(1)));
    let (status, v) = post(
        app,
        "/v1/auth/login",
        serde_json::json!({ "username": "admin", "password": PASSWORD }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(v["access_token"].is_string(), "got {v}");
}

// ─── 4. Per-IP throttling ────────────────────────────────────────────────────

#[tokio::test]
async fn login_is_throttled_per_peer_address() {
    let app = app_without_store();
    let attacker: SocketAddr = "203.0.113.7:51000".parse().unwrap();
    let bystander: SocketAddr = "203.0.113.8:51000".parse().unwrap();
    let body = serde_json::json!({ "username": "admin", "password": PASSWORD });

    // Storeless server: every allowed attempt is a 503, which proves the
    // throttle let it through to the handler body.
    for attempt in 1..=IP_MAX_ATTEMPTS {
        let (status, _) =
            post_from(app.clone(), "/v1/auth/login", body.clone(), Some(attacker)).await;
        assert_eq!(
            status,
            StatusCode::SERVICE_UNAVAILABLE,
            "attempt {attempt} is within the budget"
        );
    }

    let (status, v) = post_from(app.clone(), "/v1/auth/login", body.clone(), Some(attacker)).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(v["error"], serde_json::json!("rate_limited"));

    // Another address is unaffected — the limit is per source, so one
    // attacker cannot deny the endpoint to everybody else.
    let (status, _) = post_from(app.clone(), "/v1/auth/login", body.clone(), Some(bystander)).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);

    // The second-step endpoint shares the same per-IP budget, which the
    // attacker has already spent.
    let (status, _) = post_from(
        app,
        "/v1/auth/login/totp",
        serde_json::json!({ "mfa_token": "whatever", "code": "123456" }),
        Some(attacker),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn requests_without_a_peer_address_are_not_throttled() {
    // Direct handler tests and any deployment that does not supply
    // ConnectInfo must keep working rather than failing closed on a key
    // that does not exist.
    let app = app_without_store();
    let body = serde_json::json!({ "username": "admin", "password": PASSWORD });
    for _ in 0..(IP_MAX_ATTEMPTS + 5) {
        let (status, _) = post(app.clone(), "/v1/auth/login", body.clone()).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    }
}

// ─── Password bounds on a public endpoint ────────────────────────────────────

#[tokio::test]
async fn public_password_endpoints_enforce_the_shared_bounds() {
    let app = app_with(false, None);
    let accept = |password: String| serde_json::json!({ "token": "croniq_inv_nonexistent", "username": "newbie", "password": password });

    // Too short and too long are both 400, before the token is even looked
    // up. A policy-compliant password gets past the length gate and fails
    // on the (bogus) token instead — which is what proves the 400s above
    // came from the length rule.
    let (short, _) = post(
        app.clone(),
        "/v1/invitations/accept",
        accept("1234567".into()),
    )
    .await;
    assert_eq!(short, StatusCode::BAD_REQUEST);

    let (long, _) = post(
        app.clone(),
        "/v1/invitations/accept",
        accept("x".repeat(73)),
    )
    .await;
    assert_eq!(
        long,
        StatusCode::BAD_REQUEST,
        "bcrypt truncates at 72 bytes — anything longer is refused, not silently cut"
    );

    let (ok_len, _) = post(app, "/v1/invitations/accept", accept("12345678".into())).await;
    assert_eq!(ok_len, StatusCode::UNAUTHORIZED, "length gate passed");
}

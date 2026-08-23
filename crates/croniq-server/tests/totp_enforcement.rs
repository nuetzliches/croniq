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
    let jwt = JwtConfig::new("test-secret-not-for-prod");
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

/// Issue #408: an upgrade that changes the JWT secret also changes the
/// HKDF-derived key that wraps stored TOTP secrets, so a previously enrolled
/// user can no longer be verified. The classic trigger is dropping a
/// `pull_api { auth … }` line (removed in 0.29.0) and falling through to a
/// freshly generated `$DATA_DIR/jwt.secret`.
///
/// The response is a bare 500 — indistinguishable from any other fault — so the
/// server logs a dedicated error and `doctor` reports
/// `totp.secrets_undecryptable`. This test pins the wire behaviour and the
/// count the finding is built from.
mod rotated_jwt_secret {
    use super::*;
    use croniq_server::diagnostics::{TotpSecretTally, tally_totp_secrets};
    use croniq_store::models::TotpSecret;

    const OLD_SECRET: &str = "the-secret-that-was-in-pull_api-auth";
    const NEW_SECRET: &str = "freshly-generated-jwt.secret";
    // 160 bits: totp-rs refuses to build a generator below 128, and the
    // #531 tests compute genuinely valid codes rather than asserting on a
    // failure path only.
    const SEED_B32: &str = "JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP";

    /// One active user with a confirmed TOTP secret wrapped with `OLD_SECRET`,
    /// served by a router whose JWT secret is `NEW_SECRET`.
    fn app_after_secret_rotation() -> (axum::Router, croniq_server::store::DynStore) {
        app_after_secret_rotation_with_previous(None)
    }

    /// The same fixture, but with `CRONIQ_JWT_SECRET_PREVIOUS` named on the
    /// config — the shape `main` builds when an operator rotates deliberately
    /// (issue #531). Passed on `JwtConfig` rather than through the environment
    /// so the test does not race sibling tests over a process-global var.
    fn app_after_secret_rotation_with_previous(
        previous: Option<&str>,
    ) -> (axum::Router, croniq_server::store::DynStore) {
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
                locked_until: None,
                created_at: now,
            })
            .unwrap();
        store
            .totp_upsert(&TotpSecret {
                user_id: "u-admin".into(),
                secret_enc: croniq_auth::crypto::wrap_totp_secret(OLD_SECRET, SEED_B32.as_bytes())
                    .unwrap(),
                enabled: true,
                confirmed_at: Some(now),
                created_at: now,
            })
            .unwrap();

        let runner = AppState::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        let jwt = JwtConfig::new(NEW_SECRET).with_previous_secret(previous.map(str::to_string));
        let state = ServerState::with_auth(runner, tx, Some(jwt), Some(store.clone()));
        (server_router(state), store)
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

    #[tokio::test]
    async fn login_with_a_code_fails_while_the_server_is_healthy() {
        let (app, _store) = app_after_secret_rotation();
        // /v1/auth/config still answers 200 — the server is up and serving,
        // which is exactly why the failure reads as an outage in the UI (#410).
        let (cfg_status, _) = get_json(app.clone(), "/v1/auth/config").await;
        assert_eq!(cfg_status, StatusCode::OK);

        let (status, _body) = post_json(
            app,
            "/v1/auth/login",
            serde_json::json!({
                "username": "admin",
                "password": PASSWORD,
                "code": "123456",
            }),
        )
        .await;
        // The unwrap happens before the code is checked, so any code lands here.
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn recovery_codes_are_unaffected() {
        // Recovery codes are SHA-256 hashed, not wrapped, so they remain the
        // documented way back in. A wrong one must still be a 401, not the 500
        // above — that is what makes the recovery path usable at all.
        let (app, _store) = app_after_secret_rotation();
        let (status, _body) = post_json(
            app,
            "/v1/auth/login",
            serde_json::json!({
                "username": "admin",
                "password": PASSWORD,
                "recovery_code": "not-a-real-code",
            }),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn diagnostics_count_the_affected_secrets() {
        let (_app, store) = app_after_secret_rotation();
        assert_eq!(
            tally_totp_secrets(&store, NEW_SECRET, None).undecryptable,
            1
        );
        // Restoring the old secret is the documented remedy — it must work.
        assert_eq!(
            tally_totp_secrets(&store, OLD_SECRET, None).undecryptable,
            0
        );
    }

    // ── Rotating with CRONIQ_JWT_SECRET_PREVIOUS (issue #531) ───────────

    #[tokio::test]
    async fn the_boot_sweep_makes_a_rotated_secret_readable_again() {
        // The whole point of the issue: a rotation must stop costing every
        // enrolled user a re-enrolment. After the sweep the row reads under
        // the new key alone, so the previous value can be dropped.
        let (_app, store) = app_after_secret_rotation();
        let report = croniq_server::totp_rewrap::rewrap_all(&store, NEW_SECRET, OLD_SECRET);
        assert_eq!(report.rewrapped, 1);
        assert!(!report.still_needs_previous());
        assert_eq!(
            tally_totp_secrets(&store, NEW_SECRET, None),
            TotpSecretTally {
                under_current: 1,
                under_previous: 0,
                undecryptable: 0,
            }
        );
    }

    #[tokio::test]
    async fn a_swept_user_logs_in_with_a_real_code() {
        // End to end, through the wire: sweep, then sign in with a genuine
        // TOTP code. Before #531 this could only be a 500.
        let (app, store) = app_after_secret_rotation();
        croniq_server::totp_rewrap::rewrap_all(&store, NEW_SECRET, OLD_SECRET);

        let (status, body) = post_json(
            app,
            "/v1/auth/login",
            serde_json::json!({
                "username": "admin",
                "password": PASSWORD,
                "code": current_code(),
            }),
        )
        .await;
        let body = String::from_utf8_lossy(&body);
        assert_eq!(status, StatusCode::OK, "body: {body}");
        assert!(body.contains("access_token"), "body: {body}");
    }

    #[tokio::test]
    async fn the_unwrap_fallback_carries_a_row_the_sweep_missed() {
        // No sweep at all — only the previous secret named. The login still
        // has to work, because a row the boot sweep could not write must not
        // lock the user out.
        let (app, store) = app_after_secret_rotation_with_previous(Some(OLD_SECRET));
        let (status, body) = post_json(
            app,
            "/v1/auth/login",
            serde_json::json!({
                "username": "admin",
                "password": PASSWORD,
                "code": current_code(),
            }),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "body: {}",
            String::from_utf8_lossy(&body)
        );

        // …and it re-wraps on the way through, so the next start needs no
        // fallback for this row.
        assert_eq!(
            tally_totp_secrets(&store, NEW_SECRET, None).under_current,
            1
        );
    }

    #[tokio::test]
    async fn a_wrong_previous_secret_still_fails_closed() {
        // The fallback must not turn "I named the wrong old value" into a
        // silent success — that would be a second key that authenticates
        // nothing in particular.
        let (app, _store) = app_after_secret_rotation_with_previous(Some("not-the-old-one"));
        let (status, _body) = post_json(
            app,
            "/v1/auth/login",
            serde_json::json!({
                "username": "admin",
                "password": PASSWORD,
                "code": current_code(),
            }),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }
}

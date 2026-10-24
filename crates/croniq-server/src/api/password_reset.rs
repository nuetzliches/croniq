//! Public password-reset endpoints.
//!
//! Routes (both public, no auth):
//!   POST /v1/auth/password-reset/request    body: { username }
//!   POST /v1/auth/password-reset/confirm    body: { token, new_password }
//!
//! Request always returns 202 Accepted regardless of whether the
//! username exists — this avoids leaking which usernames are valid.
//! When the username does exist, a reset token is created and the reset
//! URL is emitted via the configured `EmailSender` (NoopSender by
//! default just logs the recipient + subject, never the body).
//!
//! The raw token never goes through `tracing`. An operator without SMTP
//! still needs to hand the link over, so it is written straight to the
//! process's stderr instead — `tracing` events fan out to the Live
//! Console hub (`crate::live_console`) and to OTLP log export, both of
//! which are readable by parties who must not see a single-use
//! credential for someone else's account.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use chrono::Utc;
use croniq_auth::api_key::{generate_token, hash_token};
use croniq_auth::password::{hash_password, validate_password};
use croniq_store::models::{PasswordCredential, PasswordReset};
use serde::Deserialize;
use uuid::Uuid;

use super::ServerState;
use super::auth_endpoints::{password_disabled_response, rate_limited_response};
use super::login_throttle::ClientIp;

const RESET_TOKEN_TTL: Duration = Duration::from_secs(60 * 60); // 1 hour
const RESET_PREFIX: &str = "croniq_pwr";

#[derive(Deserialize)]
pub struct RequestResetRequest {
    pub username: String,
}

#[derive(Deserialize)]
pub struct ConfirmResetRequest {
    pub token: String,
    pub new_password: String,
}

/// `POST /v1/auth/password-reset/request` — always 202, never leaks.
pub async fn handle_request(
    State(state): State<Arc<ServerState>>,
    ClientIp(peer_ip): ClientIp,
    headers: HeaderMap,
    Json(req): Json<RequestResetRequest>,
) -> Response {
    if !state.password_login_enabled {
        return password_disabled_response();
    }
    // Every request may send a mail and writes a row, so it goes through the
    // same per-address window as a login attempt (review 2026-10-24, A-5).
    if !state.login_throttle.allow_login(peer_ip, &req.username) {
        return rate_limited_response();
    }
    let Some(store) = state.store.as_ref() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };

    // Constant-response: hashing dominates either way, so user-enumeration
    // by timing is harder to mount. (Bcrypt is too slow to call here, so
    // we just no-op symmetrically.)
    let Some(user) = store.users_get_by_username(&req.username).ok().flatten() else {
        return StatusCode::ACCEPTED.into_response();
    };
    if !user.is_active {
        return StatusCode::ACCEPTED.into_response();
    }
    // One reset mail per account per cooldown, whatever the source address:
    // otherwise anyone who knows a username can fill that user's inbox and
    // the `password_resets` table. Answered like every other case.
    if !state.login_throttle.allow_reset_mail(&user.user_id) {
        return StatusCode::ACCEPTED.into_response();
    }

    let now = Utc::now();
    // One live token per account: a new request retires the older ones, so a
    // link that leaked from an earlier mail stops working.
    if store
        .password_resets_invalidate_for_user(&user.user_id, now)
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let (raw_token, token_hash) = generate_token(RESET_PREFIX);
    let expires_at = now + chrono::Duration::from_std(RESET_TOKEN_TTL).unwrap();
    let reset_id = Uuid::new_v4().to_string();

    let reset = PasswordReset {
        reset_id: reset_id.clone(),
        user_id: user.user_id.clone(),
        token_hash,
        expires_at,
        used_at: None,
        created_at: now,
    };
    if store.password_resets_create(&reset).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    // Public, unauthenticated endpoint → the Host header is attacker-controlled,
    // so trust_request_host = false. Without CRONIQ_APP_URL or a reverse-proxy
    // X-Forwarded-Host this falls back to the localhost default rather than
    // letting a spoofed Host poison the emailed reset link (token theft).
    let base = crate::api::resolve_link_base(&state.app_base_url, &headers, false);
    let confirm_url = format!("{base}/password-reset/confirm?token={raw_token}");

    // Email delivery — NoopSender logs only the recipient + subject
    // (body with the token is intentionally not part of the log line).
    let target_email = user.email.as_deref().unwrap_or(&req.username);
    let _ = state.email_sender.send(
        target_email,
        "Reset your Croniq password",
        &format!(
            "A password reset was requested for your Croniq account.\n\nTo set a new password, visit:\n{}\n\nThis link expires in 1 hour. If you didn't request this, ignore this message.",
            confirm_url
        ),
    );

    // INFO-level audit line. Identifiers only — the confirm URL carries a
    // single-use credential and must never enter the tracing pipeline,
    // which fans out to the Live Console SSE stream and to OTLP log
    // export. `croniq::password_reset` is additionally on the console
    // layer's drop list, but the rule holds independently of that.
    tracing::info!(
        target: "croniq::password_reset",
        user_id = %user.user_id,
        reset_id = %reset_id,
        "password reset issued"
    );

    // Operators running without a delivering mail transport still need the
    // link to hand off. Write it directly to stderr, bypassing `tracing`
    // entirely: no console hub, no ring buffer, no OTLP export — same
    // trust model as `croniq init` printing the first-run admin password
    // to the terminal. With SMTP configured the token never appears here.
    if !state.email_sender.delivers() {
        use std::io::Write;
        let mut stderr = std::io::stderr().lock();
        let _ = writeln!(
            stderr,
            "[croniq] password reset issued for user {} (reset {}) — no mail transport configured, deliver this link manually: {}",
            user.user_id, reset_id, confirm_url
        );
    }

    StatusCode::ACCEPTED.into_response()
}

/// `POST /v1/auth/password-reset/confirm`
pub async fn handle_confirm(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<ConfirmResetRequest>,
) -> Response {
    if !state.password_login_enabled {
        return password_disabled_response();
    }
    if validate_password(&req.new_password).is_err() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Some(store) = state.store.as_ref() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };

    let token_hash = hash_token(&req.token);
    let Some(reset) = store
        .password_resets_get_by_token_hash(&token_hash)
        .ok()
        .flatten()
    else {
        return StatusCode::UNAUTHORIZED.into_response();
    };

    if reset.used_at.is_some() {
        return StatusCode::GONE.into_response();
    }
    if Utc::now() > reset.expires_at {
        return StatusCode::GONE.into_response();
    }

    let Some(user) = store.users_get_by_id(&reset.user_id).ok().flatten() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // A deactivated account must not get a fresh password through a link
    // issued before it was deactivated (review 2026-10-24, A-5).
    if !user.is_active {
        return StatusCode::GONE.into_response();
    }

    let pw_hash = match hash_password(&req.new_password) {
        Ok(h) => h,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let Some(cred) = store.get_credentials(&user.username).ok().flatten() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let updated = PasswordCredential {
        password_hash: pw_hash,
        failed_attempts: 0,
        locked_until: None,
        ..cred
    };
    if store.upsert_credentials(&updated).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    // A reset is the canonical "lock the attacker out" action, so it must
    // invalidate access tokens issued under the old password (issue #431).
    if store.users_bump_token_generation(&user.user_id).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    // Retires this token and any sibling still open (review 2026-10-24, A-5).
    let _ = store.password_resets_invalidate_for_user(&user.user_id, Utc::now());

    StatusCode::NO_CONTENT.into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{DynStore, sqlite_store};
    use croniq_store::models::{Role, User};
    use croniq_store::sqlite::SqliteStore;

    fn setup() -> (Arc<ServerState>, DynStore) {
        let store = sqlite_store(SqliteStore::in_memory().unwrap());
        let now = Utc::now();
        store
            .users_create(&User {
                user_id: "u1".into(),
                username: "alex".into(),
                email: Some("alex@example.com".into()),
                display_name: None,
                role: Role::Viewer,
                is_active: true,
                created_at: now,
                updated_at: now,
                last_login_at: None,
            })
            .unwrap();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let state = ServerState::with_auth(
            croniq_runner::AppState::new(),
            tx,
            None,
            Some(Arc::clone(&store)),
        );
        (state, store)
    }

    fn seed_token(store: &DynStore, raw: &str) {
        store
            .password_resets_create(&PasswordReset {
                reset_id: Uuid::new_v4().to_string(),
                user_id: "u1".into(),
                token_hash: hash_token(raw),
                expires_at: Utc::now() + chrono::Duration::hours(1),
                used_at: None,
                created_at: Utc::now(),
            })
            .unwrap();
    }

    fn is_used(store: &DynStore, raw: &str) -> bool {
        store
            .password_resets_get_by_token_hash(&hash_token(raw))
            .unwrap()
            .unwrap()
            .used_at
            .is_some()
    }

    async fn request(state: &Arc<ServerState>) -> StatusCode {
        handle_request(
            State(Arc::clone(state)),
            ClientIp(None),
            HeaderMap::new(),
            Json(RequestResetRequest {
                username: "alex".into(),
            }),
        )
        .await
        .status()
    }

    /// Review 2026-10-24, A-5: a new request retires the older link, and a
    /// second request inside the cooldown issues nothing (same 202).
    #[tokio::test]
    async fn one_live_token_per_account_and_a_mail_cooldown() {
        let (state, store) = setup();
        seed_token(&store, "croniq_pwr_old");
        assert_eq!(request(&state).await, StatusCode::ACCEPTED);
        assert!(is_used(&store, "croniq_pwr_old"), "older token retired");

        seed_token(&store, "croniq_pwr_between");
        assert_eq!(request(&state).await, StatusCode::ACCEPTED);
        assert!(
            !is_used(&store, "croniq_pwr_between"),
            "inside the cooldown nothing is issued, so nothing is retired"
        );
    }

    /// Review 2026-10-24, A-5: a deactivated account cannot be reactivated
    /// through a link issued earlier, and redeeming retires every sibling.
    #[tokio::test]
    async fn confirm_refuses_inactive_users_and_retires_siblings() {
        let (state, store) = setup();
        seed_token(&store, "croniq_pwr_a");
        seed_token(&store, "croniq_pwr_b");
        store
            .upsert_credentials(&PasswordCredential {
                user_id: "u1".into(),
                username: "alex".into(),
                password_hash: "x".into(),
                failed_attempts: 0,
                locked_until: None,
                created_at: Utc::now(),
            })
            .unwrap();
        let confirm = |token: &str| {
            handle_confirm(
                State(Arc::clone(&state)),
                Json(ConfirmResetRequest {
                    token: token.into(),
                    new_password: "a-long-enough-password-1".into(),
                }),
            )
        };

        assert_eq!(
            confirm("croniq_pwr_a").await.status(),
            StatusCode::NO_CONTENT
        );
        assert!(is_used(&store, "croniq_pwr_b"), "sibling retired");
        assert_eq!(confirm("croniq_pwr_b").await.status(), StatusCode::GONE);

        seed_token(&store, "croniq_pwr_c");
        let mut user = store.users_get_by_id("u1").unwrap().unwrap();
        user.is_active = false;
        store.users_update(&user).unwrap();
        assert_eq!(confirm("croniq_pwr_c").await.status(), StatusCode::GONE);
    }
}

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
use super::auth_endpoints::password_disabled_response;

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
    headers: HeaderMap,
    Json(req): Json<RequestResetRequest>,
) -> Response {
    if !state.password_login_enabled {
        return password_disabled_response();
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

    let (raw_token, token_hash) = generate_token(RESET_PREFIX);
    let now = Utc::now();
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
    let _ = store.password_resets_mark_used(&reset.reset_id, Utc::now());

    StatusCode::NO_CONTENT.into_response()
}

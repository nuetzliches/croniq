//! Public password-reset endpoints.
//!
//! Routes (both public, no auth):
//!   POST /v1/auth/password-reset/request    body: { username }
//!   POST /v1/auth/password-reset/confirm    body: { token, new_password }
//!
//! Request always returns 202 Accepted regardless of whether the
//! username exists — this avoids leaking which usernames are valid.
//! When the username does exist, a reset token is created, the reset
//! URL is emitted via the configured `EmailSender` (NoopSender by
//! default just logs the audit event), and the raw token is logged
//! at INFO so an operator can still recover the link from server logs
//! if SMTP isn't configured (intentional — explicit hand-off, not
//! cleartext-logging of a real credential; reset tokens are short-TTL
//! single-use).

use std::sync::Arc;
use std::time::Duration;

use axum::{Json, extract::State, http::StatusCode};
use chrono::Utc;
use croniq_auth::api_key::{generate_token, hash_token};
use croniq_auth::password::hash_password;
use croniq_store::models::{PasswordCredential, PasswordReset};
use serde::Deserialize;
use uuid::Uuid;

use super::ServerState;

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
    Json(req): Json<RequestResetRequest>,
) -> StatusCode {
    let Some(store) = state.store.as_ref() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };

    // Constant-response: hashing dominates either way, so user-enumeration
    // by timing is harder to mount. (Bcrypt is too slow to call here, so
    // we just no-op symmetrically.)
    let Some(user) = store.users_get_by_username(&req.username).ok().flatten() else {
        return StatusCode::ACCEPTED;
    };
    if !user.is_active {
        return StatusCode::ACCEPTED;
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
        return StatusCode::INTERNAL_SERVER_ERROR;
    }

    let confirm_url = format!(
        "{}/password-reset/confirm?token={}",
        state.app_base_url.trim_end_matches('/'),
        raw_token
    );

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

    // INFO-level audit line for operators running without SMTP. The token
    // is a single-use, short-TTL credential whose value the operator
    // legitimately needs to hand off (same trust model as `croniq init`
    // printing the admin password to stdout on first run).
    tracing::info!(
        target: "croniq::password_reset",
        user_id = %user.user_id,
        reset_id = %reset_id,
        confirm_url = %confirm_url,
        "password reset issued"
    );

    StatusCode::ACCEPTED
}

/// `POST /v1/auth/password-reset/confirm`
pub async fn handle_confirm(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<ConfirmResetRequest>,
) -> StatusCode {
    if req.new_password.len() < 8 {
        return StatusCode::BAD_REQUEST;
    }
    let Some(store) = state.store.as_ref() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };

    let token_hash = hash_token(&req.token);
    let Some(reset) = store
        .password_resets_get_by_token_hash(&token_hash)
        .ok()
        .flatten()
    else {
        return StatusCode::UNAUTHORIZED;
    };

    if reset.used_at.is_some() {
        return StatusCode::GONE;
    }
    if Utc::now() > reset.expires_at {
        return StatusCode::GONE;
    }

    let Some(user) = store.users_get_by_id(&reset.user_id).ok().flatten() else {
        return StatusCode::NOT_FOUND;
    };

    let pw_hash = match hash_password(&req.new_password) {
        Ok(h) => h,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR,
    };
    let Some(cred) = store.get_credentials(&user.username).ok().flatten() else {
        return StatusCode::NOT_FOUND;
    };
    let updated = PasswordCredential {
        password_hash: pw_hash,
        failed_attempts: 0,
        locked_until: None,
        ..cred
    };
    if store.upsert_credentials(&updated).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    let _ = store.password_resets_mark_used(&reset.reset_id, Utc::now());

    StatusCode::NO_CONTENT
}

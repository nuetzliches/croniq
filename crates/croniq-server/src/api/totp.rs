//! TOTP/2FA setup, confirm, disable, recovery-code regeneration.
//!
//! Routes (all under `/v1/users/me/totp/...`, all require an
//! authenticated user — API keys can't enable TOTP):
//!   POST /v1/users/me/totp/setup
//!   POST /v1/users/me/totp/confirm                body: { code }
//!   POST /v1/users/me/totp/disable                body: { password }
//!   POST /v1/users/me/totp/recovery-codes/regenerate    body: { password }
//!
//! Setup is idempotent — calling it again before /confirm overwrites
//! the pending secret (`enabled=0`). Once /confirm succeeds, /setup
//! returns 409 until /disable runs.

use std::sync::Arc;

use axum::{Extension, Json, extract::State, http::StatusCode};
use chrono::Utc;
use croniq_auth::CallerContext;
use croniq_auth::CallerType;
use croniq_auth::crypto::{unwrap_totp_secret, wrap_totp_secret};
use croniq_auth::password::verify_password;
use croniq_auth::totp::{enroll_user, hash_recovery_code, verify_code};
use croniq_store::models::{RecoveryCode, TotpSecret};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::ServerState;
use crate::api::audit;

// ─── DTOs ────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct SetupResponse {
    /// Base32 seed (so the user can type it if the QR code scan fails).
    pub secret: String,
    /// `otpauth://` URL — render as QR-code in the UI.
    pub otpauth_url: String,
    /// 10 single-use recovery codes. Shown ONCE; only hashes are kept.
    pub recovery_codes: Vec<String>,
}

#[derive(Deserialize)]
pub struct ConfirmRequest {
    pub code: String,
}

#[derive(Deserialize)]
pub struct PasswordOnlyRequest {
    pub password: String,
}

#[derive(Serialize)]
pub struct RegenerateResponse {
    pub recovery_codes: Vec<String>,
}

// ─── Handlers ────────────────────────────────────────────────────────────────

/// `POST /v1/users/me/totp/setup` — generates pending secret + codes.
pub async fn handle_setup(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
) -> Result<Json<SetupResponse>, StatusCode> {
    let (user_id, jwt_secret) = require_user_with_jwt(&ctx, &state)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    // Refuse re-setup on a confirmed secret — admin must /disable first.
    if let Some(existing) = store
        .totp_get(user_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        && existing.enabled
    {
        return Err(StatusCode::CONFLICT);
    }

    let user = store
        .users_get_by_id(user_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Generate + persist the pending secret and recovery codes. Shared with
    // the login-time enrolment flow (enforced 2FA, not-yet-enrolled user).
    Ok(Json(create_pending_enrollment(
        store,
        jwt_secret,
        user_id,
        &user.username,
    )?))
}

/// `POST /v1/users/me/totp/confirm`
pub async fn handle_confirm(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Json(req): Json<ConfirmRequest>,
) -> StatusCode {
    let Ok((user_id, jwt_secret)) = require_user_with_jwt(&ctx, &state) else {
        return StatusCode::UNAUTHORIZED;
    };
    let Some(store) = state.store.as_ref() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    match confirm_pending_enrollment(store, jwt_secret, user_id, &req.code) {
        Ok(()) => {
            audit::record(store, &ctx, "totp.enabled", "user", Some(user_id), None);
            StatusCode::NO_CONTENT
        }
        Err(s) => s,
    }
}

/// `POST /v1/users/me/totp/disable` — requires fresh password proof.
pub async fn handle_disable(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Json(req): Json<PasswordOnlyRequest>,
) -> StatusCode {
    let Some(user_id) = ctx.user_id.as_deref() else {
        return StatusCode::FORBIDDEN;
    };
    let Some(store) = state.store.as_ref() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let Some(user) = store.users_get_by_id(user_id).ok().flatten() else {
        return StatusCode::NOT_FOUND;
    };
    if !verify_password_for_user(store, &user.username, &req.password) {
        return StatusCode::UNAUTHORIZED;
    }
    let _ = store.totp_delete(user_id);
    audit::record(store, &ctx, "totp.disabled", "user", Some(user_id), None);
    StatusCode::NO_CONTENT
}

/// `POST /v1/users/me/totp/recovery-codes/regenerate` — fresh codes,
/// old ones (used or not) all invalidated. Requires fresh password.
pub async fn handle_regenerate(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Json(req): Json<PasswordOnlyRequest>,
) -> Result<Json<RegenerateResponse>, StatusCode> {
    let Some(user_id) = ctx.user_id.as_deref() else {
        return Err(StatusCode::FORBIDDEN);
    };
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let user = store
        .users_get_by_id(user_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    if !verify_password_for_user(store, &user.username, &req.password) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let secret = store
        .totp_get(user_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    if !secret.enabled {
        return Err(StatusCode::CONFLICT);
    }

    let now = Utc::now();
    let raw_codes = croniq_auth::totp::generate_recovery_codes();
    let codes = build_recovery_codes(user_id, &raw_codes, now);
    store
        .recovery_codes_replace_all(user_id, &codes)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(RegenerateResponse {
        recovery_codes: raw_codes,
    }))
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn require_user_with_jwt<'a>(
    ctx: &'a CallerContext,
    state: &'a Arc<ServerState>,
) -> Result<(&'a str, &'a str), StatusCode> {
    if ctx.caller_type != CallerType::User {
        return Err(StatusCode::FORBIDDEN);
    }
    let user_id = ctx.user_id.as_deref().ok_or(StatusCode::UNAUTHORIZED)?;
    let secret = state
        .jwt_config
        .as_ref()
        .map(|c| c.secret.as_str())
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    Ok((user_id, secret))
}

fn verify_password_for_user(
    store: &crate::store::DynStore,
    username: &str,
    password: &str,
) -> bool {
    let Some(cred) = store.get_credentials(username).ok().flatten() else {
        return false;
    };
    matches!(verify_password(password, &cred.password_hash), Ok(true))
}

fn build_recovery_codes(
    user_id: &str,
    raw_codes: &[String],
    now: chrono::DateTime<Utc>,
) -> Vec<RecoveryCode> {
    raw_codes
        .iter()
        .map(|raw| RecoveryCode {
            code_id: Uuid::new_v4().to_string(),
            user_id: user_id.to_string(),
            code_hash: hash_recovery_code(raw),
            used_at: None,
            created_at: now,
        })
        .collect()
}

/// Generate a fresh pending TOTP secret + recovery codes for `user_id` and
/// persist them (secret `enabled=false`). Returns the once-shown enrolment
/// material. Shared by `/v1/users/me/totp/setup` and the login-time enrolment
/// flow (`/v1/auth/login/enroll/totp/begin`). Idempotent — re-calling before
/// confirm overwrites the pending secret.
pub(crate) fn create_pending_enrollment(
    store: &crate::store::DynStore,
    jwt_secret: &str,
    user_id: &str,
    username: &str,
) -> Result<SetupResponse, StatusCode> {
    let enrolment = enroll_user(username).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let wrapped = wrap_totp_secret(jwt_secret, enrolment.secret_b32.as_bytes())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let now = Utc::now();
    store
        .totp_upsert(&TotpSecret {
            user_id: user_id.to_string(),
            secret_enc: wrapped,
            enabled: false,
            confirmed_at: None,
            created_at: now,
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    // Recovery codes are persisted ahead of confirm so a partial setup (user
    // closes the browser mid-flow) still has codes once confirm completes.
    // They're useless on their own — the matching secret must be enabled.
    let codes = build_recovery_codes(user_id, &enrolment.recovery_codes, now);
    store
        .recovery_codes_replace_all(user_id, &codes)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(SetupResponse {
        secret: enrolment.secret_b32,
        otpauth_url: enrolment.otpauth_url,
        recovery_codes: enrolment.recovery_codes,
    })
}

/// Verify `code` against the user's pending secret and enable TOTP. Shared by
/// `/v1/users/me/totp/confirm` and `/v1/auth/login/enroll/totp/confirm`.
/// Errors map straight to HTTP status (`NOT_FOUND` no pending secret,
/// `CONFLICT` already enabled, `UNAUTHORIZED` wrong code).
pub(crate) fn confirm_pending_enrollment(
    store: &crate::store::DynStore,
    jwt_secret: &str,
    user_id: &str,
    code: &str,
) -> Result<(), StatusCode> {
    let secret_row = store
        .totp_get(user_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    if secret_row.enabled {
        return Err(StatusCode::CONFLICT);
    }
    // Pending enrolment, so this was wrapped with the secret that was active a
    // few seconds ago — a failure means it changed in between (issue #408).
    let raw = unwrap_totp_secret(jwt_secret, &secret_row.secret_enc).map_err(|e| {
        tracing::error!(
            user_id,
            error = %e,
            "pending TOTP secret could not be unwrapped — the JWT secret changed between enrol \
             begin and confirm. The user has to restart enrolment."
        );
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let secret_b32 = String::from_utf8(raw).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    match verify_code(&secret_b32, code) {
        Ok(true) => {}
        _ => return Err(StatusCode::UNAUTHORIZED),
    }
    store
        .totp_set_enabled(user_id, true, Some(Utc::now()))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(())
}

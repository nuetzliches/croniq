//! Auth API endpoints: login, refresh, logout, API client/key management.

// clippy 1.98 tightened `result_large_err`, which now fires on every handler
// in this module that returns `Result<T, Response>` — the `Err` variant is a
// fully-built `axum::response::Response` at 128 bytes.
//
// Returning a `Response` as the error is what these handlers need: unlike the
// rest of the API they attach headers on failure (the refresh-token cookie
// being cleared, `WWW-Authenticate`, throttling headers), which a bare
// `StatusCode` cannot carry. Clippy's remedy is to box it, which would mean
// `Box::new` at 22 error sites to move 128 bytes off the stack on a path that
// is already allocating a response body. That trade is not worth it here.
//
// Scoped to this module rather than the workspace so the lint keeps working
// where it has something useful to say.
#![allow(clippy::result_large_err)]

use std::sync::Arc;

use axum::{
    Extension, Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use chrono::Utc;
use croniq_auth::api_key::{generate_api_key, hash_api_key};
use croniq_auth::context::Scope;
use croniq_auth::crypto::{WrappedUnder, unwrap_totp_secret_with_previous};
use croniq_auth::jwt::{
    issue_mfa_token, issue_token_pair, issue_totp_enroll_token, validate_mfa_token,
    validate_totp_enroll_token,
};
use croniq_auth::password::{dummy_verify, verify_password};
use croniq_auth::totp::{hash_recovery_code, verify_code_with_step};
use croniq_auth::{AuthMethod, CallerContext, CallerType, default_scopes_for_role};
use croniq_store::models::{ApiClient, ApiKey, RefreshToken};
use croniq_store::traits::StoreError;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::ServerState;
use crate::api::audit;
use crate::api::auth_middleware::{require_grantable_scopes, require_scope};
use crate::api::calendars::ValidationError;
use crate::api::login_throttle::{ClientIp, LoginThrottle, MFA_MAX_FAILURES};
use crate::api::refresh_cookie;

// ─── Request/Response types ──────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    /// Optional second factor supplied inline so a TOTP login can complete
    /// in a single request. When omitted and the account has 2FA, the server
    /// falls back to the two-step `mfa_token` flow (`MfaRequiredResponse`).
    /// At most one of `code` / `recovery_code` may be set.
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub recovery_code: Option<String>,
    /// Opt in to `HttpOnly` refresh-cookie delivery (issue #454). The
    /// dashboard SPA sets this; every other client leaves it unset and keeps
    /// receiving `refresh_token` in the body. See
    /// [`crate::api::refresh_cookie`].
    #[serde(default)]
    pub refresh_cookie: bool,
}

#[derive(Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    /// Omitted entirely when the refresh token was delivered as a cookie —
    /// returning both would let an XSS read a fresh 7-day token straight out
    /// of a refresh it triggered itself, which is the whole thing the cookie
    /// exists to prevent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    pub token_type: String,
    pub expires_in: i64,
}

/// Response for `POST /v1/auth/login` when the user has TOTP enabled.
/// The client must POST to `/v1/auth/login/totp` with the `mfa_token`
/// and a current 6-digit code (or a recovery code) to receive
/// `TokenResponse`. The `mfa_token` itself is unusable for any other
/// API call — its purpose claim is "mfa", which `validate_token`
/// rejects.
#[derive(Serialize)]
pub struct MfaRequiredResponse {
    pub requires_totp: bool, // always true; future-proof for WebAuthn
    pub mfa_token: String,
    pub mfa_token_expires_in: i64,
}

/// Response for `POST /v1/auth/login` when enforced 2FA is on but the account
/// has no confirmed TOTP secret. Rather than refusing, the server hands back a
/// short-lived `enroll_token`; the client drives the inline enrolment flow via
/// `/v1/auth/login/enroll/totp/{begin,confirm}` to set up TOTP and finish login.
#[derive(Serialize)]
pub struct EnrollmentRequiredResponse {
    pub enrollment_required: bool, // always true; distinguishes the variant
    pub enroll_token: String,
    pub enroll_token_expires_in: i64,
}

/// `POST /v1/auth/login` response — one of three shapes. `#[serde(untagged)]`,
/// so clients pattern-match on the presence of `access_token` (tokens),
/// `requires_totp` (MFA step-up), or `enrollment_required` (forced enrolment).
#[derive(Serialize)]
#[serde(untagged)]
pub enum LoginResponse {
    Tokens(TokenResponse),
    MfaRequired(MfaRequiredResponse),
    EnrollmentRequired(EnrollmentRequiredResponse),
}

#[derive(Deserialize)]
pub struct TotpLoginRequest {
    pub mfa_token: String,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub recovery_code: Option<String>,
    /// See [`LoginRequest::refresh_cookie`].
    #[serde(default)]
    pub refresh_cookie: bool,
}

#[derive(Deserialize)]
pub struct RefreshRequest {
    /// Optional since #454: a cookie-based caller sends no body token, and
    /// the delivery mode of the response mirrors where the token came from.
    #[serde(default)]
    pub refresh_token: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct LogoutRequest {
    /// Optional since #454 — a cookie-based caller sends `{}` and the token
    /// is read from the cookie instead.
    pub refresh_token: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateClientRequest {
    pub name: String,
    pub scopes: Vec<String>,
}

#[derive(Serialize)]
pub struct CreateClientResponse {
    pub client_id: String,
    pub name: String,
    pub scopes: Vec<String>,
}

#[derive(Serialize)]
pub struct CreateApiKeyResponse {
    /// The raw API key — shown ONCE, never again.
    pub raw_key: String,
    pub key_id: String,
    pub key_prefix: String,
    pub client_id: String,
}

// ─── Error helpers ───────────────────────────────────────────────────────────

/// Lift a bare `StatusCode` into the `Response` error type. Produces the same
/// body-less response that the previous `Result<_, StatusCode>` signature did,
/// so the only externally visible response that changed is the new 403 with a
/// JSON body (see [`password_disabled_response`]).
pub(super) fn status_err(s: StatusCode) -> Response {
    s.into_response()
}

/// 403 response with a `{"error":"…"}` envelope, returned by every password-flow
/// endpoint when password login is disabled (issue #138).
pub(super) fn password_disabled_response() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({
            "error": "password_login_disabled",
            "message": "password login is disabled on this server",
        })),
    )
        .into_response()
}

/// 429 response for the per-IP login throttle (issue #428). Keyed by the
/// socket peer address — deployments behind a reverse proxy should
/// throttle at the proxy, since every request reaches us from the
/// proxy's address (see [`crate::api::login_throttle`]).
pub(super) fn rate_limited_response() -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        Json(serde_json::json!({
            "error": "rate_limited",
            "message": "too many login attempts from this address — try again later",
        })),
    )
        .into_response()
}

// ─── Refresh-token delivery ──────────────────────────────────────────────────

/// Which delivery mode a token-minting request asked for, and whether the
/// cookie may carry `Secure`. See [`crate::api::refresh_cookie`] for why this
/// is the caller's choice and not something the server second-guesses.
fn resolve_delivery(
    requested: bool,
    headers: &HeaderMap,
    state: &ServerState,
) -> (refresh_cookie::Delivery, bool) {
    let delivery = if requested {
        refresh_cookie::Delivery::Cookie
    } else {
        refresh_cookie::Delivery::Body
    };
    (
        delivery,
        refresh_cookie::is_secure_request(headers, state.app_base_url.as_deref()),
    )
}

/// Apply a delivery mode to a minted pair: either leave the refresh token in
/// the response body (pre-#454 behaviour) or move it into a `Set-Cookie`
/// header and drop it from the body. It is deliberately a *move* — the token
/// exists in exactly one of the two places, never both.
fn deliver(
    mut tokens: TokenResponse,
    delivery: refresh_cookie::Delivery,
    secure: bool,
    refresh_ttl_secs: i64,
) -> (HeaderMap, TokenResponse) {
    match delivery {
        refresh_cookie::Delivery::Body => (HeaderMap::new(), tokens),
        refresh_cookie::Delivery::Cookie => {
            let value = tokens.refresh_token.take();
            let cookie = value
                .as_deref()
                .and_then(|v| refresh_cookie::set(v, refresh_ttl_secs, secure));
            (refresh_cookie::header_map(cookie), tokens)
        }
    }
}

// ─── Handlers ────────────────────────────────────────────────────────────────

/// `POST /v1/auth/login`
pub async fn handle_login(
    State(state): State<Arc<ServerState>>,
    ClientIp(peer_ip): ClientIp,
    headers: HeaderMap,
    Json(req): Json<LoginRequest>,
) -> Result<(HeaderMap, Json<LoginResponse>), Response> {
    if !state.password_login_enabled {
        return Err(password_disabled_response());
    }
    if !state.login_throttle.allow_ip(peer_ip) {
        return Err(rate_limited_response());
    }
    // Resolved before any credential work so a request that cannot be served
    // fails on its own terms rather than after a successful password check.
    let (delivery, secure) = resolve_delivery(req.refresh_cookie, &headers, &state);
    let store = state
        .store
        .as_ref()
        .ok_or_else(|| status_err(StatusCode::SERVICE_UNAVAILABLE))?;
    let jwt_config = state
        .jwt_config
        .as_ref()
        .ok_or_else(|| status_err(StatusCode::SERVICE_UNAVAILABLE))?;

    let Some(cred) = store
        .get_credentials(&req.username)
        .map_err(|_| status_err(StatusCode::INTERNAL_SERVER_ERROR))?
    else {
        // Unknown username. Burn one bcrypt verification against a constant
        // hash so this branch costs the same as a wrong password — the
        // immediate 401 was a timing oracle for username enumeration
        // (issue #428). Same symmetry idea as password_reset's always-202.
        dummy_verify(&req.password);
        return Err(status_err(StatusCode::UNAUTHORIZED));
    };

    // Check lockout. The response is the same generic 401 as a wrong
    // password — a distinct status would confirm that the account exists
    // (only existing accounts can be locked), and the dummy verification
    // keeps the timing symmetric with the branches that do hash.
    if let Some(locked_until) = cred.locked_until
        && Utc::now() < locked_until
    {
        dummy_verify(&req.password);
        return Err(status_err(StatusCode::UNAUTHORIZED));
    }

    // Verify password
    let valid = verify_password(&req.password, &cred.password_hash)
        .map_err(|_| status_err(StatusCode::INTERNAL_SERVER_ERROR))?;

    if !valid {
        // Increment failed attempts
        let mut updated = cred.clone();
        updated.failed_attempts += 1;
        if updated.failed_attempts >= 5 {
            updated.locked_until = Some(Utc::now() + chrono::Duration::minutes(15));
        }
        let _ = store.upsert_credentials(&updated);
        audit::record_event(
            store,
            "user",
            Some(&cred.user_id),
            "auth.login_failed",
            "user",
            Some(&cred.user_id),
        );
        return Err(status_err(StatusCode::UNAUTHORIZED));
    }

    // Reset failed attempts on success
    if cred.failed_attempts > 0 {
        let mut updated = cred.clone();
        updated.failed_attempts = 0;
        updated.locked_until = None;
        let _ = store.upsert_credentials(&updated);
    }

    // Look up the user row to pull role + active flag. Pre-PR-A1 deploys
    // backfill an admin user via migration 011, so this should always
    // resolve. A missing user is treated as 401 — the credential exists
    // but the identity it points at no longer does (manual DB tamper or
    // hand-rollback to before migration 011).
    let user = store
        .users_get_by_id(&cred.user_id)
        .map_err(|_| status_err(StatusCode::INTERNAL_SERVER_ERROR))?
        .ok_or_else(|| status_err(StatusCode::UNAUTHORIZED))?;

    if !user.is_active {
        return Err(status_err(StatusCode::FORBIDDEN));
    }

    // Second factor. Password verification has already succeeded above.
    //   * account has a confirmed TOTP secret + an inline code → verify now
    //     and mint tokens (single-request login);
    //   * account has a secret but no inline code → hand back a short-lived
    //     mfa_token for the two-step /v1/auth/login/totp exchange;
    //   * account has no secret but enforced 2FA is on → hand back a
    //     short-lived enroll_token and let the user set TOTP up inline; the
    //     account is *not* refused (issue #409).
    let secret_row = store
        .totp_get(&user.user_id)
        .map_err(|_| status_err(StatusCode::INTERNAL_SERVER_ERROR))?;
    let totp_enabled = secret_row.as_ref().map(|t| t.enabled).unwrap_or(false);

    if totp_enabled {
        let secret_row = secret_row.expect("totp_enabled implies a secret row");
        if req.code.is_none() && req.recovery_code.is_none() {
            let (mfa_token, expires_in) = issue_mfa_token(jwt_config, &user.user_id)
                .map_err(|_| status_err(StatusCode::INTERNAL_SERVER_ERROR))?;
            audit::record_event(
                store,
                "user",
                Some(&user.user_id),
                "auth.login_password_ok_totp_required",
                "user",
                Some(&user.user_id),
            );
            return Ok((
                HeaderMap::new(),
                Json(LoginResponse::MfaRequired(MfaRequiredResponse {
                    requires_totp: true,
                    mfa_token,
                    mfa_token_expires_in: expires_in,
                })),
            ));
        }
        verify_second_factor(
            jwt_config,
            store,
            &state.login_throttle,
            &user.user_id,
            &secret_row,
            &req.code,
            &req.recovery_code,
        )?;
    } else if state.require_totp {
        // Enforced 2FA but this account has no confirmed secret. Instead of
        // locking the user out, hand back a short-lived enrolment token so they
        // can set up TOTP inline and finish login. Password is already verified.
        let (enroll_token, expires_in) = issue_totp_enroll_token(jwt_config, &user.user_id)
            .map_err(|_| status_err(StatusCode::INTERNAL_SERVER_ERROR))?;
        audit::record_event(
            store,
            "user",
            Some(&user.user_id),
            "auth.login_totp_enroll_required",
            "user",
            Some(&user.user_id),
        );
        return Ok((
            HeaderMap::new(),
            Json(LoginResponse::EnrollmentRequired(
                EnrollmentRequiredResponse {
                    enrollment_required: true,
                    enroll_token,
                    enroll_token_expires_in: expires_in,
                },
            )),
        ));
    }

    let tokens = mint_user_tokens(jwt_config, &user, store).map_err(status_err)?;
    let action = if !totp_enabled {
        "auth.login_success"
    } else if req.recovery_code.is_some() {
        "auth.login_totp_recovery_success"
    } else {
        "auth.login_totp_success"
    };
    audit::record_event(
        store,
        "user",
        Some(&user.user_id),
        action,
        "user",
        Some(&user.user_id),
    );
    let (headers, tokens) = deliver(tokens, delivery, secure, jwt_config.refresh_ttl_secs);
    Ok((headers, Json(LoginResponse::Tokens(tokens))))
}

/// `POST /v1/auth/login/totp` — exchange the MFA step-up token + a
/// 6-digit TOTP code (or single-use recovery code) for normal tokens.
pub async fn handle_totp_login(
    State(state): State<Arc<ServerState>>,
    ClientIp(peer_ip): ClientIp,
    headers: HeaderMap,
    Json(req): Json<TotpLoginRequest>,
) -> Result<(HeaderMap, Json<TokenResponse>), Response> {
    if !state.password_login_enabled {
        // TOTP login is the second step of the password flow; gate the same way.
        return Err(password_disabled_response());
    }
    if !state.login_throttle.allow_ip(peer_ip) {
        return Err(rate_limited_response());
    }
    let (delivery, secure) = resolve_delivery(req.refresh_cookie, &headers, &state);
    let store = state
        .store
        .as_ref()
        .ok_or_else(|| status_err(StatusCode::SERVICE_UNAVAILABLE))?;
    let jwt_config = state
        .jwt_config
        .as_ref()
        .ok_or_else(|| status_err(StatusCode::SERVICE_UNAVAILABLE))?;

    let user_id = validate_mfa_token(jwt_config, &req.mfa_token)
        .map_err(|_| status_err(StatusCode::UNAUTHORIZED))?;

    // Failure budget per mfa_token (issue #428): after MFA_MAX_FAILURES
    // wrong codes the token is dead for the rest of its 5-minute TTL and
    // the caller must redo the password step. Keyed by the token's hash;
    // the raw JWT never sits in the map.
    let mfa_key = hash_api_key(&req.mfa_token);
    if state.login_throttle.mfa_blocked(&mfa_key) {
        return Err(status_err(StatusCode::UNAUTHORIZED));
    }

    let user = store
        .users_get_by_id(&user_id)
        .map_err(|_| status_err(StatusCode::INTERNAL_SERVER_ERROR))?
        .ok_or_else(|| status_err(StatusCode::UNAUTHORIZED))?;
    if !user.is_active {
        return Err(status_err(StatusCode::FORBIDDEN));
    }

    let secret_row = store
        .totp_get(&user_id)
        .map_err(|_| status_err(StatusCode::INTERNAL_SERVER_ERROR))?
        .ok_or_else(|| status_err(StatusCode::UNAUTHORIZED))?;
    if !secret_row.enabled {
        return Err(status_err(StatusCode::UNAUTHORIZED));
    }

    // Accept either a current 6-digit TOTP code OR a single-use recovery
    // code (exactly one). Shared with the inline `/v1/auth/login` path.
    if let Err(resp) = verify_second_factor(
        jwt_config,
        store,
        &state.login_throttle,
        &user_id,
        &secret_row,
        &req.code,
        &req.recovery_code,
    ) {
        // Count only real second-factor rejections against the budget —
        // not malformed requests (400) or server errors (500).
        if resp.status() == StatusCode::UNAUTHORIZED {
            let failures = state.login_throttle.record_mfa_failure(&mfa_key);
            if failures == MFA_MAX_FAILURES {
                tracing::warn!(
                    user_id,
                    "mfa_token invalidated after {failures} failed second-factor attempts"
                );
                audit::record_event(
                    store,
                    "user",
                    Some(&user_id),
                    "auth.totp_throttled",
                    "user",
                    Some(&user_id),
                );
            }
        }
        return Err(resp);
    }
    state.login_throttle.clear_mfa(&mfa_key);

    let tokens = mint_user_tokens(jwt_config, &user, store).map_err(status_err)?;
    let action = if req.recovery_code.is_some() {
        "auth.login_totp_recovery_success"
    } else {
        "auth.login_totp_success"
    };
    audit::record_event(
        store,
        "user",
        Some(&user.user_id),
        action,
        "user",
        Some(&user.user_id),
    );
    let (headers, tokens) = deliver(tokens, delivery, secure, jwt_config.refresh_ttl_secs);
    Ok((headers, Json(tokens)))
}

// ─── Forced TOTP enrolment (login flow) ─────────────────────────────────────

#[derive(Deserialize)]
pub struct EnrollTotpBeginRequest {
    pub enroll_token: String,
}

#[derive(Deserialize)]
pub struct EnrollTotpConfirmRequest {
    pub enroll_token: String,
    pub code: String,
    /// See [`LoginRequest::refresh_cookie`].
    #[serde(default)]
    pub refresh_cookie: bool,
}

/// `POST /v1/auth/login/enroll/totp/begin` — start inline TOTP enrolment for a
/// user who hit enforced 2FA without a secret. Validates the short-lived
/// `enroll_token` (from the login `EnrollmentRequired` response) and returns
/// the once-shown secret / QR / recovery codes. Public + unauthenticated — the
/// token (issued only after a verified password) is the proof.
pub async fn handle_enroll_totp_begin(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<EnrollTotpBeginRequest>,
) -> Result<Json<super::totp::SetupResponse>, Response> {
    if !state.password_login_enabled {
        return Err(password_disabled_response());
    }
    let store = state
        .store
        .as_ref()
        .ok_or_else(|| status_err(StatusCode::SERVICE_UNAVAILABLE))?;
    let jwt_config = state
        .jwt_config
        .as_ref()
        .ok_or_else(|| status_err(StatusCode::SERVICE_UNAVAILABLE))?;

    let user_id = validate_totp_enroll_token(jwt_config, &req.enroll_token)
        .map_err(|_| status_err(StatusCode::UNAUTHORIZED))?;
    let user = store
        .users_get_by_id(&user_id)
        .map_err(|_| status_err(StatusCode::INTERNAL_SERVER_ERROR))?
        .ok_or_else(|| status_err(StatusCode::UNAUTHORIZED))?;
    if !user.is_active {
        return Err(status_err(StatusCode::FORBIDDEN));
    }
    // Already enrolled? Then no enrol token should exist for this account — refuse.
    if store
        .totp_get(&user_id)
        .ok()
        .flatten()
        .map(|t| t.enabled)
        .unwrap_or(false)
    {
        return Err(status_err(StatusCode::CONFLICT));
    }

    let resp =
        super::totp::create_pending_enrollment(store, &jwt_config.secret, &user_id, &user.username)
            .map_err(status_err)?;
    Ok(Json(resp))
}

/// `POST /v1/auth/login/enroll/totp/confirm` — verify the first TOTP code,
/// enable 2FA, and complete login by minting tokens. Same `enroll_token`.
pub async fn handle_enroll_totp_confirm(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(req): Json<EnrollTotpConfirmRequest>,
) -> Result<(HeaderMap, Json<TokenResponse>), Response> {
    if !state.password_login_enabled {
        return Err(password_disabled_response());
    }
    let (delivery, secure) = resolve_delivery(req.refresh_cookie, &headers, &state);
    let store = state
        .store
        .as_ref()
        .ok_or_else(|| status_err(StatusCode::SERVICE_UNAVAILABLE))?;
    let jwt_config = state
        .jwt_config
        .as_ref()
        .ok_or_else(|| status_err(StatusCode::SERVICE_UNAVAILABLE))?;

    let user_id = validate_totp_enroll_token(jwt_config, &req.enroll_token)
        .map_err(|_| status_err(StatusCode::UNAUTHORIZED))?;
    let user = store
        .users_get_by_id(&user_id)
        .map_err(|_| status_err(StatusCode::INTERNAL_SERVER_ERROR))?
        .ok_or_else(|| status_err(StatusCode::UNAUTHORIZED))?;
    if !user.is_active {
        return Err(status_err(StatusCode::FORBIDDEN));
    }

    super::totp::confirm_pending_enrollment(store, &jwt_config.secret, &user_id, &req.code)
        .map_err(status_err)?;
    audit::record_event(
        store,
        "user",
        Some(&user.user_id),
        "auth.login_totp_enrolled",
        "user",
        Some(&user.user_id),
    );
    let tokens = mint_user_tokens(jwt_config, &user, store).map_err(status_err)?;
    let (headers, tokens) = deliver(tokens, delivery, secure, jwt_config.refresh_ttl_secs);
    Ok((headers, Json(tokens)))
}

/// Mint + persist an access/refresh pair for a fully-authenticated
/// user. Pulled out so both the no-2FA login path and the TOTP-success
/// path can call it.
fn mint_user_tokens(
    jwt_config: &croniq_auth::jwt::JwtConfig,
    user: &croniq_store::models::User,
    store: &crate::store::DynStore,
) -> Result<TokenResponse, StatusCode> {
    let scopes = default_scopes_for_role(user.role);
    // Stamp the user's current credential generation into the token so the
    // auth middleware can invalidate it on the next password change, reset or
    // deactivation (issue #431).
    let token_generation = store
        .users_token_generation(&user.user_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let pair = issue_token_pair(
        jwt_config,
        &user.user_id,
        &user.user_id,
        CallerType::User,
        Some(&user.user_id),
        Some(user.role),
        AuthMethod::Password,
        &scopes,
        token_generation,
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let _ = store.users_set_last_login(&user.user_id, Utc::now());

    let refresh_hash = hash_api_key(&pair.refresh_token);
    let _ = store.create_refresh_token(&RefreshToken {
        token_hash: refresh_hash,
        client_id: user.user_id.clone(),
        user_id: Some(user.user_id.clone()),
        expires_at: pair.refresh_expires_at,
        revoked_at: None,
        created_at: Utc::now(),
    });

    Ok(TokenResponse {
        access_token: pair.access_token,
        refresh_token: Some(pair.refresh_token),
        token_type: "Bearer".into(),
        expires_in: jwt_config.access_ttl_secs,
    })
}

/// Write a seed that only unwrapped under `CRONIQ_JWT_SECRET_PREVIOUS` back
/// under the current key (issue #531).
///
/// Best-effort by design. This runs on a login that has already succeeded at
/// decrypting the seed, so the user gets in either way; a store that refuses
/// the write is the boot sweep's problem to report, not a reason to fail the
/// sign-in. The row is written whole so `enabled` and `confirmed_at` survive —
/// `totp_upsert` updates all three columns.
fn rewrap_after_fallback(
    store: &crate::store::DynStore,
    user_id: &str,
    secret_row: &croniq_store::models::TotpSecret,
    current_secret: &str,
    plaintext: &[u8],
) {
    let Ok(rewrapped) = croniq_auth::crypto::wrap_totp_secret(current_secret, plaintext) else {
        return;
    };
    let updated = croniq_store::models::TotpSecret {
        secret_enc: rewrapped,
        ..secret_row.clone()
    };
    if let Err(e) = store.totp_upsert(&updated) {
        tracing::error!(
            user_id,
            error = %e,
            "could not re-wrap the stored TOTP secret under the current JWT secret; it \
             stays under the previous one and keeps working only while \
             CRONIQ_JWT_SECRET_PREVIOUS is set"
        );
    }
}

/// Verify a supplied second factor — exactly one of `code` (current 6-digit
/// TOTP) or `recovery_code` (single-use) — against the user's enabled secret.
/// Recovery codes are marked consumed here, *before* any token is minted, so
/// a parallel retry can't double-spend. Shared by the inline
/// `/v1/auth/login` path and the two-step `/v1/auth/login/totp` exchange.
// Carries a full `Response` as its error, like the login handlers it backs;
// the large Err is intentional, so boxing it would only churn the call sites.
#[allow(clippy::result_large_err)]
fn verify_second_factor(
    jwt_config: &croniq_auth::jwt::JwtConfig,
    store: &crate::store::DynStore,
    throttle: &LoginThrottle,
    user_id: &str,
    secret_row: &croniq_store::models::TotpSecret,
    code: &Option<String>,
    recovery_code: &Option<String>,
) -> Result<(), Response> {
    match (code, recovery_code) {
        (Some(code), None) => {
            // A failure here is almost always a changed JWT secret, not a
            // corrupt row: the at-rest wrap key is HKDF-derived from it. The
            // error used to be discarded, leaving a bare 500 that the login
            // page rendered as "cannot reach server" — so the real cause was
            // invisible on every surface (issue #408). Log it with the user id
            // so the operator can see the scope.
            let (raw, under) = unwrap_totp_secret_with_previous(
                &jwt_config.secret,
                jwt_config.previous_secret.as_deref(),
                &secret_row.secret_enc,
            )
            .map_err(|e| {
                tracing::error!(
                    user_id,
                    error = %e,
                    "stored TOTP secret could not be unwrapped — the JWT secret has most \
                     likely changed since enrolment. Name the outgoing value in \
                     CRONIQ_JWT_SECRET_PREVIOUS and restart to re-wrap the stored secrets \
                     (issue #531), restore it as CRONIQ_JWT_SECRET, or have the user sign in \
                     with a recovery code and re-enrol. `croniq-server doctor` reports this \
                     as totp.secrets_undecryptable."
                );
                status_err(StatusCode::INTERNAL_SERVER_ERROR)
            })?;
            if under == WrappedUnder::Previous {
                // The boot sweep re-wraps every stored secret before the
                // server accepts traffic, so reaching this means it could not
                // write *this* row. Say so rather than letting the fallback
                // quietly paper over a half-finished rotation, and re-wrap
                // opportunistically — the login already paid for the unwrap.
                tracing::warn!(
                    user_id,
                    "stored TOTP secret is still under CRONIQ_JWT_SECRET_PREVIOUS; the \
                     boot-time re-wrap did not cover it. Re-wrapping it now — keep the \
                     variable set until a restart reports nothing left under the old key."
                );
                rewrap_after_fallback(store, user_id, secret_row, &jwt_config.secret, &raw);
            }
            let secret_b32 = String::from_utf8(raw).map_err(|_| {
                tracing::error!(user_id, "decrypted TOTP secret is not valid UTF-8");
                status_err(StatusCode::INTERNAL_SERVER_ERROR)
            })?;
            match verify_code_with_step(&secret_b32, code) {
                // A valid code is consumable once: the matched 30-second
                // time step is recorded per user, and any code from a step
                // at or below the last consumed one is rejected. Without
                // this, a code observed in transit stays valid for the
                // rest of its ±1-step skew window (issue #428).
                Ok(Some(step)) if throttle.consume_totp_step(user_id, step) => Ok(()),
                _ => Err(status_err(StatusCode::UNAUTHORIZED)),
            }
        }
        (None, Some(recovery)) => {
            let hash = hash_recovery_code(recovery);
            let row = store
                .recovery_codes_find_unused(user_id, &hash)
                .map_err(|_| status_err(StatusCode::INTERNAL_SERVER_ERROR))?
                .ok_or_else(|| status_err(StatusCode::UNAUTHORIZED))?;
            store
                .recovery_codes_mark_used(&row.code_id, Utc::now())
                .map_err(|_| status_err(StatusCode::INTERNAL_SERVER_ERROR))?;
            Ok(())
        }
        // Both set or neither — caller must supply exactly one.
        _ => Err(status_err(StatusCode::BAD_REQUEST)),
    }
}

/// `POST /v1/auth/refresh`
///
/// Accepts the refresh token from the request body (every client before #454)
/// or from the `croniq_refresh` cookie (the dashboard SPA). The response
/// **mirrors the source**: a body token yields a body token, a cookie yields a
/// rotated cookie and no `refresh_token` field at all. Without that
/// equivalence the cookie would buy nothing — an XSS could POST here, have the
/// browser attach the `HttpOnly` cookie for it, and read the fresh 7-day token
/// out of the response.
pub async fn handle_refresh(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(req): Json<RefreshRequest>,
) -> Result<(HeaderMap, Json<TokenResponse>), Response> {
    let store = state
        .store
        .as_ref()
        .ok_or_else(|| status_err(StatusCode::SERVICE_UNAVAILABLE))?;
    let jwt_config = state
        .jwt_config
        .as_ref()
        .ok_or_else(|| status_err(StatusCode::SERVICE_UNAVAILABLE))?;

    let (presented, delivery) = match req.refresh_token {
        Some(body_token) => (body_token, refresh_cookie::Delivery::Body),
        None => match refresh_cookie::read(&headers) {
            Some(cookie_token) => (cookie_token, refresh_cookie::Delivery::Cookie),
            // Neither source. 401 rather than 400: "you have no session" is
            // exactly what the SPA's bootstrap refresh needs to hear on a
            // first visit, and it is indistinguishable from a stale token.
            None => return Err(status_err(StatusCode::UNAUTHORIZED)),
        },
    };
    let secure = refresh_cookie::is_secure_request(&headers, state.app_base_url.as_deref());

    let token_hash = hash_api_key(&presented);
    let token = store
        .validate_refresh_token(&token_hash)
        .map_err(|_| status_err(StatusCode::INTERNAL_SERVER_ERROR))?
        .ok_or_else(|| status_err(StatusCode::UNAUTHORIZED))?;

    if Utc::now() > token.expires_at {
        return Err(status_err(StatusCode::UNAUTHORIZED));
    }

    // Revoke old token
    let _ = store.revoke_refresh_token(&token_hash, Utc::now());

    // Branch on caller type. User refresh re-loads the user row so role
    // changes propagate without forcing a re-login; API-key refresh
    // picks up scope changes on the owning client the same way.
    let (caller_type, user_id, role, auth_method, scopes) = if let Some(uid) = &token.user_id {
        let user = store
            .users_get_by_id(uid)
            .map_err(|_| status_err(StatusCode::INTERNAL_SERVER_ERROR))?
            .ok_or_else(|| status_err(StatusCode::UNAUTHORIZED))?;
        if !user.is_active {
            return Err(status_err(StatusCode::FORBIDDEN));
        }
        let scopes = default_scopes_for_role(user.role);
        (
            CallerType::User,
            Some(user.user_id.clone()),
            Some(user.role),
            AuthMethod::Password,
            scopes,
        )
    } else {
        // A row with no user belongs to a machine client minted by
        // `POST /v1/api-clients/{id}/tokens`. Re-resolve the client so the
        // rotated token reflects the current row: narrowed scopes take
        // effect, a deactivated client stops refreshing, and a deleted one
        // 401s instead of silently rotating into a scope-less token.
        let client = store
            .get_client(&token.client_id)
            .map_err(|_| status_err(StatusCode::INTERNAL_SERVER_ERROR))?
            .ok_or_else(|| status_err(StatusCode::UNAUTHORIZED))?;
        if !client.is_active {
            return Err(status_err(StatusCode::FORBIDDEN));
        }
        (
            CallerType::ApiKey,
            None,
            None,
            AuthMethod::ApiKey,
            client.scopes,
        )
    };

    let caller_id = user_id.clone().unwrap_or_else(|| token.client_id.clone());
    // Re-read the generation rather than carrying it over: a refresh that
    // happens after a password change must mint a token for the *new*
    // generation, not resurrect the old one (issue #431).
    let token_generation = match user_id.as_deref() {
        Some(uid) => store
            .users_token_generation(uid)
            .map_err(|_| status_err(StatusCode::INTERNAL_SERVER_ERROR))?,
        None => None,
    };
    let pair = issue_token_pair(
        jwt_config,
        &caller_id,
        &token.client_id,
        caller_type,
        user_id.as_deref(),
        role,
        auth_method,
        &scopes,
        token_generation,
    )
    .map_err(|_| status_err(StatusCode::INTERNAL_SERVER_ERROR))?;

    let new_hash = hash_api_key(&pair.refresh_token);
    let _ = store.create_refresh_token(&RefreshToken {
        token_hash: new_hash,
        client_id: token.client_id,
        user_id: token.user_id,
        expires_at: pair.refresh_expires_at,
        revoked_at: None,
        created_at: Utc::now(),
    });

    let tokens = TokenResponse {
        access_token: pair.access_token,
        refresh_token: Some(pair.refresh_token),
        token_type: "Bearer".into(),
        expires_in: jwt_config.access_ttl_secs,
    };
    let (headers, tokens) = deliver(tokens, delivery, secure, jwt_config.refresh_ttl_secs);
    Ok((headers, Json(tokens)))
}

/// `POST /v1/auth/logout`
///
/// Revokes the presented refresh token — from the body, the cookie, or both —
/// and always answers with a cookie-clearing `Set-Cookie`. Clearing
/// unconditionally means a caller whose cookie is already unknown to the store
/// (revoked, expired, or minted before a JWT-secret change) still ends up with
/// a clean browser rather than a cookie that will 401 forever.
pub async fn handle_logout(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(req): Json<LogoutRequest>,
) -> (HeaderMap, StatusCode) {
    let secure = refresh_cookie::is_secure_request(&headers, state.app_base_url.as_deref());
    let clearing = refresh_cookie::header_map(refresh_cookie::clear(secure));
    let Some(store) = state.store.as_ref() else {
        return (clearing, StatusCode::SERVICE_UNAVAILABLE);
    };
    let now = Utc::now();
    for raw in [req.refresh_token, refresh_cookie::read(&headers)]
        .into_iter()
        .flatten()
    {
        let _ = store.revoke_refresh_token(&hash_api_key(&raw), now);
    }
    (clearing, StatusCode::NO_CONTENT)
}

/// `managed_by` value for a client the environment declares (issue #471).
///
/// Re-exported from the module that owns the declaration grammar so the two
/// cannot disagree about the marker they both compare against.
pub use crate::api_client_env::MANAGED_BY_ENV;
/// `managed_by` value for a client created through the API or dashboard.
pub const MANAGED_BY_API: &str = "api";

/// Error half of the API-client mutation handlers.
///
/// The env-managed refusal has to say *which* variable owns the row — a bare
/// 409 leaves the operator with nothing to act on — while the existing
/// authorization, not-found and validation paths stay plain statuses.
#[derive(Debug)]
pub enum ApiClientError {
    Status(StatusCode),
    WithBody(StatusCode, Json<ValidationError>),
}

impl ApiClientError {
    pub fn status(&self) -> StatusCode {
        match self {
            Self::Status(s) | Self::WithBody(s, _) => *s,
        }
    }
}

/// Lets a test assert against the status without unwrapping the variant —
/// the body is an operator-facing detail, the status is the contract.
impl PartialEq<StatusCode> for ApiClientError {
    fn eq(&self, other: &StatusCode) -> bool {
        self.status() == *other
    }
}

impl From<StatusCode> for ApiClientError {
    fn from(s: StatusCode) -> Self {
        Self::Status(s)
    }
}

impl From<(StatusCode, Json<ValidationError>)> for ApiClientError {
    fn from((status, body): (StatusCode, Json<ValidationError>)) -> Self {
        Self::WithBody(status, body)
    }
}

impl IntoResponse for ApiClientError {
    fn into_response(self) -> Response {
        match self {
            Self::Status(s) => s.into_response(),
            Self::WithBody(s, body) => (s, body).into_response(),
        }
    }
}

/// Refuse a mutation that the environment owns.
///
/// For an env-declared client the environment is the source of truth and the
/// reconciler re-applies it on every explicit reload. Accepting an API edit
/// would mean the change survives until the next `SIGHUP` and then silently
/// reverts — drift that is invisible from both the dashboard and the env file.
/// Refusing names the variable to edit instead.
fn refuse_env_managed(
    client: &ApiClient,
    consequence: &str,
) -> Option<(StatusCode, Json<ValidationError>)> {
    if client.managed_by != MANAGED_BY_ENV {
        return None;
    }
    // Reconstructing the variable name here got the `default` client wrong: it
    // is declared by CRONIQ_API_KEY, not under the named-client prefix, so the
    // advice sent operators to add a second declaration of the same client —
    // which the next boot refuses outright (issue #481).
    let var = crate::api_client_env::declaring_key_var(&client.name);
    Some((
        StatusCode::CONFLICT,
        Json(ValidationError {
            error: "env_managed",
            message: format!(
                "API client '{name}' is declared in the environment ({var}) and is owned \
                 by it, so {consequence}. Change the environment (or the file \
                 {var}_FILE points at) and reload with SIGHUP or \
                 POST /v1/admin/reload-config. To hand the client back to the API, remove \
                 its environment declaration and restart.",
                name = client.name
            ),
        }),
    ))
}

/// `GET /v1/api-clients`
pub async fn handle_list_clients(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
) -> Result<Json<Vec<ApiClient>>, StatusCode> {
    require_scope(&ctx, Scope::API_CLIENTS_ADMIN)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let clients = store
        .list_clients()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(clients))
}

/// `POST /v1/api-clients`
pub async fn handle_create_client(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Json(req): Json<CreateClientRequest>,
) -> Result<(StatusCode, Json<CreateClientResponse>), StatusCode> {
    require_scope(&ctx, Scope::API_CLIENTS_ADMIN)?;
    if req.name.trim().is_empty() || req.scopes.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    // `api-clients:admin` provisions clients; it does not confer the
    // right to provision one that outranks the caller.
    require_grantable_scopes(&ctx, &req.scopes)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let client_id = Uuid::new_v4().to_string();
    let client = ApiClient {
        client_id: client_id.clone(),
        name: req.name.clone(),
        scopes: req.scopes.clone(),
        is_active: true,
        created_at: Utc::now(),
        managed_by: MANAGED_BY_API.to_string(),
    };
    store
        .create_client(&client)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((
        StatusCode::CREATED,
        Json(CreateClientResponse {
            client_id,
            name: req.name,
            scopes: req.scopes,
        }),
    ))
}

#[derive(Deserialize)]
pub struct UpdateClientRequest {
    pub name: Option<String>,
    pub scopes: Option<Vec<String>>,
    pub is_active: Option<bool>,
}

/// `PUT /v1/api-clients/{id}` — patch name, scopes, or active flag.
/// Omitted fields are left untouched. Empty `scopes` is rejected — a
/// scope-less client can't authorise anything and is almost always a
/// mistake.
pub async fn handle_update_client(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(client_id): axum::extract::Path<String>,
    Json(req): Json<UpdateClientRequest>,
) -> Result<Json<ApiClient>, ApiClientError> {
    require_scope(&ctx, Scope::API_CLIENTS_ADMIN)?;
    if let Some(ref scopes) = req.scopes
        && scopes.is_empty()
    {
        return Err(StatusCode::BAD_REQUEST.into());
    }
    if let Some(ref scopes) = req.scopes {
        require_grantable_scopes(&ctx, scopes)?;
    }
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let mut client = store
        .get_client(&client_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    if let Some(refusal) = refuse_env_managed(
        &client,
        "an edit made here would be reverted by the next reconcile",
    ) {
        return Err(refusal.into());
    }
    if let Some(name) = req.name {
        if name.trim().is_empty() {
            return Err(StatusCode::BAD_REQUEST.into());
        }
        client.name = name;
    }
    if let Some(scopes) = req.scopes {
        client.scopes = scopes;
    }
    if let Some(active) = req.is_active {
        client.is_active = active;
    }
    store
        .create_client(&client)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(client))
}

/// Whether `DELETE /v1/api-clients/{id}` may proceed, given what the store
/// said about the client.
///
/// Three outcomes, and the distinction between the first two is the point:
///
/// * `Err(_)` — the store could not answer. **Not** evidence of absence. The
///   handler used to read it as one (`if let Ok(Some(_))`), which skipped the
///   env-managed refusal on a transient lock or IO failure and deleted an
///   env-owned client anyway, reporting `204` for it (issue #504).
/// * `Ok(None)` — already gone. Delete was idempotent before the refusal
///   existed and there is no reason for it to stop being.
/// * `Ok(Some(_))` — refuse if the environment owns it, otherwise proceed.
fn deletion_guard(lookup: Result<Option<ApiClient>, StoreError>) -> Result<(), ApiClientError> {
    let Some(client) = lookup.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)? else {
        return Ok(());
    };
    // Deleting an env-declared client is worse than a no-op: the next
    // reconcile recreates it, so the operator would watch it reappear with no
    // explanation. Deletion happens by removing the declaration.
    match refuse_env_managed(&client, "the next reconcile would recreate it") {
        Some(refusal) => Err(refusal.into()),
        None => Ok(()),
    }
}

/// `DELETE /v1/api-clients/{id}`
pub async fn handle_delete_client(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(client_id): axum::extract::Path<String>,
) -> Result<StatusCode, ApiClientError> {
    require_scope(&ctx, Scope::API_CLIENTS_ADMIN)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    // Deleting an env-declared client is worse than a no-op: the next
    // reconcile recreates it, so the operator would watch it reappear with no
    // explanation. Deletion happens by removing the declaration.
    //
    deletion_guard(store.get_client(&client_id))?;
    store
        .delete_client(&client_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /v1/api-clients/{id}/tokens`
pub async fn handle_issue_client_token(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(client_id): axum::extract::Path<String>,
) -> Result<Json<TokenResponse>, StatusCode> {
    require_scope(&ctx, Scope::API_CLIENTS_ADMIN)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let jwt_config = state
        .jwt_config
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let client = store
        .get_client(&client_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    // The token inherits the client's scopes verbatim, so issuing one for
    // a client that outranks the caller is a straight escalation — the
    // guard on client creation alone would leave pre-existing clients
    // (the bootstrap `admin` client, for one) as a way around it.
    require_grantable_scopes(&ctx, &client.scopes)?;

    let pair = issue_token_pair(
        jwt_config,
        &client.client_id,
        &client.client_id,
        CallerType::ApiKey,
        None,
        None,
        AuthMethod::ApiKey,
        &client.scopes,
        // API-key callers have no user row, so no generation applies.
        None,
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Persist the refresh half, exactly like `mint_user_tokens` and the OIDC
    // callback do. Without a `refresh_tokens` row the token handed out below
    // could never be redeemed — `handle_refresh` looks the presented hash up
    // and 401s when it finds nothing (issue #463). `user_id` stays `None`:
    // that is what marks the row as belonging to a machine client and sends
    // `handle_refresh` down its API-key branch.
    let refresh_hash = hash_api_key(&pair.refresh_token);
    store
        .create_refresh_token(&RefreshToken {
            token_hash: refresh_hash,
            client_id: client.client_id.clone(),
            user_id: None,
            expires_at: pair.refresh_expires_at,
            revoked_at: None,
            created_at: Utc::now(),
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Body delivery, always: this endpoint is called by an authenticated admin
    // tool to provision a machine credential, never by a browser session that
    // could hold a cookie.
    Ok(Json(TokenResponse {
        access_token: pair.access_token,
        refresh_token: Some(pair.refresh_token),
        token_type: "Bearer".into(),
        expires_in: jwt_config.access_ttl_secs,
    }))
}

/// One row of `GET /v1/api-keys`.
///
/// Deliberately not the `ApiKey` model: that carries `key_hash`, and an
/// endpoint whose whole job is to be readable by an operator must not hand
/// out the hash of a live credential. `key_prefix` is the first 12
/// characters of the raw key, which is what makes a row recognisable
/// without being usable.
#[derive(Serialize)]
pub struct ApiKeySummary {
    pub key_id: String,
    pub client_id: String,
    pub key_prefix: String,
    pub created_at: chrono::DateTime<Utc>,
    /// Set on a key that a rotation retired: it keeps authenticating until
    /// this instant (see `CRONIQ_API_KEY_ROTATION_GRACE`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<chrono::DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<chrono::DateTime<Utc>>,
}

impl From<ApiKey> for ApiKeySummary {
    fn from(k: ApiKey) -> Self {
        Self {
            key_id: k.key_id,
            client_id: k.client_id,
            key_prefix: k.key_prefix,
            created_at: k.created_at,
            expires_at: k.expires_at,
            revoked_at: k.revoked_at,
        }
    }
}

#[derive(Deserialize)]
pub struct ListApiKeysQuery {
    pub client_id: String,
}

/// `GET /v1/api-keys?client_id=…`
///
/// Without this there is no way to see which keys a client has: the raw
/// value is shown once at creation and the `key_id` needed to revoke one
/// was only ever available in that same response. That makes both auditing
/// ("which credentials are live?") and the break-glass revoke after a
/// rotation impossible from an API-only deployment.
///
/// Newest first, revoked and expired rows included — a listing that hid
/// them would answer the audit question wrongly.
pub async fn handle_list_api_keys(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Query(query): axum::extract::Query<ListApiKeysQuery>,
) -> Result<Json<Vec<ApiKeySummary>>, StatusCode> {
    require_scope(&ctx, Scope::API_KEYS_ADMIN)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    // 404 on an unknown client rather than an empty list: "this client has
    // no keys" and "you typed the id wrong" are different problems and an
    // operator chasing a broken credential needs to tell them apart.
    let client = store
        .get_client(&query.client_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    // Listing a client's keys exposes how its credentials are provisioned,
    // so it is bounded by the caller's own scopes exactly like issuing one.
    require_grantable_scopes(&ctx, &client.scopes)?;

    let keys = store
        .list_api_keys(&query.client_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(keys.into_iter().map(ApiKeySummary::from).collect()))
}

/// `POST /v1/api-keys`
pub async fn handle_create_api_key(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Json(req): Json<CreateApiKeyClientRequest>,
) -> Result<(StatusCode, Json<CreateApiKeyResponse>), ApiClientError> {
    require_scope(&ctx, Scope::API_KEYS_ADMIN)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    // Verify client exists
    let client = store
        .get_client(&req.client_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    // An env-declared client's key value comes from the environment, and the
    // reconciler retires every key that is not the declared one. A key minted
    // here would therefore stop working at the next reconcile — issue it and
    // the operator has a credential with a silent expiry date.
    if let Some(refusal) = refuse_env_managed(
        &client,
        "a key minted here would be retired by the next reconcile, which keeps only \
         the declared one",
    ) {
        return Err(refusal.into());
    }
    // The key authenticates as the client and therefore carries the
    // client's scopes; bound them by the caller's own.
    require_grantable_scopes(&ctx, &client.scopes)?;

    let (raw_key, key_hash, prefix) = generate_api_key();
    let key_id = Uuid::new_v4().to_string();

    store
        .create_api_key(&ApiKey {
            key_id: key_id.clone(),
            client_id: req.client_id.clone(),
            key_hash,
            key_prefix: prefix.clone(),
            expires_at: None,
            revoked_at: None,
            created_at: Utc::now(),
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok((
        StatusCode::CREATED,
        Json(CreateApiKeyResponse {
            raw_key,
            key_id,
            key_prefix: prefix,
            client_id: req.client_id,
        }),
    ))
}

#[derive(Deserialize)]
pub struct CreateApiKeyClientRequest {
    pub client_id: String,
}

/// `DELETE /v1/api-keys/{id}`
pub async fn handle_revoke_api_key(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(key_id): axum::extract::Path<String>,
) -> StatusCode {
    if let Err(s) = require_scope(&ctx, Scope::API_KEYS_ADMIN) {
        return s;
    }
    let Some(store) = state.store.as_ref() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let _ = store.revoke_api_key(&key_id, Utc::now());
    StatusCode::NO_CONTENT
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{DynStore, sqlite_store};
    use croniq_auth::jwt::JwtConfig;
    use croniq_runner::AppState;
    use croniq_store::sqlite::SqliteStore;
    use tokio::sync::mpsc;

    fn make_store() -> DynStore {
        sqlite_store(SqliteStore::in_memory().unwrap())
    }

    fn state_with(store: &DynStore) -> Arc<ServerState> {
        let (tx, _rx) = mpsc::unbounded_channel();
        ServerState::with_auth(
            AppState::new(),
            tx,
            Some(JwtConfig::for_tests()),
            Some(Arc::clone(store)),
        )
    }

    /// An API-key caller holding exactly `scopes` — the shape of a
    /// deliberately narrow provisioning credential.
    fn key_ctx(scopes: &[&str]) -> CallerContext {
        CallerContext {
            caller_type: CallerType::ApiKey,
            caller_id: "key-1".into(),
            client_id: "client-1".into(),
            user_id: None,
            role: None,
            auth_method: AuthMethod::ApiKey,
            scopes: scopes.iter().map(|s| s.to_string()).collect(),
            token_generation: None,
        }
    }

    fn seed_client(store: &DynStore, client_id: &str, scopes: &[&str]) {
        seed_client_owned(store, client_id, scopes, MANAGED_BY_API);
    }

    fn seed_client_owned(store: &DynStore, client_id: &str, scopes: &[&str], managed_by: &str) {
        store
            .create_client(&ApiClient {
                client_id: client_id.into(),
                name: client_id.into(),
                scopes: scopes.iter().map(|s| s.to_string()).collect(),
                is_active: true,
                created_at: Utc::now(),
                managed_by: managed_by.into(),
            })
            .unwrap();
    }

    #[tokio::test]
    async fn client_creation_is_bounded_by_the_callers_scopes() {
        let store = make_store();
        let state = state_with(&store);
        let ctx = key_ctx(&[Scope::API_CLIENTS_ADMIN, Scope::JOBS_READ]);

        let Err(status) = handle_create_client(
            State(Arc::clone(&state)),
            Extension(ctx.clone()),
            Json(CreateClientRequest {
                name: "escalated".into(),
                scopes: vec!["admin".into()],
            }),
        )
        .await
        else {
            panic!("provisioning an admin client must be refused");
        };
        assert_eq!(status, StatusCode::FORBIDDEN);

        // A client within the caller's own scopes is still fine.
        let (status, Json(body)) = handle_create_client(
            State(state),
            Extension(ctx),
            Json(CreateClientRequest {
                name: "reader".into(),
                scopes: vec![Scope::JOBS_READ.into()],
            }),
        )
        .await
        .expect("granting a held scope is allowed");
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body.scopes, vec![Scope::JOBS_READ]);
    }

    #[tokio::test]
    async fn admin_caller_may_still_create_an_admin_client() {
        let store = make_store();
        let (status, Json(body)) = handle_create_client(
            State(state_with(&store)),
            Extension(key_ctx(&["admin"])),
            Json(CreateClientRequest {
                name: "root".into(),
                scopes: vec!["admin".into()],
            }),
        )
        .await
        .expect("an admin caller is unrestricted");
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body.scopes, vec!["admin"]);
    }

    #[tokio::test]
    async fn client_update_cannot_widen_scopes_past_the_caller() {
        let store = make_store();
        seed_client(&store, "c1", &[Scope::JOBS_READ]);
        let state = state_with(&store);
        let ctx = key_ctx(&[Scope::API_CLIENTS_ADMIN, Scope::JOBS_READ]);

        let Err(status) = handle_update_client(
            State(Arc::clone(&state)),
            Extension(ctx.clone()),
            axum::extract::Path("c1".into()),
            Json(UpdateClientRequest {
                name: None,
                scopes: Some(vec!["admin".into()]),
                is_active: None,
            }),
        )
        .await
        else {
            panic!("widening an existing client must be refused");
        };
        assert_eq!(status, StatusCode::FORBIDDEN);

        // The stored row is untouched.
        let stored = store.get_client("c1").unwrap().unwrap();
        assert_eq!(stored.scopes, vec![Scope::JOBS_READ]);

        // A rename leaves the scopes alone and is still allowed.
        let Json(updated) = handle_update_client(
            State(state),
            Extension(ctx),
            axum::extract::Path("c1".into()),
            Json(UpdateClientRequest {
                name: Some("renamed".into()),
                scopes: None,
                is_active: None,
            }),
        )
        .await
        .expect("a rename is not an escalation");
        assert_eq!(updated.name, "renamed");
        assert_eq!(updated.scopes, vec![Scope::JOBS_READ]);
    }

    #[tokio::test]
    async fn issuing_a_token_for_a_wider_client_is_refused() {
        let store = make_store();
        // The bootstrap client every install ships with.
        seed_client(&store, "root", &["admin"]);
        seed_client(&store, "reader", &[Scope::JOBS_READ]);
        let state = state_with(&store);
        let ctx = key_ctx(&[Scope::API_CLIENTS_ADMIN, Scope::JOBS_READ]);

        let Err(status) = handle_issue_client_token(
            State(Arc::clone(&state)),
            Extension(ctx.clone()),
            axum::extract::Path("root".into()),
        )
        .await
        else {
            panic!("minting an admin token must be refused");
        };
        assert_eq!(status, StatusCode::FORBIDDEN);

        let _ = handle_issue_client_token(
            State(state),
            Extension(ctx),
            axum::extract::Path("reader".into()),
        )
        .await
        .expect("a client within the caller's scopes still issues");
    }

    /// The endpoint advertises a `refresh_token`, so it has to leave a row
    /// behind for `handle_refresh` to find — issue #463. `user_id: None` is
    /// the part that matters: it is what routes the redemption down the
    /// API-key branch instead of looking for a user that does not exist.
    #[tokio::test]
    async fn issuing_a_client_token_persists_its_refresh_half() {
        let store = make_store();
        seed_client(&store, "reader", &[Scope::JOBS_READ]);
        let state = state_with(&store);
        let ctx = key_ctx(&[Scope::API_CLIENTS_ADMIN, Scope::JOBS_READ]);

        let Json(tokens) = handle_issue_client_token(
            State(state),
            Extension(ctx),
            axum::extract::Path("reader".into()),
        )
        .await
        .expect("issuing within the caller's scopes succeeds");

        let raw = tokens
            .refresh_token
            .expect("the endpoint returns a refresh token");
        let row = store
            .validate_refresh_token(&hash_api_key(&raw))
            .unwrap()
            .expect("the returned refresh token must be redeemable");
        assert_eq!(row.client_id, "reader");
        assert!(
            row.user_id.is_none(),
            "a machine credential has no owning user"
        );
    }

    #[tokio::test]
    async fn api_key_creation_is_bounded_by_the_target_clients_scopes() {
        let store = make_store();
        seed_client(&store, "root", &["admin"]);
        seed_client(&store, "reader", &[Scope::JOBS_READ]);
        let state = state_with(&store);
        let ctx = key_ctx(&[Scope::API_KEYS_ADMIN, Scope::JOBS_READ]);

        let Err(status) = handle_create_api_key(
            State(Arc::clone(&state)),
            Extension(ctx.clone()),
            Json(CreateApiKeyClientRequest {
                client_id: "root".into(),
            }),
        )
        .await
        else {
            panic!("keying an admin client must be refused");
        };
        assert_eq!(status, StatusCode::FORBIDDEN);

        let (status, _) = handle_create_api_key(
            State(state),
            Extension(ctx),
            Json(CreateApiKeyClientRequest {
                client_id: "reader".into(),
            }),
        )
        .await
        .expect("keying a client within the caller's scopes still works");
        assert_eq!(status, StatusCode::CREATED);
    }

    fn client_owned_by(managed_by: &str) -> ApiClient {
        ApiClient {
            client_id: "c1".into(),
            name: "default".into(),
            scopes: vec!["admin".into()],
            is_active: true,
            created_at: Utc::now(),
            managed_by: managed_by.into(),
        }
    }

    #[test]
    fn a_store_error_blocks_the_delete_instead_of_reading_as_absent() {
        // Issue #504: the guard was `if let Ok(Some(client))`, so a store
        // failure took the same branch as "no such client" — skipping the
        // env-managed refusal and deleting the row with a 204.
        let err = deletion_guard(Err(StoreError::Database("lock timeout".into())))
            .expect_err("an unreadable store must not authorise a delete");
        assert_eq!(err.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn deleting_an_absent_client_stays_idempotent() {
        // The 204-on-absent behaviour predates the refusal and is deliberate;
        // fixing the error path must not turn it into a 404.
        assert!(deletion_guard(Ok(None)).is_ok());
    }

    #[test]
    fn an_env_owned_client_is_refused_and_an_api_owned_one_is_not() {
        let err = deletion_guard(Ok(Some(client_owned_by(MANAGED_BY_ENV))))
            .expect_err("the environment owns this row");
        assert_eq!(err.status(), StatusCode::CONFLICT);
        assert!(deletion_guard(Ok(Some(client_owned_by(MANAGED_BY_API)))).is_ok());
    }
}

//! Auth API endpoints: login, refresh, logout, API client/key management.

use std::sync::Arc;

use axum::{
    Extension, Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::Utc;
use croniq_auth::api_key::{generate_api_key, hash_api_key};
use croniq_auth::context::Scope;
use croniq_auth::crypto::unwrap_totp_secret;
use croniq_auth::jwt::{issue_mfa_token, issue_token_pair, validate_mfa_token};
use croniq_auth::password::verify_password;
use croniq_auth::totp::{hash_recovery_code, verify_code};
use croniq_auth::{AuthMethod, CallerContext, CallerType, default_scopes_for_role};
use croniq_store::models::{ApiClient, ApiKey, RefreshToken};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::ServerState;
use crate::api::audit;
use crate::api::auth_middleware::require_scope;

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
}

#[derive(Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
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

/// `POST /v1/auth/login` response — one variant or the other. Tagged
/// internally for client convenience: clients can pattern-match on
/// the presence of `requires_totp`.
#[derive(Serialize)]
#[serde(untagged)]
pub enum LoginResponse {
    Tokens(TokenResponse),
    MfaRequired(MfaRequiredResponse),
}

#[derive(Deserialize)]
pub struct TotpLoginRequest {
    pub mfa_token: String,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub recovery_code: Option<String>,
}

#[derive(Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

#[derive(Deserialize)]
pub struct LogoutRequest {
    pub refresh_token: String,
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

/// 403 envelope from `/v1/auth/login` when enforced 2FA is on but the account
/// has no confirmed TOTP secret. Such accounts must enrol *before*
/// enforcement is enabled; if everyone is locked out, relax the flag
/// (`auth { totp { required false } }` / `CRONIQ_REQUIRE_TOTP=false`), enrol,
/// then re-enable.
pub(super) fn totp_required_not_configured_response() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({
            "error": "totp_required_not_configured",
            "message": "two-factor authentication is required but not set up for this account; an administrator must enable it before you can sign in",
        })),
    )
        .into_response()
}

// ─── Handlers ────────────────────────────────────────────────────────────────

/// `POST /v1/auth/login`
pub async fn handle_login(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, Response> {
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

    let cred = store
        .get_credentials(&req.username)
        .map_err(|_| status_err(StatusCode::INTERNAL_SERVER_ERROR))?
        .ok_or_else(|| status_err(StatusCode::UNAUTHORIZED))?;

    // Check lockout
    if let Some(locked_until) = cred.locked_until
        && Utc::now() < locked_until
    {
        return Err(status_err(StatusCode::FORBIDDEN));
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
    //   * account has no secret but enforced 2FA is on → refuse (it can't
    //     satisfy the requirement; it must enrol before enforcement).
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
            return Ok(Json(LoginResponse::MfaRequired(MfaRequiredResponse {
                requires_totp: true,
                mfa_token,
                mfa_token_expires_in: expires_in,
            })));
        }
        verify_second_factor(
            jwt_config,
            store,
            &user.user_id,
            &secret_row,
            &req.code,
            &req.recovery_code,
        )?;
    } else if state.require_totp {
        audit::record_event(
            store,
            "user",
            Some(&user.user_id),
            "auth.login_totp_required_not_configured",
            "user",
            Some(&user.user_id),
        );
        return Err(totp_required_not_configured_response());
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
    Ok(Json(LoginResponse::Tokens(tokens)))
}

/// `POST /v1/auth/login/totp` — exchange the MFA step-up token + a
/// 6-digit TOTP code (or single-use recovery code) for normal tokens.
pub async fn handle_totp_login(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<TotpLoginRequest>,
) -> Result<Json<TokenResponse>, Response> {
    if !state.password_login_enabled {
        // TOTP login is the second step of the password flow; gate the same way.
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

    let user_id = validate_mfa_token(jwt_config, &req.mfa_token)
        .map_err(|_| status_err(StatusCode::UNAUTHORIZED))?;
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
    verify_second_factor(
        jwt_config,
        store,
        &user_id,
        &secret_row,
        &req.code,
        &req.recovery_code,
    )?;

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
    Ok(Json(tokens))
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
    let pair = issue_token_pair(
        jwt_config,
        &user.user_id,
        &user.user_id,
        CallerType::User,
        Some(&user.user_id),
        Some(user.role),
        AuthMethod::Password,
        &scopes,
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
        refresh_token: pair.refresh_token,
        token_type: "Bearer".into(),
        expires_in: jwt_config.access_ttl_secs,
    })
}

/// Verify a supplied second factor — exactly one of `code` (current 6-digit
/// TOTP) or `recovery_code` (single-use) — against the user's enabled secret.
/// Recovery codes are marked consumed here, *before* any token is minted, so
/// a parallel retry can't double-spend. Shared by the inline
/// `/v1/auth/login` path and the two-step `/v1/auth/login/totp` exchange.
fn verify_second_factor(
    jwt_config: &croniq_auth::jwt::JwtConfig,
    store: &crate::store::DynStore,
    user_id: &str,
    secret_row: &croniq_store::models::TotpSecret,
    code: &Option<String>,
    recovery_code: &Option<String>,
) -> Result<(), Response> {
    match (code, recovery_code) {
        (Some(code), None) => {
            let raw = unwrap_totp_secret(&jwt_config.secret, &secret_row.secret_enc)
                .map_err(|_| status_err(StatusCode::INTERNAL_SERVER_ERROR))?;
            let secret_b32 = String::from_utf8(raw)
                .map_err(|_| status_err(StatusCode::INTERNAL_SERVER_ERROR))?;
            match verify_code(&secret_b32, code) {
                Ok(true) => Ok(()),
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
pub async fn handle_refresh(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<RefreshRequest>,
) -> Result<Json<TokenResponse>, StatusCode> {
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let jwt_config = state
        .jwt_config
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let token_hash = hash_api_key(&req.refresh_token);
    let token = store
        .validate_refresh_token(&token_hash)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if Utc::now() > token.expires_at {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Revoke old token
    let _ = store.revoke_refresh_token(&token_hash, Utc::now());

    // Branch on caller type. User refresh re-loads the user row so role
    // changes propagate without forcing a re-login; API-key refresh
    // picks up scope changes on the owning client the same way.
    let (caller_type, user_id, role, auth_method, scopes) = if let Some(uid) = &token.user_id {
        let user = store
            .users_get_by_id(uid)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(StatusCode::UNAUTHORIZED)?;
        if !user.is_active {
            return Err(StatusCode::FORBIDDEN);
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
        let scopes = store
            .get_client(&token.client_id)
            .ok()
            .flatten()
            .map(|c| c.scopes)
            .unwrap_or_default();
        (CallerType::ApiKey, None, None, AuthMethod::ApiKey, scopes)
    };

    let caller_id = user_id.clone().unwrap_or_else(|| token.client_id.clone());
    let pair = issue_token_pair(
        jwt_config,
        &caller_id,
        &token.client_id,
        caller_type,
        user_id.as_deref(),
        role,
        auth_method,
        &scopes,
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let new_hash = hash_api_key(&pair.refresh_token);
    let _ = store.create_refresh_token(&RefreshToken {
        token_hash: new_hash,
        client_id: token.client_id,
        user_id: token.user_id,
        expires_at: pair.refresh_expires_at,
        revoked_at: None,
        created_at: Utc::now(),
    });

    Ok(Json(TokenResponse {
        access_token: pair.access_token,
        refresh_token: pair.refresh_token,
        token_type: "Bearer".into(),
        expires_in: jwt_config.access_ttl_secs,
    }))
}

/// `POST /v1/auth/logout`
pub async fn handle_logout(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<LogoutRequest>,
) -> StatusCode {
    let Some(store) = state.store.as_ref() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let token_hash = hash_api_key(&req.refresh_token);
    let _ = store.revoke_refresh_token(&token_hash, Utc::now());
    StatusCode::NO_CONTENT
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
) -> Result<Json<ApiClient>, StatusCode> {
    require_scope(&ctx, Scope::API_CLIENTS_ADMIN)?;
    if let Some(ref scopes) = req.scopes
        && scopes.is_empty()
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let mut client = store
        .get_client(&client_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    if let Some(name) = req.name {
        if name.trim().is_empty() {
            return Err(StatusCode::BAD_REQUEST);
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

/// `DELETE /v1/api-clients/{id}`
pub async fn handle_delete_client(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(client_id): axum::extract::Path<String>,
) -> StatusCode {
    if let Err(s) = require_scope(&ctx, Scope::API_CLIENTS_ADMIN) {
        return s;
    }
    let Some(store) = state.store.as_ref() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let _ = store.delete_client(&client_id);
    StatusCode::NO_CONTENT
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

    let pair = issue_token_pair(
        jwt_config,
        &client.client_id,
        &client.client_id,
        CallerType::ApiKey,
        None,
        None,
        AuthMethod::ApiKey,
        &client.scopes,
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(TokenResponse {
        access_token: pair.access_token,
        refresh_token: pair.refresh_token,
        token_type: "Bearer".into(),
        expires_in: jwt_config.access_ttl_secs,
    }))
}

/// `POST /v1/api-keys`
pub async fn handle_create_api_key(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Json(req): Json<CreateApiKeyClientRequest>,
) -> Result<(StatusCode, Json<CreateApiKeyResponse>), StatusCode> {
    require_scope(&ctx, Scope::API_KEYS_ADMIN)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    // Verify client exists
    store
        .get_client(&req.client_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

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

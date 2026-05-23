//! Auth API endpoints: login, refresh, logout, API client/key management.

use std::sync::Arc;

use axum::{Extension, Json, extract::State, http::StatusCode};
use chrono::Utc;
use croniq_auth::api_key::{generate_api_key, hash_api_key};
use croniq_auth::context::Scope;
use croniq_auth::jwt::issue_token_pair;
use croniq_auth::password::verify_password;
use croniq_auth::{AuthMethod, CallerContext, CallerType, default_scopes_for_role};
use croniq_store::models::{ApiClient, ApiKey, RefreshToken};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::ServerState;
use crate::api::auth_middleware::require_scope;

// ─── Request/Response types ──────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub expires_in: i64,
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

// ─── Handlers ────────────────────────────────────────────────────────────────

/// `POST /v1/auth/login`
pub async fn handle_login(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<TokenResponse>, StatusCode> {
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let jwt_config = state
        .jwt_config
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let cred = store
        .get_credentials(&req.username)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // Check lockout
    if let Some(locked_until) = cred.locked_until
        && Utc::now() < locked_until
    {
        return Err(StatusCode::FORBIDDEN);
    }

    // Verify password
    let valid = verify_password(&req.password, &cred.password_hash)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if !valid {
        // Increment failed attempts
        let mut updated = cred.clone();
        updated.failed_attempts += 1;
        if updated.failed_attempts >= 5 {
            updated.locked_until = Some(Utc::now() + chrono::Duration::minutes(15));
        }
        let _ = store.upsert_credentials(&updated);
        return Err(StatusCode::UNAUTHORIZED);
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
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if !user.is_active {
        return Err(StatusCode::FORBIDDEN);
    }

    // Issue tokens with role-derived scopes (no more hardcoded admin).
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

    // Stamp last-login so the UI can show "last seen N min ago" without
    // joining against login_events (added in PR-A5).
    let _ = store.users_set_last_login(&user.user_id, Utc::now());

    // Store refresh token
    let refresh_hash = hash_api_key(&pair.refresh_token);
    let _ = store.create_refresh_token(&RefreshToken {
        token_hash: refresh_hash,
        client_id: user.user_id.clone(),
        user_id: Some(user.user_id.clone()),
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

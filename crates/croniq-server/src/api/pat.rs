//! Personal Access Token management — `/v1/users/me/tokens`.
//!
//! Issuing a PAT mints a `croniq_pat_…` raw token, returns it ONCE,
//! and stores its SHA-256 hash. Subsequent API calls authenticate via
//! `Authorization: Bearer croniq_pat_…` (the auth middleware
//! recognises the prefix). PAT scopes are restricted to a subset of
//! the owning user's role's default scopes — a Viewer can't mint a
//! PAT with `jobs:write`.

use std::sync::Arc;

use axum::{Extension, Json, extract::State, http::StatusCode};
use chrono::Utc;
use croniq_auth::api_key::{generate_token, hash_token};
use croniq_auth::{CallerContext, CallerType, default_scopes_for_role};
use croniq_store::models::PersonalAccessToken;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::ServerState;

const PAT_PREFIX: &str = "croniq_pat";

// ─── DTOs ────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreatePatRequest {
    pub name: String,
    /// Optional explicit scope list. Default: full role-scope set. Any
    /// scope not in the role's default-set is rejected with 403.
    #[serde(default)]
    pub scopes: Option<Vec<String>>,
    /// Optional absolute expiry. None = never expires (revoke explicitly).
    #[serde(default)]
    pub expires_at: Option<chrono::DateTime<Utc>>,
}

#[derive(Serialize)]
pub struct CreatePatResponse {
    pub token_id: String,
    pub name: String,
    /// Raw `croniq_pat_…` token — shown ONCE.
    pub token: String,
    pub token_prefix: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<chrono::DateTime<Utc>>,
}

#[derive(Serialize)]
pub struct PatView {
    pub token_id: String,
    pub name: String,
    pub token_prefix: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<chrono::DateTime<Utc>>,
    pub revoked_at: Option<chrono::DateTime<Utc>>,
    pub last_used_at: Option<chrono::DateTime<Utc>>,
    pub created_at: chrono::DateTime<Utc>,
}

impl From<PersonalAccessToken> for PatView {
    fn from(p: PersonalAccessToken) -> Self {
        PatView {
            token_id: p.token_id,
            name: p.name,
            token_prefix: p.token_prefix,
            scopes: p.scopes,
            expires_at: p.expires_at,
            revoked_at: p.revoked_at,
            last_used_at: p.last_used_at,
            created_at: p.created_at,
        }
    }
}

// ─── Handlers ────────────────────────────────────────────────────────────────

/// `POST /v1/users/me/tokens`
pub async fn handle_create(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Json(req): Json<CreatePatRequest>,
) -> Result<(StatusCode, Json<CreatePatResponse>), StatusCode> {
    let user_id = require_user_caller(&ctx)?;
    if req.name.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let user = store
        .users_get_by_id(user_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let role_scopes = default_scopes_for_role(user.role);
    let granted = req.scopes.unwrap_or_else(|| role_scopes.clone());

    // Reject scopes outside the user's role. Admin role gets the
    // wildcard `admin` so any explicit list is by definition a subset.
    let is_admin = role_scopes.iter().any(|s| s == "admin");
    if !is_admin {
        for s in &granted {
            if !role_scopes.contains(s) {
                return Err(StatusCode::FORBIDDEN);
            }
        }
    }
    if granted.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let (raw_token, token_hash) = generate_token(PAT_PREFIX);
    let token_prefix: String = raw_token.chars().take(12).collect();
    let pat = PersonalAccessToken {
        token_id: Uuid::new_v4().to_string(),
        user_id: user_id.to_string(),
        name: req.name.clone(),
        token_hash,
        token_prefix: token_prefix.clone(),
        scopes: granted.clone(),
        expires_at: req.expires_at,
        revoked_at: None,
        last_used_at: None,
        created_at: Utc::now(),
    };
    store
        .pat_create(&pat)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok((
        StatusCode::CREATED,
        Json(CreatePatResponse {
            token_id: pat.token_id,
            name: req.name,
            token: raw_token,
            token_prefix,
            scopes: granted,
            expires_at: pat.expires_at,
        }),
    ))
}

/// `GET /v1/users/me/tokens`
pub async fn handle_list(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
) -> Result<Json<Vec<PatView>>, StatusCode> {
    let user_id = require_user_caller(&ctx)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let items = store
        .pat_list(user_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(items.into_iter().map(PatView::from).collect()))
}

/// `DELETE /v1/users/me/tokens/{token_id}`
pub async fn handle_revoke(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(token_id): axum::extract::Path<String>,
) -> StatusCode {
    let Ok(user_id) = require_user_caller(&ctx) else {
        return StatusCode::UNAUTHORIZED;
    };
    let Some(store) = state.store.as_ref() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    // Find by token_id via list (PATs are small per user; index by
    // token_id on the row is fine even without a dedicated lookup).
    let owns = store
        .pat_list(user_id)
        .ok()
        .into_iter()
        .flatten()
        .any(|p| p.token_id == token_id);
    if !owns {
        return StatusCode::NOT_FOUND;
    }
    let _ = store.pat_revoke(&token_id, Utc::now());
    StatusCode::NO_CONTENT
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn require_user_caller(ctx: &CallerContext) -> Result<&str, StatusCode> {
    if ctx.caller_type != CallerType::User {
        return Err(StatusCode::FORBIDDEN);
    }
    ctx.user_id.as_deref().ok_or(StatusCode::UNAUTHORIZED)
}

// Compile-time use of `hash_token` to avoid an "unused" warning during
// edits — the symbol is exported by the auth crate but consumed via
// `generate_token` here. (Suppress is gentler than a `let _ =`.)
#[allow(dead_code)]
const _: fn(&str) -> String = hash_token;

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
use crate::api::audit;

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

    audit::record(store, &ctx, "pat.issued", "pat", Some(&pat.token_id), None);
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
    // Hide revoked tokens. The settings UI has no status column, so a
    // revoked PAT left in the list looks identical to a live one and a
    // "revoke" appears to do nothing. Auth already rejects revoked PATs
    // (see auth_middleware) and the audit log keeps the revocation record,
    // so dropping them here costs no real history.
    Ok(Json(
        items
            .into_iter()
            .filter(|p| p.revoked_at.is_none())
            .map(PatView::from)
            .collect(),
    ))
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
    audit::record(store, &ctx, "pat.revoked", "pat", Some(&token_id), None);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{DynStore, sqlite_store};
    use croniq_auth::AuthMethod;
    use croniq_runner::AppState;
    use croniq_store::models::{Role, User};
    use croniq_store::sqlite::SqliteStore;
    use tokio::sync::mpsc;

    /// PATs carry a FK to `users`, so the owning row must exist first.
    fn seed_user(store: &DynStore, user_id: &str) {
        store
            .users_create(&User {
                user_id: user_id.into(),
                username: user_id.into(),
                email: None,
                display_name: None,
                role: Role::Admin,
                is_active: true,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                last_login_at: None,
            })
            .unwrap();
    }

    fn make_store() -> DynStore {
        sqlite_store(SqliteStore::in_memory().unwrap())
    }

    fn user_ctx(user_id: &str) -> CallerContext {
        CallerContext {
            caller_type: CallerType::User,
            caller_id: user_id.into(),
            client_id: user_id.into(),
            user_id: Some(user_id.into()),
            role: None,
            auth_method: AuthMethod::Password,
            scopes: vec!["admin".into()],
        }
    }

    fn pat(user_id: &str, token_id: &str, name: &str) -> PersonalAccessToken {
        PersonalAccessToken {
            token_id: token_id.into(),
            user_id: user_id.into(),
            name: name.into(),
            token_hash: format!("hash-{token_id}"),
            token_prefix: "croniq_pat_x".into(),
            scopes: vec!["admin".into()],
            expires_at: None,
            revoked_at: None,
            last_used_at: None,
            created_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn list_omits_revoked_tokens() {
        let store = make_store();
        seed_user(&store, "user-1");
        store.pat_create(&pat("user-1", "live", "active")).unwrap();
        store.pat_create(&pat("user-1", "dead", "revoked")).unwrap();
        store.pat_revoke("dead", Utc::now()).unwrap();

        let (tx, _rx) = mpsc::unbounded_channel();
        let state = ServerState::with_auth(AppState::new(), tx, None, Some(Arc::clone(&store)));

        let Json(list) = handle_list(State(state), Extension(user_ctx("user-1")))
            .await
            .expect("list should succeed");

        assert_eq!(list.len(), 1, "the revoked token must be hidden");
        assert_eq!(list[0].name, "active");
    }
}

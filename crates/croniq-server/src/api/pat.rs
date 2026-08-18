//! Personal Access Token management — `/v1/users/me/tokens`.
//!
//! Issuing a PAT mints a `croniq_pat_…` raw token, returns it ONCE,
//! and stores its SHA-256 hash. Subsequent API calls authenticate via
//! `Authorization: Bearer croniq_pat_…` (the auth middleware
//! recognises the prefix). PAT scopes are restricted to a subset of
//! the owning user's role's default scopes — a Viewer can't mint a
//! PAT with `jobs:write` — *and* to a subset of the scopes carried by
//! the credential used to make the request, so a narrow token cannot
//! re-mint itself wider. A PAT may not mint further PATs at all.

use std::sync::Arc;

use axum::{Extension, Json, extract::State, http::StatusCode};
use chrono::Utc;
use croniq_auth::api_key::{generate_token, hash_token};
use croniq_auth::{AuthMethod, CallerContext, CallerType, default_scopes_for_role};
use croniq_store::models::PersonalAccessToken;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::ServerState;
use crate::api::audit;
use crate::api::auth_middleware::require_grantable_scopes;

const PAT_PREFIX: &str = "croniq_pat";

// ─── DTOs ────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreatePatRequest {
    pub name: String,
    /// Optional explicit scope list. Default: the role-scope set, narrowed
    /// to what the calling credential itself holds. Any scope outside the
    /// role's default-set, or outside the caller's own scopes, is rejected
    /// with 403.
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
    // A PAT never mints another PAT. Even bounded by the caller's own
    // scopes, chaining would let a leaked token spawn a sibling that
    // survives revoking the one it came from.
    if ctx.auth_method == AuthMethod::Pat {
        return Err(StatusCode::FORBIDDEN);
    }
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
    // The implicit default is the role's set narrowed to what this
    // credential actually carries, so omitting `scopes` can't widen
    // either — a session token holds the full role set, so the common
    // case is unchanged.
    let granted = req
        .scopes
        .unwrap_or_else(|| ctx.grantable_subset(&role_scopes));

    // Reject scopes outside the user's role. Admin role gets the
    // wildcard `admin` so any explicit list is by definition a subset.
    let is_admin_role = role_scopes.iter().any(|s| s == "admin");
    if !is_admin_role {
        for s in &granted {
            if !role_scopes.contains(s) {
                return Err(StatusCode::FORBIDDEN);
            }
        }
    }
    // …and reject anything the presented credential does not itself
    // hold. The role check above is about what the *user* may have; this
    // one is about what the *token in hand* may pass on.
    require_grantable_scopes(&ctx, &granted)?;
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
        seed_user_with_role(store, user_id, Role::Admin);
    }

    fn seed_user_with_role(store: &DynStore, user_id: &str, role: Role) {
        store
            .users_create(&User {
                user_id: user_id.into(),
                username: user_id.into(),
                email: None,
                display_name: None,
                role,
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

    /// A caller context with an explicit scope set and auth method — the
    /// two inputs the issuance bound is built on.
    fn ctx_with(user_id: &str, scopes: &[&str], auth_method: AuthMethod) -> CallerContext {
        CallerContext {
            auth_method,
            scopes: scopes.iter().map(|s| s.to_string()).collect(),
            ..user_ctx(user_id)
        }
    }

    fn create_req(name: &str, scopes: Option<Vec<&str>>) -> CreatePatRequest {
        CreatePatRequest {
            name: name.into(),
            scopes: scopes.map(|v| v.into_iter().map(String::from).collect()),
            expires_at: None,
        }
    }

    fn state_with(store: &DynStore) -> Arc<ServerState> {
        let (tx, _rx) = mpsc::unbounded_channel();
        ServerState::with_auth(AppState::new(), tx, None, Some(Arc::clone(store)))
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

    #[tokio::test]
    async fn admin_session_may_still_mint_an_admin_pat() {
        let store = make_store();
        seed_user(&store, "user-1");
        let ctx = ctx_with("user-1", &["admin"], AuthMethod::Password);

        let (status, Json(body)) = handle_create(
            State(state_with(&store)),
            Extension(ctx),
            Json(create_req("ci", Some(vec!["admin"]))),
        )
        .await
        .expect("an admin caller may grant anything");

        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body.scopes, vec!["admin"]);
    }

    #[tokio::test]
    async fn narrow_caller_cannot_mint_a_pat_it_does_not_hold() {
        let store = make_store();
        // Admin *user* — the role would allow `admin`, but the credential
        // in hand only carries `jobs:read`.
        seed_user(&store, "user-1");
        let state = state_with(&store);
        let ctx = ctx_with("user-1", &["jobs:read"], AuthMethod::Password);

        let Err(status) = handle_create(
            State(Arc::clone(&state)),
            Extension(ctx.clone()),
            Json(create_req("escalate", Some(vec!["admin"]))),
        )
        .await
        else {
            panic!("must not mint the admin wildcard");
        };
        assert_eq!(status, StatusCode::FORBIDDEN);

        // Not just `admin` — any scope the caller does not itself hold.
        let Err(status) = handle_create(
            State(Arc::clone(&state)),
            Extension(ctx.clone()),
            Json(create_req(
                "escalate",
                Some(vec!["jobs:read", "jobs:write"]),
            )),
        )
        .await
        else {
            panic!("must not mint an unheld scope");
        };
        assert_eq!(status, StatusCode::FORBIDDEN);

        // The scopes it does hold still work.
        let (status, Json(body)) = handle_create(
            State(state),
            Extension(ctx),
            Json(create_req("readonly", Some(vec!["jobs:read"]))),
        )
        .await
        .expect("granting a held scope is fine");
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body.scopes, vec!["jobs:read"]);
    }

    #[tokio::test]
    async fn omitted_scopes_default_to_what_the_caller_holds() {
        let store = make_store();
        seed_user_with_role(&store, "op-1", Role::Operator);
        let ctx = ctx_with("op-1", &["jobs:read"], AuthMethod::Password);

        let (_, Json(body)) = handle_create(
            State(state_with(&store)),
            Extension(ctx),
            Json(create_req("default", None)),
        )
        .await
        .expect("the default set narrows instead of widening");

        assert_eq!(
            body.scopes,
            vec!["jobs:read"],
            "the role default must not leak past the presented credential"
        );
    }

    #[tokio::test]
    async fn a_pat_cannot_mint_another_pat() {
        let store = make_store();
        seed_user(&store, "user-1");
        let ctx = ctx_with("user-1", &["admin"], AuthMethod::Pat);

        let Err(status) = handle_create(
            State(state_with(&store)),
            Extension(ctx),
            Json(create_req("chained", Some(vec!["admin"]))),
        )
        .await
        else {
            panic!("PAT chaining is refused outright");
        };
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
}

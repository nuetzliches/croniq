//! User management endpoints.
//!
//! Layout:
//!   GET    /v1/users               admin — list all
//!   POST   /v1/users               admin — direct create with password
//!   GET    /v1/users/{id}          admin — load one
//!   PATCH  /v1/users/{id}          admin — partial update
//!   DELETE /v1/users/{id}          admin — delete user
//!   GET    /v1/users/me            self  — load own
//!   PATCH  /v1/users/me            self  — change own display_name / email
//!   POST   /v1/users/me/change-password  self — verify old, hash new
//!
//! Last-active-admin guard: PATCH role-demotion, PATCH is_active=false,
//! and DELETE all reject with 409 when the operation would leave zero
//! active admins. Implemented via `users_count_active_admins()` +
//! checking whether the target row is itself an active admin.

use std::sync::Arc;

use axum::{Extension, Json, extract::State, http::StatusCode};
use chrono::Utc;
use croniq_auth::context::Scope;
use croniq_auth::password::{hash_password, validate_password, verify_password};
use croniq_auth::{CallerContext, CallerType};
use croniq_store::models::{PasswordCredential, Role, User};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::ServerState;
use crate::api::audit;
use crate::api::auth_middleware::require_scope;

// ─── DTOs ────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct UserView {
    pub user_id: String,
    pub username: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub role: Role,
    pub is_active: bool,
    pub created_at: chrono::DateTime<Utc>,
    pub updated_at: chrono::DateTime<Utc>,
    pub last_login_at: Option<chrono::DateTime<Utc>>,
    /// Whether the user has a confirmed TOTP secret. Only populated by
    /// `GET /v1/users/me` (where the self-caller is allowed to see it);
    /// omitted from admin list/get responses to avoid an N+1 lookup and
    /// to keep those payloads unchanged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub totp_enabled: Option<bool>,
}

impl From<User> for UserView {
    fn from(u: User) -> Self {
        UserView {
            user_id: u.user_id,
            username: u.username,
            email: u.email,
            display_name: u.display_name,
            role: u.role,
            is_active: u.is_active,
            created_at: u.created_at,
            updated_at: u.updated_at,
            last_login_at: u.last_login_at,
            totp_enabled: None,
        }
    }
}

#[derive(Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    pub role: Role,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateUserRequest {
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub role: Option<Role>,
    #[serde(default)]
    pub is_active: Option<bool>,
}

#[derive(Deserialize)]
pub struct UpdateMeRequest {
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
}

#[derive(Deserialize)]
pub struct ChangePasswordRequest {
    pub old_password: String,
    pub new_password: String,
}

// ─── Admin-scoped handlers ───────────────────────────────────────────────────

/// `GET /v1/users`
pub async fn handle_list(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
) -> Result<Json<Vec<UserView>>, StatusCode> {
    require_user_admin(&ctx)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let users = store
        .users_list()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(users.into_iter().map(UserView::from).collect()))
}

/// `POST /v1/users`
pub async fn handle_create(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Json(req): Json<CreateUserRequest>,
) -> Result<(StatusCode, Json<UserView>), StatusCode> {
    require_user_admin(&ctx)?;
    if req.username.trim().is_empty() || validate_password(&req.password).is_err() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    if store
        .users_get_by_username(&req.username)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .is_some()
    {
        return Err(StatusCode::CONFLICT);
    }

    let now = Utc::now();
    let user_id = Uuid::new_v4().to_string();
    let user = User {
        user_id: user_id.clone(),
        username: req.username.clone(),
        email: req.email,
        display_name: req.display_name,
        role: req.role,
        is_active: true,
        created_at: now,
        updated_at: now,
        last_login_at: None,
    };
    store
        .users_create(&user)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let pw_hash = hash_password(&req.password).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    store
        .upsert_credentials(&PasswordCredential {
            user_id,
            username: req.username,
            password_hash: pw_hash,
            failed_attempts: 0,
            locked_until: None,
            created_at: now,
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    audit::record(
        store,
        &ctx,
        "user.created",
        "user",
        Some(&user.user_id),
        None,
    );
    Ok((StatusCode::CREATED, Json(UserView::from(user))))
}

/// `GET /v1/users/{id}`
pub async fn handle_get(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(user_id): axum::extract::Path<String>,
) -> Result<Json<UserView>, StatusCode> {
    require_user_admin(&ctx)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let user = store
        .users_get_by_id(&user_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(UserView::from(user)))
}

/// `PATCH /v1/users/{id}` — partial update with last-admin protection.
pub async fn handle_update(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(user_id): axum::extract::Path<String>,
    Json(req): Json<UpdateUserRequest>,
) -> Result<Json<UserView>, StatusCode> {
    require_user_admin(&ctx)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let mut user = store
        .users_get_by_id(&user_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let demotes_admin = req
        .role
        .map(|r| user.role == Role::Admin && r != Role::Admin)
        .unwrap_or(false);
    let deactivates = req.is_active == Some(false) && user.is_active;
    if (demotes_admin || (deactivates && user.role == Role::Admin))
        && would_leave_no_admins(store, &user)?
    {
        return Err(StatusCode::CONFLICT);
    }

    if let Some(email) = req.email {
        user.email = Some(email);
    }
    if let Some(display_name) = req.display_name {
        user.display_name = Some(display_name);
    }
    if let Some(role) = req.role {
        user.role = role;
    }
    if let Some(active) = req.is_active {
        user.is_active = active;
    }
    user.updated_at = Utc::now();

    store
        .users_update(&user)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    // Deactivating an account must end its sessions now, not when the last
    // access token happens to expire (issue #431). Role and profile edits
    // deliberately do not bump: signing someone out is a real cost, and a role
    // change already propagates on the next refresh.
    if deactivates {
        store
            .users_bump_token_generation(&user.user_id)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    audit::record(
        store,
        &ctx,
        "user.updated",
        "user",
        Some(&user.user_id),
        None,
    );
    Ok(Json(UserView::from(user)))
}

/// `DELETE /v1/users/{id}` — refuses to delete the last admin.
pub async fn handle_delete(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(user_id): axum::extract::Path<String>,
) -> StatusCode {
    if let Err(s) = require_user_admin(&ctx) {
        return s;
    }
    let Some(store) = state.store.as_ref() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let Some(user) = store.users_get_by_id(&user_id).ok().flatten() else {
        return StatusCode::NOT_FOUND;
    };
    if user.role == Role::Admin && user.is_active {
        match would_leave_no_admins(store, &user) {
            Ok(true) => return StatusCode::CONFLICT,
            Ok(false) => {}
            Err(s) => return s,
        }
    }
    let _ = store.users_delete(&user_id);
    audit::record(store, &ctx, "user.deleted", "user", Some(&user_id), None);
    StatusCode::NO_CONTENT
}

// ─── Self-scoped handlers ────────────────────────────────────────────────────

/// `GET /v1/users/me`
pub async fn handle_get_me(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
) -> Result<Json<UserView>, StatusCode> {
    let user_id = require_self_user(&ctx)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let user = store
        .users_get_by_id(user_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let totp_enabled = store
        .totp_get(user_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map(|t| t.enabled)
        .unwrap_or(false);
    let mut view = UserView::from(user);
    view.totp_enabled = Some(totp_enabled);
    Ok(Json(view))
}

/// `PATCH /v1/users/me` — display_name / email only. Role + is_active
/// require `users:admin`.
pub async fn handle_update_me(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Json(req): Json<UpdateMeRequest>,
) -> Result<Json<UserView>, StatusCode> {
    let user_id = require_self_user(&ctx)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let mut user = store
        .users_get_by_id(user_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    if let Some(email) = req.email {
        user.email = Some(email);
    }
    if let Some(display_name) = req.display_name {
        user.display_name = Some(display_name);
    }
    user.updated_at = Utc::now();

    store
        .users_update(&user)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(UserView::from(user)))
}

/// `POST /v1/users/me/change-password` — verify old, hash new, replace.
pub async fn handle_change_password(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Json(req): Json<ChangePasswordRequest>,
) -> StatusCode {
    let Ok(user_id) = require_self_user(&ctx) else {
        return StatusCode::UNAUTHORIZED;
    };
    if validate_password(&req.new_password).is_err() {
        return StatusCode::BAD_REQUEST;
    }
    let Some(store) = state.store.as_ref() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let Some(user) = store.users_get_by_id(user_id).ok().flatten() else {
        return StatusCode::NOT_FOUND;
    };
    let Some(cred) = store.get_credentials(&user.username).ok().flatten() else {
        return StatusCode::NOT_FOUND;
    };

    match verify_password(&req.old_password, &cred.password_hash) {
        Ok(true) => {}
        _ => return StatusCode::UNAUTHORIZED,
    }

    let pw_hash = match hash_password(&req.new_password) {
        Ok(h) => h,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR,
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
    // Invalidate access tokens minted under the old password (issue #431).
    // Refresh was already blocked by the is_active re-check, but access tokens
    // stayed valid until exp — up to an hour after the change.
    if store.users_bump_token_generation(user_id).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    audit::record(
        store,
        &ctx,
        "user.password_changed",
        "user",
        Some(user_id),
        None,
    );
    StatusCode::NO_CONTENT
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Require either the `admin` wildcard or `users:admin` scope.
fn require_user_admin(ctx: &CallerContext) -> Result<(), StatusCode> {
    if ctx.has_any_scope(&[Scope::ADMIN, Scope::USERS_ADMIN]) {
        Ok(())
    } else {
        require_scope(ctx, Scope::USERS_ADMIN)
    }
}

/// Require that the caller is a user (not an API key). Returns the
/// user_id; this is the same value as `caller_id` for users but the
/// explicit accessor makes intent clear.
fn require_self_user(ctx: &CallerContext) -> Result<&str, StatusCode> {
    if ctx.caller_type != CallerType::User {
        return Err(StatusCode::FORBIDDEN);
    }
    ctx.user_id.as_deref().ok_or(StatusCode::UNAUTHORIZED)
}

/// Check whether mutating `user` (demote/deactivate/delete) would leave
/// zero active admins. The user is counted as "currently an active
/// admin" if their *stored* role is Admin and is_active, so this is
/// safe to call before applying the mutation.
fn would_leave_no_admins(store: &crate::store::DynStore, user: &User) -> Result<bool, StatusCode> {
    let active_admins = store
        .users_count_active_admins()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    // If they aren't currently an active admin, removing/demoting them
    // doesn't affect the admin count at all.
    if user.role != Role::Admin || !user.is_active {
        return Ok(false);
    }
    Ok(active_admins <= 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_user() -> User {
        let now = Utc::now();
        User {
            user_id: "u1".into(),
            username: "admin".into(),
            email: None,
            display_name: None,
            role: Role::Admin,
            is_active: true,
            created_at: now,
            updated_at: now,
            last_login_at: None,
        }
    }

    // Admin list/get responses go through `From<User>`, which leaves
    // `totp_enabled` as `None`. The field must then be absent from the
    // payload so those responses stay byte-for-byte unchanged.
    #[test]
    fn user_view_omits_totp_enabled_when_none() {
        let view = UserView::from(sample_user());
        let v = serde_json::to_value(&view).unwrap();
        assert!(
            v.get("totp_enabled").is_none(),
            "totp_enabled must be omitted unless explicitly set (got {v})"
        );
    }

    // `GET /v1/users/me` sets the flag explicitly; it must then serialize
    // as a real boolean the UI can branch on.
    #[test]
    fn user_view_includes_totp_enabled_when_set() {
        let mut view = UserView::from(sample_user());
        view.totp_enabled = Some(true);
        let v = serde_json::to_value(&view).unwrap();
        assert_eq!(v["totp_enabled"], serde_json::json!(true));

        view.totp_enabled = Some(false);
        let v = serde_json::to_value(&view).unwrap();
        assert_eq!(v["totp_enabled"], serde_json::json!(false));
    }
}

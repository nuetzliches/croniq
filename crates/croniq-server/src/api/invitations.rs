//! Invitation endpoints.
//!
//! Admin issues a new user invite carrying a single-use token. The raw
//! token is returned ONCE in the create response (and the invite URL
//! built from `state.app_base_url`); only its SHA-256 hash is stored.
//! Acceptance is public — the redeemer presents the raw token and a
//! password.
//!
//! Routes:
//!   POST   /v1/invitations                       admin — create
//!   GET    /v1/invitations                       admin — list
//!   DELETE /v1/invitations/{id}                  admin — revoke
//!   POST   /v1/invitations/accept                public — redeem (body: {token, password})

use std::sync::Arc;
use std::time::Duration;

use axum::{Extension, Json, extract::State, http::StatusCode};
use chrono::Utc;
use croniq_auth::api_key::{generate_token, hash_token};
use croniq_auth::context::Scope;
use croniq_auth::password::hash_password;
use croniq_auth::{CallerContext, CallerType};
use croniq_store::models::{Invitation, PasswordCredential, Role, User};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::ServerState;
use crate::api::audit;
use crate::api::auth_middleware::require_scope;

const INVITE_TOKEN_TTL: Duration = Duration::from_secs(7 * 24 * 3600); // 7 days
const INVITE_PREFIX: &str = "croniq_inv";

// ─── DTOs ────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateInvitationRequest {
    pub email: String,
    pub role: Role,
}

#[derive(Serialize)]
pub struct CreateInvitationResponse {
    pub invitation_id: String,
    pub email: String,
    pub role: Role,
    pub expires_at: chrono::DateTime<Utc>,
    /// Raw token — shown ONCE in the create response. The admin
    /// delivers this URL to the invitee out-of-band (email if SMTP is
    /// configured, otherwise copy/paste).
    pub token: String,
    /// Pre-built acceptance URL: `{app_base_url}/invitations/accept?token=...`.
    pub accept_url: String,
}

#[derive(Serialize)]
pub struct InvitationView {
    pub invitation_id: String,
    pub email: String,
    pub role: Role,
    pub invited_by: String,
    pub expires_at: chrono::DateTime<Utc>,
    pub accepted_at: Option<chrono::DateTime<Utc>>,
    pub revoked_at: Option<chrono::DateTime<Utc>>,
    pub created_at: chrono::DateTime<Utc>,
}

impl From<Invitation> for InvitationView {
    fn from(i: Invitation) -> Self {
        InvitationView {
            invitation_id: i.invitation_id,
            email: i.email,
            role: i.role,
            invited_by: i.invited_by,
            expires_at: i.expires_at,
            accepted_at: i.accepted_at,
            revoked_at: i.revoked_at,
            created_at: i.created_at,
        }
    }
}

#[derive(Deserialize)]
pub struct AcceptInvitationRequest {
    pub token: String,
    pub username: String,
    pub password: String,
}

// ─── Admin endpoints ─────────────────────────────────────────────────────────

/// `POST /v1/invitations`
pub async fn handle_create(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Json(req): Json<CreateInvitationRequest>,
) -> Result<(StatusCode, Json<CreateInvitationResponse>), StatusCode> {
    require_user_admin(&ctx)?;
    if req.email.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let invited_by = ctx.user_id.clone().ok_or(StatusCode::FORBIDDEN)?; // API keys can't invite — humans only
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let (raw_token, token_hash) = generate_token(INVITE_PREFIX);
    let now = Utc::now();
    let expires_at = now + chrono::Duration::from_std(INVITE_TOKEN_TTL).unwrap();
    let invitation_id = Uuid::new_v4().to_string();

    let invite = Invitation {
        invitation_id: invitation_id.clone(),
        email: req.email.clone(),
        role: req.role,
        token_hash,
        invited_by,
        expires_at,
        accepted_at: None,
        revoked_at: None,
        created_at: now,
    };
    store
        .invitations_create(&invite)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let accept_url = format!(
        "{}/invitations/accept?token={}",
        state.app_base_url.trim_end_matches('/'),
        raw_token
    );

    // Best-effort email delivery. NoopSender is a no-op + audit log.
    let _ = state.email_sender.send(
        &req.email,
        "You're invited to Croniq",
        &format!(
            "You've been invited to join Croniq. To accept, visit:\n\n{}\n\nThis link expires in 7 days.",
            accept_url
        ),
    );

    audit::record(
        store,
        &ctx,
        "invitation.issued",
        "invitation",
        Some(&invitation_id),
        None,
    );
    Ok((
        StatusCode::CREATED,
        Json(CreateInvitationResponse {
            invitation_id,
            email: req.email,
            role: req.role,
            expires_at,
            token: raw_token,
            accept_url,
        }),
    ))
}

/// `GET /v1/invitations`
pub async fn handle_list(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
) -> Result<Json<Vec<InvitationView>>, StatusCode> {
    require_user_admin(&ctx)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let items = store
        .invitations_list()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(items.into_iter().map(InvitationView::from).collect()))
}

/// `DELETE /v1/invitations/{id}` — revoke (cannot un-revoke).
pub async fn handle_revoke(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    axum::extract::Path(invitation_id): axum::extract::Path<String>,
) -> StatusCode {
    if let Err(s) = require_user_admin(&ctx) {
        return s;
    }
    let Some(store) = state.store.as_ref() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let Some(invite) = store.invitations_get(&invitation_id).ok().flatten() else {
        return StatusCode::NOT_FOUND;
    };
    if invite.accepted_at.is_some() {
        return StatusCode::CONFLICT; // already accepted, nothing to revoke
    }
    let _ = store.invitations_revoke(&invitation_id, Utc::now());
    audit::record(
        store,
        &ctx,
        "invitation.revoked",
        "invitation",
        Some(&invitation_id),
        None,
    );
    StatusCode::NO_CONTENT
}

// ─── Public endpoint ─────────────────────────────────────────────────────────

/// `POST /v1/invitations/accept` — public, redeems the token.
///
/// Creates the user (role from invitation), sets password, marks the
/// invitation as accepted. Returns 410 Gone if expired/revoked.
pub async fn handle_accept(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<AcceptInvitationRequest>,
) -> StatusCode {
    if req.username.trim().is_empty() || req.password.len() < 8 {
        return StatusCode::BAD_REQUEST;
    }
    let Some(store) = state.store.as_ref() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };

    let token_hash = hash_token(&req.token);
    let Some(invite) = store
        .invitations_get_by_token_hash(&token_hash)
        .ok()
        .flatten()
    else {
        return StatusCode::UNAUTHORIZED;
    };

    if invite.revoked_at.is_some() || invite.accepted_at.is_some() {
        return StatusCode::GONE;
    }
    if Utc::now() > invite.expires_at {
        return StatusCode::GONE;
    }

    // Reject username collision early — token consumption is otherwise
    // wasted on a guaranteed failure.
    if store
        .users_get_by_username(&req.username)
        .ok()
        .flatten()
        .is_some()
    {
        return StatusCode::CONFLICT;
    }

    let now = Utc::now();
    let user_id = Uuid::new_v4().to_string();
    let user = User {
        user_id: user_id.clone(),
        username: req.username.clone(),
        email: Some(invite.email.clone()),
        display_name: None,
        role: invite.role,
        is_active: true,
        created_at: now,
        updated_at: now,
        last_login_at: None,
    };
    if store.users_create(&user).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR;
    }

    let pw_hash = match hash_password(&req.password) {
        Ok(h) => h,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR,
    };
    let _ = store.upsert_credentials(&PasswordCredential {
        user_id,
        username: req.username,
        password_hash: pw_hash,
        failed_attempts: 0,
        locked_until: None,
        created_at: now,
    });

    let _ = store.invitations_mark_accepted(&invite.invitation_id, now);

    // Public endpoint — no CallerContext. Actor is the freshly-created
    // user; target is the invitation that just got consumed.
    audit::record_event(
        store,
        "user",
        Some(&user.user_id),
        "invitation.accepted",
        "invitation",
        Some(&invite.invitation_id),
    );
    StatusCode::CREATED
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn require_user_admin(ctx: &CallerContext) -> Result<(), StatusCode> {
    if ctx.has_any_scope(&[Scope::ADMIN, Scope::USERS_ADMIN]) {
        Ok(())
    } else {
        require_scope(ctx, Scope::USERS_ADMIN)
    }
}

// Force compile-time check that CallerType is used (avoid lint when the
// helper logic above doesn't reference it directly).
#[allow(dead_code)]
const _: Option<CallerType> = None;

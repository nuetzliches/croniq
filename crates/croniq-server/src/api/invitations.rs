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

use axum::{
    Extension, Json,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use chrono::Utc;
use croniq_auth::api_key::{generate_token, hash_token};
use croniq_auth::context::Scope;
use croniq_auth::password::{hash_password, validate_password};
use croniq_auth::{CallerContext, CallerType};
use croniq_store::models::{Invitation, PasswordCredential, Role, User};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::ServerState;
use crate::api::audit;
use crate::api::auth_middleware::require_scope;

const INVITE_TOKEN_TTL: Duration = Duration::from_secs(7 * 24 * 3600); // 7 days

/// Ceiling on a caller-chosen invitation lifetime.
///
/// An invitation token is a bearer credential that creates an account, so an
/// unbounded lifetime is a link that stays live in someone's inbox forever.
/// Thirty days is generous against the seven-day default and still an interval
/// an operator can reason about. A request above it is refused rather than
/// silently clamped — clamping would report success for a deadline the server
/// did not honour, which is the shape of the bug this whole change is about.
const INVITE_TOKEN_MAX_TTL_HOURS: u32 = 30 * 24;
const INVITE_PREFIX: &str = "croniq_inv";

// ─── DTOs ────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateInvitationRequest {
    pub email: String,
    pub role: Role,
    /// How long the invitation stays valid, in hours. Omitted means the
    /// server default of seven days; the ceiling is
    /// [`INVITE_TOKEN_MAX_TTL_HOURS`].
    ///
    /// The dashboard has always sent this and the server has never read it, so
    /// the "Valid for (hours)" box did nothing and every invitation expired
    /// after exactly seven days regardless (issue #658).
    #[serde(default)]
    pub expires_in_hours: Option<u32>,
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
    headers: HeaderMap,
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
    let expires_at = match req.expires_in_hours {
        None => now + chrono::Duration::from_std(INVITE_TOKEN_TTL).unwrap(),
        Some(hours) if hours == 0 || hours > INVITE_TOKEN_MAX_TTL_HOURS => {
            return Err(StatusCode::BAD_REQUEST);
        }
        Some(hours) => now + chrono::Duration::hours(i64::from(hours)),
    };
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

    // Admin-authenticated request → the Host header is the admin's own and
    // safe to derive from when CRONIQ_APP_URL is unset.
    let base = crate::api::resolve_link_base(&state.app_base_url, &headers, true);
    let accept_url = format!("{base}/invitations/accept?token={raw_token}");

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
    if req.username.trim().is_empty() || validate_password(&req.password).is_err() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{DynStore, sqlite_store};
    use croniq_runner::AppState;
    use croniq_store::sqlite::SqliteStore;
    use tokio::sync::mpsc;

    fn make_store() -> DynStore {
        let store = sqlite_store(SqliteStore::in_memory().unwrap());
        store
            .users_create(&User {
                user_id: "admin-1".into(),
                username: "admin".into(),
                email: Some("admin@example.com".into()),
                display_name: None,
                role: Role::Admin,
                is_active: true,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                last_login_at: None,
            })
            .unwrap();
        store
    }

    fn admin_ctx() -> CallerContext {
        CallerContext {
            caller_type: CallerType::User,
            caller_id: "admin-1".into(),
            client_id: "admin-1".into(),
            user_id: Some("admin-1".into()),
            role: Some(Role::Admin),
            auth_method: croniq_auth::AuthMethod::Password,
            scopes: vec!["admin".into()],
            token_generation: None,
        }
    }

    fn state_with(store: &DynStore) -> Arc<ServerState> {
        let (tx, _rx) = mpsc::unbounded_channel();
        ServerState::with_auth(AppState::new(), tx, None, Some(Arc::clone(store)))
    }

    async fn create(
        store: &DynStore,
        hours: Option<u32>,
    ) -> Result<(StatusCode, Json<CreateInvitationResponse>), StatusCode> {
        handle_create(
            State(state_with(store)),
            Extension(admin_ctx()),
            HeaderMap::new(),
            Json(CreateInvitationRequest {
                email: format!("invitee-{:?}@example.com", hours),
                role: Role::Viewer,
                expires_in_hours: hours,
            }),
        )
        .await
    }

    /// The dashboard has always sent `expires_in_hours` and the server has
    /// never read it, so the "Valid for (hours)" box did nothing and every
    /// invitation expired after exactly seven days (issue #658).
    #[tokio::test]
    async fn expires_in_hours_sets_the_deadline() {
        let store = make_store();
        let before = Utc::now();

        let Ok((_, Json(created))) = create(&store, Some(48)).await else {
            panic!("create should succeed");
        };

        let hours = (created.expires_at - before).num_minutes() as f64 / 60.0;
        assert!(
            (47.9..=48.2).contains(&hours),
            "expected ~48 hours, got {hours}"
        );
    }

    /// Omitting it keeps the seven-day default a caller that predates the
    /// field relies on.
    #[tokio::test]
    async fn omitting_it_keeps_the_seven_day_default() {
        let store = make_store();
        let before = Utc::now();

        let Ok((_, Json(created))) = create(&store, None).await else {
            panic!("create should succeed");
        };

        let days = (created.expires_at - before).num_hours() as f64 / 24.0;
        assert!((6.9..=7.1).contains(&days), "expected ~7 days, got {days}");
    }

    /// An invitation token creates an account, so an unbounded lifetime is a
    /// live link sitting in an inbox forever. Refused rather than clamped: a
    /// clamp would report success for a deadline the server did not honour,
    /// which is the shape of the bug this change fixes.
    #[tokio::test]
    async fn a_lifetime_past_the_ceiling_is_refused() {
        let store = make_store();

        match create(&store, Some(INVITE_TOKEN_MAX_TTL_HOURS + 1)).await {
            Err(status) => assert_eq!(status, StatusCode::BAD_REQUEST),
            Ok(_) => panic!("a lifetime past the ceiling must be refused"),
        }
    }

    #[tokio::test]
    async fn the_ceiling_itself_is_allowed() {
        let store = make_store();
        assert!(
            create(&store, Some(INVITE_TOKEN_MAX_TTL_HOURS))
                .await
                .is_ok(),
            "the ceiling is inclusive"
        );
    }

    #[tokio::test]
    async fn zero_hours_is_refused() {
        let store = make_store();
        match create(&store, Some(0)).await {
            Err(status) => assert_eq!(status, StatusCode::BAD_REQUEST),
            Ok(_) => panic!("zero hours must be refused"),
        }
    }
}

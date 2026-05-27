//! OIDC HTTP endpoints — login redirect + callback.
//!
//! Routes (both public, no auth header expected):
//!   GET /v1/auth/oidc/login           302 to IdP authorize URL
//!   GET /v1/auth/oidc/callback?...    finishes the dance, mints tokens
//!
//! On success the callback returns a `TokenResponse` JSON directly so
//! the UI's existing `fetch('/v1/auth/oidc/callback')` handler picks
//! it up. (Alternative is a browser redirect with the tokens in
//! query/fragment; both work but JSON is simpler to test.)

use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
};
use chrono::Utc;
use croniq_auth::api_key::hash_api_key;
use croniq_auth::jwt::issue_token_pair;
use croniq_auth::{AuthMethod, CallerType, default_scopes_for_role};
use croniq_store::models::{OidcIdentity, OidcPendingLogin, RefreshToken, Role, User};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::ServerState;
use crate::api::audit;
use crate::api::auth_endpoints::TokenResponse;

const PENDING_TTL: Duration = Duration::from_secs(600); // 10 min

#[derive(Deserialize)]
pub struct CallbackParams {
    pub code: String,
    pub state: String,
}

#[derive(Serialize)]
pub struct OidcConfigResponse {
    pub enabled: bool,
    pub provider_name: Option<String>,
    pub login_url: Option<String>,
}

/// Combined sign-in-method probe — what the login UI hits before any auth
/// happens (issue #138). Keeps OIDC and password gates in one payload so a
/// fresh LoginPage only needs a single fetch.
#[derive(Serialize)]
pub struct AuthConfigResponse {
    pub oidc: OidcConfigResponse,
    pub password: PasswordConfigResponse,
    pub totp: TotpConfigResponse,
}

#[derive(Serialize)]
pub struct PasswordConfigResponse {
    pub enabled: bool,
}

/// Whether the server enforces 2FA for every password login. The login UI
/// reads this to decide if the code field should be shown up-front (and the
/// whole login done in a single request) rather than revealed after the
/// credential probe.
#[derive(Serialize)]
pub struct TotpConfigResponse {
    pub required: bool,
}

/// `GET /v1/auth/oidc/login` — 302-redirect to the IdP's authorize URL.
pub async fn handle_login(State(state): State<Arc<ServerState>>) -> Result<Response, StatusCode> {
    let provider = state.oidc.clone().ok_or(StatusCode::NOT_FOUND)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    // Opportunistically purge stale pending-login rows.
    let _ = store.oidc_pending_purge_expired(Utc::now());

    let (auth_url, csrf, nonce) = provider.authorize();
    let now = Utc::now();
    let pending = OidcPendingLogin {
        state: csrf.clone(),
        nonce,
        redirect_to: None,
        created_at: now,
        expires_at: now + chrono::Duration::from_std(PENDING_TTL).unwrap(),
    };
    store
        .oidc_pending_create(&pending)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Redirect::to(&auth_url).into_response())
}

/// `GET /v1/auth/oidc/callback?code=&state=` — finish the flow,
/// JIT-create user if needed, mint tokens, return JSON.
pub async fn handle_callback(
    State(state): State<Arc<ServerState>>,
    Query(params): Query<CallbackParams>,
) -> Result<Json<TokenResponse>, StatusCode> {
    let provider = state.oidc.clone().ok_or(StatusCode::NOT_FOUND)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let jwt_config = state
        .jwt_config
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    // Atomic take-and-delete — state is single-use even under
    // concurrent callbacks.
    let pending = store
        .oidc_pending_take(&params.state)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if Utc::now() > pending.expires_at {
        return Err(StatusCode::GONE);
    }

    let oidc_user = provider
        .exchange(&params.code, &pending.nonce)
        .await
        .map_err(|e| {
            tracing::warn!(target: "croniq::oidc", error = %e, "OIDC token exchange failed");
            StatusCode::UNAUTHORIZED
        })?;

    // Look up existing identity → linked user. If none, JIT-create
    // with default_role from the OIDC config.
    let user = match store
        .oidc_get_by_subject(&provider.config.provider_name, &oidc_user.subject)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        Some(existing) => store
            .users_get_by_id(&existing.user_id)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?,
        None => jit_create_user(store, &provider.config.default_role, &oidc_user)?,
    };

    if !user.is_active {
        return Err(StatusCode::FORBIDDEN);
    }

    let now = Utc::now();
    let _ = store.oidc_link(&OidcIdentity {
        provider: provider.config.provider_name.clone(),
        subject: oidc_user.subject.clone(),
        user_id: user.user_id.clone(),
        email: oidc_user.email.clone(),
        linked_at: now,
        last_login_at: Some(now),
    });
    let _ = store.oidc_touch_last_login(&provider.config.provider_name, &oidc_user.subject, now);

    // Mint the standard access + refresh pair. `auth_method = Oidc`
    // so audit logs can tell SSO sessions apart from password ones.
    let scopes = default_scopes_for_role(user.role);
    let pair = issue_token_pair(
        jwt_config,
        &user.user_id,
        &user.user_id,
        CallerType::User,
        Some(&user.user_id),
        Some(user.role),
        AuthMethod::Oidc,
        &scopes,
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let user_id = user.user_id.clone();
    let _ = store.users_set_last_login(&user_id, now);
    let refresh_hash = hash_api_key(&pair.refresh_token);
    let _ = store.create_refresh_token(&RefreshToken {
        token_hash: refresh_hash,
        client_id: user_id.clone(),
        user_id: Some(user_id.clone()),
        expires_at: pair.refresh_expires_at,
        revoked_at: None,
        created_at: now,
    });

    audit::record_event(
        store,
        "oidc",
        Some(&user_id),
        "auth.oidc_login_success",
        "user",
        Some(&user_id),
    );
    Ok(Json(TokenResponse {
        access_token: pair.access_token,
        refresh_token: pair.refresh_token,
        token_type: "Bearer".into(),
        expires_in: jwt_config.access_ttl_secs,
    }))
}

/// `GET /v1/auth/oidc/config` — read-only metadata so the login UI can
/// hide the SSO button when OIDC isn't configured. No secrets here.
///
/// Kept for back-compat with anything probing the OIDC-only endpoint;
/// the canonical probe is now [`handle_auth_config`] (`GET /v1/auth/config`).
pub async fn handle_config(State(state): State<Arc<ServerState>>) -> Json<OidcConfigResponse> {
    Json(oidc_config(&state))
}

/// `GET /v1/auth/config` — read-only summary of every UI sign-in method
/// (issue #138). Combines [`handle_config`] with the password-login flag.
pub async fn handle_auth_config(State(state): State<Arc<ServerState>>) -> Json<AuthConfigResponse> {
    Json(AuthConfigResponse {
        oidc: oidc_config(&state),
        password: PasswordConfigResponse {
            enabled: state.password_login_enabled,
        },
        totp: TotpConfigResponse {
            required: state.require_totp,
        },
    })
}

fn oidc_config(state: &ServerState) -> OidcConfigResponse {
    if let Some(provider) = &state.oidc {
        OidcConfigResponse {
            enabled: true,
            provider_name: Some(provider.config.provider_name.clone()),
            login_url: Some(format!(
                "{}/v1/auth/oidc/login",
                state.app_base_url.trim_end_matches('/')
            )),
        }
    } else {
        OidcConfigResponse {
            enabled: false,
            provider_name: None,
            login_url: None,
        }
    }
}

// ─── JIT user creation ───────────────────────────────────────────────────────

fn jit_create_user(
    store: &crate::store::DynStore,
    default_role: &Role,
    oidc_user: &crate::oidc::OidcUser,
) -> Result<User, StatusCode> {
    let now = Utc::now();
    let user_id = Uuid::new_v4().to_string();
    let username = oidc_user
        .preferred_username
        .clone()
        .or_else(|| oidc_user.email.clone())
        .unwrap_or_else(|| format!("oidc-{}", &user_id[..8]));

    // Username collision guard — if a local user already owns this
    // name, refuse the JIT-link. Admin must rename or merge manually.
    if let Some(existing) = store
        .users_get_by_username(&username)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        // If the existing row has no OIDC link yet, refuse to silently
        // hijack a password-only account.
        return if existing.is_active {
            Err(StatusCode::CONFLICT)
        } else {
            Err(StatusCode::FORBIDDEN)
        };
    }

    let user = User {
        user_id: user_id.clone(),
        username,
        email: oidc_user.email.clone(),
        display_name: oidc_user.display_name.clone(),
        role: *default_role,
        is_active: true,
        created_at: now,
        updated_at: now,
        last_login_at: Some(now),
    };
    store
        .users_create(&user)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(user)
}

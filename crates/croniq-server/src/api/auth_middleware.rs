//! Auth middleware: extracts CallerContext from Bearer JWT or ApiKey header.

use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use croniq_auth::api_key::{hash_api_key, hash_token};
use croniq_auth::context::Scope;
use croniq_auth::jwt::validate_token;
use croniq_auth::{AuthMethod, CallerContext, CallerType};

use super::ServerState;

/// Auth middleware for routes that require authentication.
/// Extracts CallerContext and inserts it as a request extension.
///
/// Supports:
/// - `Authorization: Bearer <jwt>` — JWT token validation
/// - `Authorization: ApiKey <raw_key>` — API key hash lookup
///
/// Skipped when `ServerState.jwt_config` is None (auth disabled).
pub async fn require_auth(
    State(state): State<Arc<ServerState>>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let Some(ref jwt_config) = state.jwt_config else {
        // Auth not configured — open mode for tests and unconfigured dev
        // servers. Inject a synthetic admin context so per-handler
        // `require_scope` checks pass through. Production deployments must
        // configure JWT/API keys to actually enforce anything.
        req.extensions_mut().insert(CallerContext {
            caller_type: CallerType::User,
            caller_id: "anonymous".into(),
            client_id: "anonymous".into(),
            user_id: None,
            role: None,
            auth_method: AuthMethod::ApiKey,
            scopes: vec![Scope::ADMIN.to_string()],
        });
        return Ok(next.run(req).await);
    };

    let auth_header = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let Some(header) = auth_header else {
        return Err(StatusCode::UNAUTHORIZED);
    };

    let ctx = if let Some(token) = header.strip_prefix("Bearer ") {
        // PAT short-circuit: a "Bearer croniq_pat_…" header is a PAT,
        // not a JWT. Distinguishing by prefix avoids client confusion
        // (every browser-facing flow uses `Bearer`) while keeping the
        // JWT path the default.
        if token.starts_with("croniq_pat_") {
            resolve_pat(state.as_ref(), token).await?
        } else {
            validate_token(jwt_config, token).map_err(|_| StatusCode::UNAUTHORIZED)?
        }
    } else if let Some(raw_pat) = header.strip_prefix("PAT ") {
        // Explicit `Authorization: PAT croniq_pat_…` form for clients
        // that prefer not to overload Bearer.
        resolve_pat(state.as_ref(), raw_pat).await?
    } else if let Some(raw_key) = header.strip_prefix("ApiKey ") {
        // API key lookup
        let store = state
            .store
            .as_ref()
            .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
        let key_hash = hash_api_key(raw_key);
        let api_key = store
            .find_api_key_by_hash(&key_hash)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(StatusCode::UNAUTHORIZED)?;

        // Check not revoked
        if api_key.revoked_at.is_some() {
            return Err(StatusCode::UNAUTHORIZED);
        }

        // Check not expired
        if let Some(expires) = api_key.expires_at
            && chrono::Utc::now() > expires
        {
            return Err(StatusCode::UNAUTHORIZED);
        }

        // Load client for scopes
        let client = store
            .get_client(&api_key.client_id)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(StatusCode::UNAUTHORIZED)?;

        if !client.is_active {
            return Err(StatusCode::UNAUTHORIZED);
        }

        CallerContext {
            caller_type: CallerType::ApiKey,
            caller_id: api_key.key_id,
            client_id: api_key.client_id,
            user_id: None,
            role: None,
            auth_method: AuthMethod::ApiKey,
            scopes: client.scopes,
        }
    } else {
        return Err(StatusCode::UNAUTHORIZED);
    };

    req.extensions_mut().insert(ctx);
    Ok(next.run(req).await)
}

/// Scope-checking extractor. Use after require_auth middleware.
pub fn require_scope(ctx: &CallerContext, scope: &str) -> Result<(), StatusCode> {
    if ctx.has_scope(scope) {
        Ok(())
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

/// Guard for every endpoint that issues a credential (PAT, API client,
/// API key, client token): the scopes stamped onto the new credential
/// must be a subset of the scopes carried by the credential presented on
/// this request. `admin` callers are unrestricted — `admin` is the
/// wildcard, so every set is a subset of it.
///
/// Without this bound a narrowly scoped credential is not a boundary at
/// all: it can re-mint itself with wider rights. Any new issuing
/// endpoint must call this before persisting the scope list.
pub fn require_grantable_scopes(
    ctx: &CallerContext,
    requested: &[String],
) -> Result<(), StatusCode> {
    if ctx.can_grant_scopes(requested) {
        Ok(())
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

/// Resolve an `Authorization: ... croniq_pat_…` header into a
/// CallerContext. Validates revocation + expiry + owning user is
/// active, then stamps `last_used_at` (best-effort, errors swallowed).
async fn resolve_pat(state: &ServerState, raw_token: &str) -> Result<CallerContext, StatusCode> {
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let token_hash = hash_token(raw_token);
    let pat = store
        .pat_find_by_hash(&token_hash)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;
    if pat.revoked_at.is_some() {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if let Some(expires) = pat.expires_at
        && chrono::Utc::now() > expires
    {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let user = store
        .users_get_by_id(&pat.user_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;
    if !user.is_active {
        return Err(StatusCode::UNAUTHORIZED);
    }
    // Best-effort touch; do not fail the request if the UPDATE blocks.
    let _ = store.pat_touch_last_used(&pat.token_id, chrono::Utc::now());

    Ok(CallerContext {
        caller_type: CallerType::User,
        caller_id: user.user_id.clone(),
        client_id: user.user_id.clone(),
        user_id: Some(user.user_id),
        role: Some(user.role),
        auth_method: AuthMethod::Pat,
        scopes: pat.scopes,
    })
}

//! Auth middleware: extracts CallerContext from Bearer JWT or ApiKey header.

use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use croniq_auth::api_key::{hash_api_key, hash_token};

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
/// Fails closed with 401 when `ServerState.jwt_config` is None: without a
/// signing key there is no way to authenticate anyone, so nobody is
/// authenticated (issue #431).
pub async fn require_auth(
    State(state): State<Arc<ServerState>>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let Some(ref jwt_config) = state.jwt_config else {
        // No JWT config — refuse the request instead of minting a synthetic
        // `admin` caller (issue #431). The old fail-open branch made the
        // entire REST API and `/mcp` reachable as admin, and the only thing
        // standing between that and a shipped binary was `main.rs` always
        // passing `Some`. The safety property now lives here, where it can be
        // read off the middleware, rather than emerging from one call site.
        //
        // The scope-less context is inserted anyway: `next` is never run on
        // this path, but any future code that reaches for `CallerContext`
        // without going through `require_auth` finds a caller that can do
        // nothing rather than no caller at all.
        req.extensions_mut().insert(CallerContext {
            caller_type: CallerType::User,
            caller_id: "anonymous".into(),
            client_id: "anonymous".into(),
            user_id: None,
            role: None,
            auth_method: AuthMethod::ApiKey,
            scopes: Vec::new(),
            token_generation: None,
        });
        tracing::warn!(
            "rejecting an authenticated route: the server has no JWT configuration, \
             so no credential can be verified"
        );
        return Err(StatusCode::UNAUTHORIZED);
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
            let ctx = validate_token(jwt_config, token).map_err(|_| StatusCode::UNAUTHORIZED)?;
            enforce_token_generation(state.as_ref(), &ctx)?;
            ctx
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
            // API keys have no user row; revocation is the `revoked_at`
            // column, checked above on every request.
            token_generation: None,
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
        // PATs carry their own revocation column, re-read above on every
        // request, so they need no generation claim.
        token_generation: None,
    })
}

/// Reject a JWT that was minted before the user's credentials last changed
/// (issue #431).
///
/// A password change, a password reset, or a deactivation bumps
/// `users.token_generation`; every access token carries the generation it was
/// minted under. Without this check those events left already-issued access
/// tokens working until `exp` — up to an hour — so "I reset the password to
/// lock an attacker out" did not actually lock them out.
///
/// This costs one primary-key lookup per JWT-authenticated request. The
/// API-key and PAT paths already do store I/O per request; this brings the JWT
/// path in line, which is the price of making a stateless token revocable.
///
/// Skipped when the token has no `user_id` (API-key JWTs have nothing to
/// check) or the server has no store (in-memory test servers).
fn enforce_token_generation(state: &ServerState, ctx: &CallerContext) -> Result<(), StatusCode> {
    let Some(user_id) = ctx.user_id.as_deref() else {
        return Ok(());
    };
    let Some(store) = state.store.as_ref() else {
        return Ok(());
    };

    let current = store
        .users_token_generation(user_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    // No row means the user was deleted. Their tokens die with them.
    let Some(current) = current else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    // A token minted before the upgrade carries no claim and reads as
    // generation 0, which is what every existing row was backfilled to — so a
    // rolling restart does not sign everyone out. The first bump on an account
    // invalidates those tokens along with the rest.
    if ctx.token_generation.unwrap_or(0) != current {
        tracing::info!(
            caller = %ctx.caller_id,
            "rejecting a token minted under a superseded credential generation"
        );
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(())
}

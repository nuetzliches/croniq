//! Auth middleware: extracts CallerContext from Bearer JWT or ApiKey header.

use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use croniq_auth::api_key::hash_api_key;
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
        // JWT validation
        validate_token(jwt_config, token).map_err(|_| StatusCode::UNAUTHORIZED)?
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

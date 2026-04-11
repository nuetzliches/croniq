//! JWT token issuance and validation.

use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use crate::context::{CallerContext, CallerType};

/// JWT configuration.
#[derive(Debug, Clone)]
pub struct JwtConfig {
    /// Secret key for HS256 signing.
    pub secret: String,
    /// Token validity in seconds. Default: 3600 (1 hour).
    pub access_ttl_secs: i64,
    /// Refresh token validity in seconds. Default: 604800 (7 days).
    pub refresh_ttl_secs: i64,
    /// Issuer claim.
    pub issuer: String,
}

impl Default for JwtConfig {
    fn default() -> Self {
        Self {
            secret: "croniq-dev-secret-change-me".into(),
            access_ttl_secs: 3600,
            refresh_ttl_secs: 604800,
            issuer: "croniq".into(),
        }
    }
}

/// Claims embedded in the JWT.
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub client_id: String,
    pub caller_type: String,
    pub scopes: Vec<String>,
    pub iss: String,
    pub exp: i64,
    pub iat: i64,
}

/// A pair of access + refresh tokens.
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub access_expires_at: chrono::DateTime<Utc>,
    pub refresh_expires_at: chrono::DateTime<Utc>,
}

/// Issue an access + refresh token pair.
pub fn issue_token_pair(
    config: &JwtConfig,
    caller_id: &str,
    client_id: &str,
    caller_type: CallerType,
    scopes: &[String],
) -> Result<TokenPair, AuthError> {
    let now = Utc::now();
    let access_exp = now + Duration::seconds(config.access_ttl_secs);
    let refresh_exp = now + Duration::seconds(config.refresh_ttl_secs);
    let caller_type_str = match caller_type {
        CallerType::ApiKey => "apikey",
        CallerType::User => "user",
    };

    let access_claims = Claims {
        sub: caller_id.to_string(),
        client_id: client_id.to_string(),
        caller_type: caller_type_str.to_string(),
        scopes: scopes.to_vec(),
        iss: config.issuer.clone(),
        exp: access_exp.timestamp(),
        iat: now.timestamp(),
    };

    let access_token = encode(
        &Header::default(),
        &access_claims,
        &EncodingKey::from_secret(config.secret.as_bytes()),
    )
    .map_err(|e| AuthError::TokenError(e.to_string()))?;

    // Refresh token is a random string, not a JWT — stored hashed in the DB
    let refresh_token = uuid::Uuid::new_v4().to_string();

    Ok(TokenPair {
        access_token,
        refresh_token,
        access_expires_at: access_exp,
        refresh_expires_at: refresh_exp,
    })
}

/// Validate a JWT and extract the caller context.
pub fn validate_token(config: &JwtConfig, token: &str) -> Result<CallerContext, AuthError> {
    let mut validation = Validation::default();
    validation.set_issuer(&[&config.issuer]);

    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(config.secret.as_bytes()),
        &validation,
    )
    .map_err(|e| AuthError::TokenError(e.to_string()))?;

    let caller_type = match token_data.claims.caller_type.as_str() {
        "apikey" => CallerType::ApiKey,
        _ => CallerType::User,
    };

    Ok(CallerContext {
        caller_type,
        caller_id: token_data.claims.sub,
        client_id: token_data.claims.client_id,
        scopes: token_data.claims.scopes,
    })
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("token error: {0}")]
    TokenError(String),

    #[error("invalid credentials")]
    InvalidCredentials,

    #[error("account locked")]
    AccountLocked,

    #[error("expired")]
    Expired,

    #[error("store error: {0}")]
    Store(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_and_validate_round_trip() {
        let config = JwtConfig::default();
        let pair = issue_token_pair(
            &config,
            "user-1",
            "client-1",
            CallerType::User,
            &["jobs:read".into(), "runners:read".into()],
        )
        .unwrap();

        let ctx = validate_token(&config, &pair.access_token).unwrap();
        assert_eq!(ctx.caller_id, "user-1");
        assert_eq!(ctx.client_id, "client-1");
        assert_eq!(ctx.caller_type, CallerType::User);
        assert!(ctx.has_scope("jobs:read"));
        assert!(!ctx.has_scope("admin"));
    }

    #[test]
    fn invalid_token_rejected() {
        let config = JwtConfig::default();
        let result = validate_token(&config, "not.a.valid.token");
        assert!(result.is_err());
    }

    #[test]
    fn wrong_secret_rejected() {
        let config1 = JwtConfig { secret: "secret-1".into(), ..Default::default() };
        let config2 = JwtConfig { secret: "secret-2".into(), ..Default::default() };

        let pair = issue_token_pair(&config1, "u", "c", CallerType::User, &[]).unwrap();
        let result = validate_token(&config2, &pair.access_token);
        assert!(result.is_err());
    }
}

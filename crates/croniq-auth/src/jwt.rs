//! JWT token issuance and validation.

use chrono::{Duration, Utc};
use croniq_store::models::Role;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};

use crate::context::{AuthMethod, CallerContext, CallerType};

/// Issuer of all JWTs minted by croniq-server.
///
/// **Hard-cut:** any token whose `iss` claim does not exactly match this
/// constant is rejected by `validate_token`. When the auth model changes
/// in a way that breaks existing claims (added required fields, role
/// inference, etc.), bump the version suffix here. PR-A1 introduced the
/// user_id + role + auth_method claims, which existing `iss: "croniq"`
/// tokens lack, so the issuer moves to `"croniq-v1"`. Old tokens become
/// invalid the moment a server with this constant rolls out — operators
/// are expected to re-login (and runners re-mint their JWT through the
/// API-key flow, which is unaffected because it doesn't carry a JWT).
pub const JWT_ISSUER: &str = "croniq-v1";

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

/// Default access-token validity: 1 hour.
pub const DEFAULT_ACCESS_TTL_SECS: i64 = 3600;
/// Default refresh-token validity: 7 days.
pub const DEFAULT_REFRESH_TTL_SECS: i64 = 604_800;

impl JwtConfig {
    /// Build a config around `secret`, with the shipped TTLs and issuer.
    ///
    /// There is deliberately no `Default` impl (issue #431). The old one
    /// carried `secret: "croniq-dev-secret-change-me"`, so any future
    /// `JwtConfig::default()` — or a `..Default::default()` that forgot to
    /// name `secret` — would have silently signed production tokens with a
    /// string published in this repository. Requiring the secret as an
    /// argument makes that mistake unrepresentable; `..JwtConfig::new(secret)`
    /// still covers the "override one field" case.
    pub fn new(secret: impl Into<String>) -> Self {
        Self {
            secret: secret.into(),
            access_ttl_secs: DEFAULT_ACCESS_TTL_SECS,
            refresh_ttl_secs: DEFAULT_REFRESH_TTL_SECS,
            issuer: JWT_ISSUER.into(),
        }
    }

    /// A config with a freshly generated random secret, for tests.
    ///
    /// Public because croniq-server's integration tests need it. Safe to
    /// expose: the secret is a per-call UUID, so even a misuse in production
    /// code fails closed (tokens stop validating across restarts) rather than
    /// signing with a value an attacker can look up.
    pub fn for_tests() -> Self {
        Self::new(uuid::Uuid::new_v4().to_string())
    }
}

/// Claims embedded in the JWT.
///
/// `user_id`, `role`, and `auth_method` were added in PR-A1. Old tokens
/// that lack these fields are rejected via the issuer-version bump (see
/// [`JWT_ISSUER`]). `user_id` and `role` are `Option` because API-key
/// callers don't have a user.
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub client_id: String,
    pub caller_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Authentication method used to mint this token. Stable enum
    /// serialised as lowercase string ("password" / "apikey" /
    /// "pat" / "oidc").
    pub auth_method: String,
    pub scopes: Vec<String>,
    /// Credential generation this token was minted under (issue #431). The
    /// auth middleware rejects the token when it no longer matches the user
    /// row, which is how a password change / reset / deactivation invalidates
    /// tokens already issued. `#[serde(default)]` so tokens minted before the
    /// upgrade deserialise as `None` and are read as generation 0 — a rolling
    /// restart does not sign everyone out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_generation: Option<i64>,
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

fn caller_type_to_str(t: CallerType) -> &'static str {
    match t {
        CallerType::ApiKey => "apikey",
        CallerType::User => "user",
    }
}

fn auth_method_to_str(m: AuthMethod) -> &'static str {
    match m {
        AuthMethod::Password => "password",
        AuthMethod::ApiKey => "apikey",
        AuthMethod::Pat => "pat",
        AuthMethod::Oidc => "oidc",
    }
}

fn parse_auth_method(s: &str) -> AuthMethod {
    match s {
        "password" => AuthMethod::Password,
        "pat" => AuthMethod::Pat,
        "oidc" => AuthMethod::Oidc,
        _ => AuthMethod::ApiKey,
    }
}

/// Issue an access + refresh token pair.
///
/// For users: pass `Some(user_id)` and `Some(role)`. For API keys: pass
/// `None` for both — the caller's permissions come from `scopes` directly.
#[allow(clippy::too_many_arguments)]
pub fn issue_token_pair(
    config: &JwtConfig,
    caller_id: &str,
    client_id: &str,
    caller_type: CallerType,
    user_id: Option<&str>,
    role: Option<Role>,
    auth_method: AuthMethod,
    scopes: &[String],
    token_generation: Option<i64>,
) -> Result<TokenPair, AuthError> {
    let now = Utc::now();
    let access_exp = now + Duration::seconds(config.access_ttl_secs);
    let refresh_exp = now + Duration::seconds(config.refresh_ttl_secs);

    let access_claims = Claims {
        sub: caller_id.to_string(),
        client_id: client_id.to_string(),
        caller_type: caller_type_to_str(caller_type).to_string(),
        user_id: user_id.map(|s| s.to_string()),
        role: role.map(|r| r.as_str().to_string()),
        auth_method: auth_method_to_str(auth_method).to_string(),
        scopes: scopes.to_vec(),
        token_generation,
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

/// Short-lived token issued between password-success and TOTP-verify
/// during step-up login. Distinct claim shape (no scopes, no role) so
/// `validate_token` rejects it for normal API calls — the only valid
/// consumer is `/v1/auth/login/totp` which calls
/// [`validate_mfa_token`] instead.
#[derive(Debug, Serialize, Deserialize)]
struct MfaClaims {
    sub: String,
    purpose: String, // always "mfa"
    iss: String,
    exp: i64,
    iat: i64,
    /// Unique per issuance. Without it the claim set is a pure function of
    /// (user, second), so two logins in the same second produce the exact
    /// same token — and the server's per-token second-factor failure budget
    /// (issue #428) would then follow a user into their *next* login attempt
    /// instead of being cleared by redoing the password step. Defaulted on
    /// deserialisation so tokens minted before the upgrade still validate
    /// through a rolling restart.
    #[serde(default)]
    jti: String,
}

/// Mint an MFA step-up token for `user_id`. TTL is 5 minutes —
/// generous enough to switch to an authenticator app, short enough
/// that a leaked half-state-token decays quickly.
pub fn issue_mfa_token(config: &JwtConfig, user_id: &str) -> Result<(String, i64), AuthError> {
    let now = Utc::now();
    let exp = now + Duration::seconds(300);
    let claims = MfaClaims {
        sub: user_id.to_string(),
        purpose: "mfa".into(),
        iss: config.issuer.clone(),
        exp: exp.timestamp(),
        iat: now.timestamp(),
        jti: uuid::Uuid::new_v4().to_string(),
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(config.secret.as_bytes()),
    )
    .map_err(|e| AuthError::TokenError(e.to_string()))?;
    Ok((token, 300))
}

/// Validate an MFA step-up token. Returns the user_id it was issued for.
pub fn validate_mfa_token(config: &JwtConfig, token: &str) -> Result<String, AuthError> {
    let mut validation = Validation::default();
    validation.set_issuer(&[&config.issuer]);
    let data = decode::<MfaClaims>(
        token,
        &DecodingKey::from_secret(config.secret.as_bytes()),
        &validation,
    )
    .map_err(|e| AuthError::TokenError(e.to_string()))?;
    if data.claims.purpose != "mfa" {
        return Err(AuthError::TokenError("not an MFA token".into()));
    }
    Ok(data.claims.sub)
}

/// Mint a short-lived TOTP-enrolment token for `user_id`. Issued after a
/// password success when enforced 2FA is on but the account has no confirmed
/// TOTP yet, so the user can enrol inline and finish login instead of being
/// locked out. Purpose claim is `"totp_enroll"`, so it's useless for normal
/// API calls and for the MFA step-up. TTL is 10 minutes — enrolment (scan the
/// QR, save recovery codes, enter a code) takes longer than a plain code entry.
pub fn issue_totp_enroll_token(
    config: &JwtConfig,
    user_id: &str,
) -> Result<(String, i64), AuthError> {
    let now = Utc::now();
    let exp = now + Duration::seconds(600);
    let claims = MfaClaims {
        sub: user_id.to_string(),
        purpose: "totp_enroll".into(),
        iss: config.issuer.clone(),
        exp: exp.timestamp(),
        iat: now.timestamp(),
        jti: uuid::Uuid::new_v4().to_string(),
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(config.secret.as_bytes()),
    )
    .map_err(|e| AuthError::TokenError(e.to_string()))?;
    Ok((token, 600))
}

/// Validate a TOTP-enrolment token. Returns the user_id it was issued for.
pub fn validate_totp_enroll_token(config: &JwtConfig, token: &str) -> Result<String, AuthError> {
    let mut validation = Validation::default();
    validation.set_issuer(&[&config.issuer]);
    let data = decode::<MfaClaims>(
        token,
        &DecodingKey::from_secret(config.secret.as_bytes()),
        &validation,
    )
    .map_err(|e| AuthError::TokenError(e.to_string()))?;
    if data.claims.purpose != "totp_enroll" {
        return Err(AuthError::TokenError("not a TOTP-enrolment token".into()));
    }
    Ok(data.claims.sub)
}

/// Validate a JWT and extract the caller context.
///
/// Rejects tokens whose `iss` claim doesn't match the configured issuer.
/// PR-A1 bumped the default issuer from `"croniq"` to `"croniq-v1"`, so
/// any pre-A1 token is rejected here (hard-cut migration; see [`JWT_ISSUER`]).
pub fn validate_token(config: &JwtConfig, token: &str) -> Result<CallerContext, AuthError> {
    let mut validation = Validation::default();
    validation.set_issuer(&[&config.issuer]);

    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(config.secret.as_bytes()),
        &validation,
    )
    .map_err(|e| AuthError::TokenError(e.to_string()))?;

    let claims = token_data.claims;
    let caller_type = match claims.caller_type.as_str() {
        "apikey" => CallerType::ApiKey,
        _ => CallerType::User,
    };
    let role = claims.role.as_deref().and_then(|s| s.parse::<Role>().ok());
    let auth_method = parse_auth_method(&claims.auth_method);

    Ok(CallerContext {
        caller_type,
        caller_id: claims.sub,
        client_id: claims.client_id,
        user_id: claims.user_id,
        role,
        auth_method,
        scopes: claims.scopes,
        token_generation: claims.token_generation,
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

    /// Fixed secret for the tests that need two configs to agree on the key
    /// while differing in some other field.
    const SHARED_SECRET: &str = "unit-test-secret";

    #[test]
    fn every_mfa_token_is_unique_and_still_validates() {
        // Two tokens minted for the same user in the same second must not be
        // byte-identical: the server's per-token second-factor failure budget
        // (issue #428) keys on the token, so a repeated password step has to
        // yield a genuinely new token.
        let cfg = JwtConfig::for_tests();
        let (a, ttl) = issue_mfa_token(&cfg, "u-1").unwrap();
        let (b, _) = issue_mfa_token(&cfg, "u-1").unwrap();
        assert_ne!(a, b, "mfa tokens must differ per issuance");
        assert_eq!(ttl, 300);
        assert_eq!(validate_mfa_token(&cfg, &a).unwrap(), "u-1");
        assert_eq!(validate_mfa_token(&cfg, &b).unwrap(), "u-1");
    }

    #[test]
    fn issue_and_validate_round_trip_for_user() {
        let config = JwtConfig::for_tests();
        let pair = issue_token_pair(
            &config,
            "user-1",
            "user-1",
            CallerType::User,
            Some("user-1"),
            Some(Role::Operator),
            AuthMethod::Password,
            &["jobs:read".into(), "runners:read".into()],
            None,
        )
        .unwrap();

        let ctx = validate_token(&config, &pair.access_token).unwrap();
        assert_eq!(ctx.caller_id, "user-1");
        assert_eq!(ctx.user_id.as_deref(), Some("user-1"));
        assert_eq!(ctx.role, Some(Role::Operator));
        assert_eq!(ctx.auth_method, AuthMethod::Password);
        assert_eq!(ctx.caller_type, CallerType::User);
        assert!(ctx.has_scope("jobs:read"));
        assert!(!ctx.has_scope("admin"));
    }

    #[test]
    fn issue_and_validate_round_trip_for_api_key() {
        let config = JwtConfig::for_tests();
        let pair = issue_token_pair(
            &config,
            "key-1",
            "client-1",
            CallerType::ApiKey,
            None,
            None,
            AuthMethod::ApiKey,
            &["jobs:read".into()],
            None,
        )
        .unwrap();

        let ctx = validate_token(&config, &pair.access_token).unwrap();
        assert_eq!(ctx.caller_id, "key-1");
        assert!(ctx.user_id.is_none());
        assert!(ctx.role.is_none());
        assert_eq!(ctx.auth_method, AuthMethod::ApiKey);
        assert_eq!(ctx.caller_type, CallerType::ApiKey);
    }

    #[test]
    fn invalid_token_rejected() {
        let config = JwtConfig::for_tests();
        let result = validate_token(&config, "not.a.valid.token");
        assert!(result.is_err());
    }

    #[test]
    fn wrong_secret_rejected() {
        let config1 = JwtConfig::new("secret-1");
        let config2 = JwtConfig::new("secret-2");

        let pair = issue_token_pair(
            &config1,
            "u",
            "c",
            CallerType::User,
            Some("u"),
            Some(Role::Admin),
            AuthMethod::Password,
            &[],
            None,
        )
        .unwrap();
        let result = validate_token(&config2, &pair.access_token);
        assert!(result.is_err());
    }

    #[test]
    fn old_issuer_rejected_as_hard_cut() {
        // Mint a token under the legacy issuer "croniq" and verify that
        // validating it against the default config (issuer "croniq-v1")
        // is rejected. This is the PR-A1 hard-cut migration guarantee.
        let legacy = JwtConfig {
            issuer: "croniq".into(),
            ..JwtConfig::new(SHARED_SECRET)
        };
        let pair = issue_token_pair(
            &legacy,
            "u",
            "c",
            CallerType::User,
            Some("u"),
            Some(Role::Admin),
            AuthMethod::Password,
            &["admin".into()],
            None,
        )
        .unwrap();

        // Current config uses "croniq-v1" — old issuer must be rejected.
        let current = JwtConfig::new(SHARED_SECRET);
        assert!(validate_token(&current, &pair.access_token).is_err());
    }
}

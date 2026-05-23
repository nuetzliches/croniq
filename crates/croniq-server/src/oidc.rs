//! OIDC/SSO integration — manual Authorization-Code flow.
//!
//! Tested against Authentik, Keycloak, Auth0. Scope is deliberately
//! limited to the parts Croniq needs:
//!   1. Discovery: GET `<issuer>/.well-known/openid-configuration`
//!   2. Authorize URL build (no PKCE — Croniq is a confidential client
//!      and the client_secret is already the binding factor).
//!   3. Token exchange: POST to the token endpoint with `code` +
//!      Basic-auth `client_id:client_secret`.
//!   4. ID token verify via JWKS + nonce check.
//!   5. Userinfo fetch for the email/name we don't get in the ID token.
//!
//! Configuration is **env-only** in PR-A5 — a follow-up PR-A5b will
//! add a Croniqfile `oidc { … }` block for IaC. Env vars:
//!   CRONIQ_OIDC_ISSUER           required to enable
//!   CRONIQ_OIDC_CLIENT_ID        required
//!   CRONIQ_OIDC_CLIENT_SECRET    required
//!   CRONIQ_OIDC_REDIRECT_URL     required
//!   CRONIQ_OIDC_DEFAULT_ROLE     optional, default "viewer"
//!   CRONIQ_OIDC_PROVIDER_NAME    optional, default "oidc"
//!   CRONIQ_OIDC_POST_LOGIN       optional, default "/"

use std::collections::HashMap;
use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use rand::RngCore;
use reqwest::Client as HttpClient;
use serde::Deserialize;
use thiserror::Error;

use croniq_store::models::Role;

#[derive(Debug, Error)]
pub enum OidcError {
    #[error("OIDC not configured")]
    NotConfigured,
    #[error("OIDC discovery failed: {0}")]
    Discovery(String),
    #[error("token exchange failed: {0}")]
    TokenExchange(String),
    #[error("invalid ID token: {0}")]
    InvalidToken(String),
    #[error("userinfo failed: {0}")]
    Userinfo(String),
}

/// Operator-configured OIDC settings.
#[derive(Debug, Clone)]
pub struct OidcConfig {
    pub issuer: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_url: String,
    pub default_role: Role,
    pub post_login_redirect: String,
    pub provider_name: String,
}

#[derive(Debug, Clone, Deserialize)]
struct Discovery {
    authorization_endpoint: String,
    token_endpoint: String,
    jwks_uri: String,
    userinfo_endpoint: Option<String>,
    issuer: String,
}

#[derive(Debug, Clone, Deserialize)]
struct Jwks {
    keys: Vec<Jwk>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct Jwk {
    kid: Option<String>,
    kty: String,
    #[serde(rename = "use")]
    usage: Option<String>,
    alg: Option<String>,
    n: Option<String>, // RSA modulus (b64url)
    e: Option<String>, // RSA exponent (b64url)
}

#[derive(Debug, Deserialize)]
struct TokenExchange {
    id_token: String,
    access_token: String,
}

#[derive(Debug, Deserialize)]
struct IdTokenClaims {
    sub: String,
    nonce: Option<String>,
    email: Option<String>,
    preferred_username: Option<String>,
    name: Option<String>,
    #[allow(dead_code)]
    aud: serde_json::Value,
    #[allow(dead_code)]
    iss: String,
    #[allow(dead_code)]
    exp: i64,
    #[allow(dead_code)]
    iat: i64,
}

#[derive(Debug, Deserialize)]
struct Userinfo {
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    preferred_username: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

pub struct OidcProvider {
    pub config: OidcConfig,
    http: HttpClient,
    discovery: Discovery,
}

impl OidcProvider {
    /// Fetch discovery doc, build the long-lived HTTP client.
    pub async fn discover(config: OidcConfig) -> Result<Self, OidcError> {
        let http = HttpClient::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| OidcError::Discovery(e.to_string()))?;

        let url = format!(
            "{}/.well-known/openid-configuration",
            config.issuer.trim_end_matches('/')
        );
        let discovery: Discovery = http
            .get(&url)
            .send()
            .await
            .map_err(|e| OidcError::Discovery(e.to_string()))?
            .error_for_status()
            .map_err(|e| OidcError::Discovery(e.to_string()))?
            .json()
            .await
            .map_err(|e| OidcError::Discovery(e.to_string()))?;
        if discovery.issuer.trim_end_matches('/') != config.issuer.trim_end_matches('/') {
            return Err(OidcError::Discovery(format!(
                "issuer mismatch: discovery returned {}",
                discovery.issuer
            )));
        }
        Ok(Self {
            config,
            http,
            discovery,
        })
    }

    /// Build the authorize URL plus the (state, nonce) pair the caller
    /// must persist for the round-trip.
    pub fn authorize(&self) -> (String, String, String) {
        let state = random_token();
        let nonce = random_token();
        let url = format!(
            "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&nonce={}",
            self.discovery.authorization_endpoint,
            url_enc(&self.config.client_id),
            url_enc(&self.config.redirect_url),
            url_enc("openid email profile"),
            url_enc(&state),
            url_enc(&nonce),
        );
        (url, state, nonce)
    }

    /// Exchange `code` for tokens, validate the ID token (signature,
    /// issuer, audience, nonce), and fetch userinfo. Returns the
    /// caller-shaped `OidcUser`.
    pub async fn exchange(&self, code: &str, expected_nonce: &str) -> Result<OidcUser, OidcError> {
        // Token exchange via Basic auth on the token endpoint.
        let resp = self
            .http
            .post(&self.discovery.token_endpoint)
            .basic_auth(&self.config.client_id, Some(&self.config.client_secret))
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", &self.config.redirect_url),
                ("client_id", &self.config.client_id),
            ])
            .send()
            .await
            .map_err(|e| OidcError::TokenExchange(e.to_string()))?;
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(OidcError::TokenExchange(body));
        }
        let tokens: TokenExchange = resp
            .json()
            .await
            .map_err(|e| OidcError::TokenExchange(e.to_string()))?;

        // Verify ID token via JWKS.
        let claims = self
            .verify_id_token(&tokens.id_token, expected_nonce)
            .await?;

        // Best-effort userinfo for fields missing from the ID token.
        let mut email = claims.email.clone();
        let mut preferred_username = claims.preferred_username.clone();
        let mut display_name = claims.name.clone();
        if let Some(userinfo_url) = &self.discovery.userinfo_endpoint
            && let Ok(resp) = self
                .http
                .get(userinfo_url)
                .bearer_auth(&tokens.access_token)
                .send()
                .await
            && resp.status().is_success()
            && let Ok(ui) = resp.json::<Userinfo>().await
        {
            email = email.or(ui.email);
            preferred_username = preferred_username.or(ui.preferred_username);
            display_name = display_name.or(ui.name);
        }

        Ok(OidcUser {
            subject: claims.sub,
            email,
            preferred_username,
            display_name,
        })
    }

    async fn verify_id_token(
        &self,
        token: &str,
        expected_nonce: &str,
    ) -> Result<IdTokenClaims, OidcError> {
        let header = decode_header(token).map_err(|e| OidcError::InvalidToken(e.to_string()))?;
        let kid = header
            .kid
            .ok_or_else(|| OidcError::InvalidToken("id_token has no kid header".into()))?;

        // Fetch + cache-less JWKS lookup (good enough for PR-A5; can
        // memoise later if the call site grows).
        let jwks: Jwks = self
            .http
            .get(&self.discovery.jwks_uri)
            .send()
            .await
            .map_err(|e| OidcError::Discovery(e.to_string()))?
            .error_for_status()
            .map_err(|e| OidcError::Discovery(e.to_string()))?
            .json()
            .await
            .map_err(|e| OidcError::Discovery(e.to_string()))?;
        let jwk = jwks
            .keys
            .into_iter()
            .find(|k| k.kid.as_deref() == Some(&kid))
            .ok_or_else(|| OidcError::InvalidToken(format!("kid {kid} not found in JWKS")))?;
        if jwk.kty != "RSA" {
            return Err(OidcError::InvalidToken(format!(
                "unsupported kty: {} (expected RSA)",
                jwk.kty
            )));
        }
        let n = jwk
            .n
            .as_deref()
            .ok_or_else(|| OidcError::InvalidToken("JWK missing modulus".into()))?;
        let e = jwk
            .e
            .as_deref()
            .ok_or_else(|| OidcError::InvalidToken("JWK missing exponent".into()))?;
        let key = DecodingKey::from_rsa_components(n, e)
            .map_err(|e| OidcError::InvalidToken(e.to_string()))?;

        let alg = match jwk.alg.as_deref() {
            Some("RS256") | None => Algorithm::RS256,
            Some("RS384") => Algorithm::RS384,
            Some("RS512") => Algorithm::RS512,
            Some(other) => {
                return Err(OidcError::InvalidToken(format!("unsupported alg: {other}")));
            }
        };
        let mut validation = Validation::new(alg);
        validation.set_issuer(&[&self.config.issuer]);
        validation.set_audience(&[&self.config.client_id]);

        let data = decode::<IdTokenClaims>(token, &key, &validation)
            .map_err(|e| OidcError::InvalidToken(e.to_string()))?;
        if data.claims.nonce.as_deref() != Some(expected_nonce) {
            return Err(OidcError::InvalidToken("nonce mismatch".into()));
        }
        Ok(data.claims)
    }
}

#[derive(Debug, Clone)]
pub struct OidcUser {
    pub subject: String,
    pub email: Option<String>,
    pub preferred_username: Option<String>,
    pub display_name: Option<String>,
}

pub type SharedOidcProvider = Option<Arc<OidcProvider>>;

/// Read the operator config from env vars. Returns `Err(NotConfigured)`
/// when any required var is missing — the caller logs + continues
/// with OIDC disabled.
pub fn config_from_env() -> Result<OidcConfig, OidcError> {
    let issuer = std::env::var("CRONIQ_OIDC_ISSUER").map_err(|_| OidcError::NotConfigured)?;
    let client_id = std::env::var("CRONIQ_OIDC_CLIENT_ID").map_err(|_| OidcError::NotConfigured)?;
    let client_secret =
        std::env::var("CRONIQ_OIDC_CLIENT_SECRET").map_err(|_| OidcError::NotConfigured)?;
    let redirect_url =
        std::env::var("CRONIQ_OIDC_REDIRECT_URL").map_err(|_| OidcError::NotConfigured)?;
    let default_role = std::env::var("CRONIQ_OIDC_DEFAULT_ROLE")
        .ok()
        .as_deref()
        .and_then(|s| s.parse::<Role>().ok())
        .unwrap_or(Role::Viewer);
    let provider_name =
        std::env::var("CRONIQ_OIDC_PROVIDER_NAME").unwrap_or_else(|_| "oidc".into());
    let post_login_redirect =
        std::env::var("CRONIQ_OIDC_POST_LOGIN").unwrap_or_else(|_| "/".into());
    Ok(OidcConfig {
        issuer,
        client_id,
        client_secret,
        redirect_url,
        default_role,
        post_login_redirect,
        provider_name,
    })
}

// ─── small helpers ──────────────────────────────────────────────────────────

fn random_token() -> String {
    let mut buf = [0u8; 24];
    rand::rng().fill_bytes(&mut buf);
    B64.encode(buf)
}

fn url_enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        // RFC 3986 unreserved characters; everything else gets %xx.
        let ok = b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~');
        if ok {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

// Compile-time unused-import suppressor: keep HashMap pulled in for
// the inevitable future JWKS-cache without making the lint angry.
#[allow(dead_code)]
const _: fn() -> HashMap<String, String> = HashMap::new;

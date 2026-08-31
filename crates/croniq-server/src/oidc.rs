//! OIDC/SSO integration — manual Authorization-Code flow.
//!
//! Tested against Authentik, Keycloak, Auth0. Scope is deliberately
//! limited to the parts Croniq needs:
//!   1. Discovery: GET `<issuer>/.well-known/openid-configuration`.
//!      The issuer and every endpoint the document advertises must be
//!      `https://` (loopback exempted) — see `require_https`.
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
use url::Url;

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
    /// Discovery and JWKS: public documents, redirects allowed.
    http: HttpClient,
    /// Token exchange and userinfo: both send a credential, so this client
    /// refuses to follow redirects. See [`OidcProvider::discover`].
    http_credentialed: HttpClient,
    discovery: Discovery,
}

impl OidcProvider {
    /// Fetch discovery doc, build the long-lived HTTP client.
    pub async fn discover(config: OidcConfig) -> Result<Self, OidcError> {
        // The issuer is the one URL an operator types by hand, and every
        // other endpoint below is taken from the document it serves. If that
        // first hop is plaintext, a network attacker rewrites the whole
        // discovery document — including `jwks_uri`, which decides whose
        // signature counts as a valid ID token. Refuse before we fetch.
        require_https("issuer", &config.issuer)?;

        let http = HttpClient::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| OidcError::Discovery(e.to_string()))?;

        // Separate client for the two requests that carry a credential: the
        // token POST sends `client_id:client_secret` as Basic auth, and
        // userinfo sends the access token as a bearer. reqwest already drops
        // those headers on a cross-origin redirect, but a redirect chain has
        // no legitimate role in either call, so refuse to follow one at all.
        let http_credentialed = HttpClient::builder()
            .timeout(std::time::Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
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

        // Every endpoint below is a URL this process will call, taken verbatim
        // from a document fetched over the network. The issuer match above
        // proves the document belongs to the configured issuer; it says
        // nothing about the scheme of the endpoints inside it. A single
        // `http://` entry would put the client_secret (token endpoint) or the
        // signing keys (jwks_uri) on the wire in the clear.
        //
        // Deliberately *not* a same-host check: real providers split these
        // across hosts (Google's issuer is accounts.google.com while its JWKS
        // lives on www.googleapis.com), so host pinning would reject valid
        // deployments while https already closes the transport gap.
        require_https("authorization_endpoint", &discovery.authorization_endpoint)?;
        require_https("token_endpoint", &discovery.token_endpoint)?;
        require_https("jwks_uri", &discovery.jwks_uri)?;
        if let Some(userinfo) = &discovery.userinfo_endpoint {
            require_https("userinfo_endpoint", userinfo)?;
        }

        Ok(Self {
            config,
            http,
            http_credentialed,
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
            .http_credentialed
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
                .http_credentialed
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

/// Build the final config by merging an optional Croniqfile `oidc {}`
/// block with the env vars. DSL fields win where set; env vars fill
/// the gaps. `client_secret` is env-only (it never appears in the DSL).
///
/// Returns `Err(NotConfigured)` when issuer / client_id / redirect_url /
/// client_secret can't be assembled from either source.
pub fn config_from_dsl_and_env(
    dsl: Option<&croniq_config::compile::OidcDslConfig>,
) -> Result<OidcConfig, OidcError> {
    fn dsl_or_env(dsl_val: Option<&String>, env_key: &str) -> Option<String> {
        dsl_val.cloned().or_else(|| std::env::var(env_key).ok())
    }

    let issuer = dsl
        .and_then(|d| d.issuer.clone())
        .or_else(|| std::env::var("CRONIQ_OIDC_ISSUER").ok())
        .ok_or(OidcError::NotConfigured)?;
    let client_id = dsl
        .and_then(|d| d.client_id.clone())
        .or_else(|| std::env::var("CRONIQ_OIDC_CLIENT_ID").ok())
        .ok_or(OidcError::NotConfigured)?;
    let client_secret =
        std::env::var("CRONIQ_OIDC_CLIENT_SECRET").map_err(|_| OidcError::NotConfigured)?;
    let redirect_url = dsl
        .and_then(|d| d.redirect_url.clone())
        .or_else(|| std::env::var("CRONIQ_OIDC_REDIRECT_URL").ok())
        .ok_or(OidcError::NotConfigured)?;

    let default_role_str = dsl_or_env(
        dsl.and_then(|d| d.default_role.as_ref()),
        "CRONIQ_OIDC_DEFAULT_ROLE",
    );
    let default_role = default_role_str
        .as_deref()
        .and_then(|s| s.parse::<Role>().ok())
        .unwrap_or(Role::Viewer);
    let provider_name = dsl_or_env(
        dsl.and_then(|d| d.provider_name.as_ref()),
        "CRONIQ_OIDC_PROVIDER_NAME",
    )
    .unwrap_or_else(|| "oidc".into());
    let post_login_redirect = dsl_or_env(
        dsl.and_then(|d| d.post_login_redirect.as_ref()),
        "CRONIQ_OIDC_POST_LOGIN",
    )
    .unwrap_or_else(|| "/".into());

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

/// Reject an OIDC endpoint that is not reachable over TLS.
///
/// Loopback is exempt: a developer running Keycloak on `http://localhost:8080`
/// has no transport to attack, and requiring a certificate there would push
/// people toward disabling the check outright.
fn require_https(label: &str, raw: &str) -> Result<(), OidcError> {
    let parsed = Url::parse(raw)
        .map_err(|e| OidcError::Discovery(format!("{label} is not a valid URL ({raw}): {e}")))?;
    match parsed.scheme() {
        "https" => Ok(()),
        "http" if is_loopback(&parsed) => Ok(()),
        other => Err(OidcError::Discovery(format!(
            "{label} must use https, got {other}:// ({raw})"
        ))),
    }
}

/// True when the URL's host is the local machine. `Url::host()` already
/// normalises `[::1]` to an `Ipv6` host, so no bracket handling is needed.
fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(d)) => d == "localhost",
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_endpoints_pass() {
        assert!(require_https("token_endpoint", "https://idp.example/token").is_ok());
        // Cross-host is fine: Google's JWKS does not live on its issuer host.
        assert!(require_https("jwks_uri", "https://www.googleapis.com/oauth2/v3/certs").is_ok());
    }

    #[test]
    fn plaintext_endpoints_are_rejected() {
        let err = require_https("jwks_uri", "http://idp.example/keys").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("jwks_uri"), "names the endpoint: {msg}");
        assert!(msg.contains("https"), "names the requirement: {msg}");
    }

    #[test]
    fn loopback_stays_usable_over_plaintext() {
        // A local Keycloak/Authentik is the normal development setup; there is
        // no transport between the two processes for an attacker to sit on.
        for url in [
            "http://localhost:8080/realms/croniq",
            "http://127.0.0.1:8080/token",
            "http://[::1]:8080/token",
        ] {
            assert!(require_https("issuer", url).is_ok(), "{url} should pass");
        }
    }

    #[test]
    fn a_host_that_merely_looks_local_is_not_loopback() {
        // `localhost.evil.example` resolves wherever its owner points it.
        for url in [
            "http://localhost.evil.example/token",
            "http://notlocalhost/token",
            "http://127.0.0.1.evil.example/token",
        ] {
            assert!(require_https("issuer", url).is_err(), "{url} should fail");
        }
    }

    #[test]
    fn non_http_schemes_are_rejected() {
        // `file://` and friends would make the URL an SSRF primitive rather
        // than an HTTP call.
        assert!(require_https("issuer", "file:///etc/passwd").is_err());
        assert!(require_https("issuer", "not a url").is_err());
    }
}

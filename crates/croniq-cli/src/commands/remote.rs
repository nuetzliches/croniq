//! Authenticated HTTP access to a running croniq-server (issue #475).
//!
//! The CLI's server commands used to issue naked requests — no credential,
//! and `.json()` called on whatever came back. Against a server with auth
//! enabled that turned a `401` into a serde decode error, so the operator saw
//! a parse failure instead of "your credential was rejected".
//!
//! Everything that talks to the server goes through [`Remote`], which carries
//! the credential and converts a non-2xx response into a message naming what
//! to do about it.

use miette::{IntoDiagnostic, Result, miette};
use reqwest::blocking::{Client, RequestBuilder, Response};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// A croniq-server endpoint plus the credential to reach it with.
pub struct Remote {
    url: String,
    api_key: Option<String>,
    client: Client,
}

impl Remote {
    pub fn new(url: &str, api_key: Option<String>) -> Self {
        Self {
            // A trailing slash would produce `//v1/...`, which some proxies
            // normalise and others 404 on.
            url: url.trim_end_matches('/').to_string(),
            api_key,
            client: Client::new(),
        }
    }

    fn auth(&self, req: RequestBuilder) -> RequestBuilder {
        match &self.api_key {
            Some(key) => req.header("authorization", format!("ApiKey {key}")),
            None => req,
        }
    }

    fn send(&self, req: RequestBuilder, path: &str) -> Result<Response> {
        let resp = self
            .auth(req)
            .send()
            .map_err(|e| miette!("Could not connect to {}{path}: {e}", self.url))?;
        self.check(resp, path)
    }

    /// Turn a non-2xx into something an operator can act on.
    ///
    /// The server answers `401`/`403` with an empty body and `409` with a
    /// JSON `{error, message}` (the env-managed refusal from #471 among
    /// them), so the message it took the trouble to write is worth
    /// surfacing verbatim rather than reducing to a status code.
    fn check(&self, resp: Response, path: &str) -> Result<Response> {
        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }
        let body = resp.text().unwrap_or_default();
        let server_message = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v["message"].as_str().map(str::to_string));

        Err(match status.as_u16() {
            401 if self.api_key.is_none() => miette!(
                "{path} requires authentication and no credential was given.\n\
                 Pass --api-key croniq_… or set CRONIQ_API_KEY."
            ),
            401 => miette!(
                "{path} rejected the credential (401). The key may have been revoked, or its \
                 rotation grace window may have elapsed — check `croniq api-keys list`."
            ),
            403 => miette!(
                "{path} refused the credential's scopes (403).{}",
                server_message
                    .as_deref()
                    .map(|m| format!(" {m}"))
                    .unwrap_or_default()
            ),
            _ => match server_message {
                Some(m) => miette!("{path} failed ({status}): {m}"),
                None if body.trim().is_empty() => miette!("{path} failed ({status})"),
                None => miette!("{path} failed ({status}): {}", body.trim()),
            },
        })
    }

    pub fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let resp = self.send(self.client.get(format!("{}{path}", self.url)), path)?;
        resp.json().into_diagnostic()
    }

    pub fn post_json<B: Serialize, T: DeserializeOwned>(&self, path: &str, body: &B) -> Result<T> {
        let resp = self.send(
            self.client.post(format!("{}{path}", self.url)).json(body),
            path,
        )?;
        resp.json().into_diagnostic()
    }

    pub fn put_json<B: Serialize, T: DeserializeOwned>(&self, path: &str, body: &B) -> Result<T> {
        let resp = self.send(
            self.client.put(format!("{}{path}", self.url)).json(body),
            path,
        )?;
        resp.json().into_diagnostic()
    }

    /// For endpoints answering `204 No Content`.
    pub fn delete(&self, path: &str) -> Result<()> {
        self.send(self.client.delete(format!("{}{path}", self.url)), path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(key: Option<&str>) -> Remote {
        Remote::new("http://localhost:4000/", key.map(str::to_string))
    }

    #[test]
    fn trailing_slash_does_not_double_up() {
        assert_eq!(remote(None).url, "http://localhost:4000");
    }

    #[test]
    fn an_unauthenticated_401_names_the_flag_to_use() {
        // The common first encounter: someone runs `croniq trigger` against a
        // server with auth on. Telling them the status code alone leaves them
        // to guess that a credential is even supported.
        let err = remote(None)
            .check(unauth_response(), "/v1/trigger")
            .unwrap_err()
            .to_string();
        assert!(err.contains("--api-key"), "{err}");
        assert!(err.contains("CRONIQ_API_KEY"), "{err}");
    }

    #[test]
    fn a_401_with_a_credential_points_at_revocation_and_expiry() {
        // Different problem, different remedy: the key was sent and refused,
        // which after #472 most often means it was revoked or its grace
        // window elapsed.
        let err = remote(Some("croniq_x"))
            .check(unauth_response(), "/v1/trigger")
            .unwrap_err()
            .to_string();
        assert!(err.contains("api-keys list"), "{err}");
        assert!(!err.contains("--api-key croniq_"), "{err}");
    }

    #[test]
    fn a_server_message_survives_verbatim() {
        // The 409 from an env-managed client says which variable owns it —
        // reducing that to "conflict" would throw away the only actionable
        // part of the response (issue #471).
        let resp = http_response(
            409,
            r#"{"error":"env_managed","message":"declared by CRONIQ_API_CLIENT_RUNNER_KEY"}"#,
        );
        let err = remote(Some("croniq_x"))
            .check(resp, "/v1/api-clients/runner")
            .unwrap_err()
            .to_string();
        assert!(err.contains("CRONIQ_API_CLIENT_RUNNER_KEY"), "{err}");
    }

    #[test]
    fn a_bodyless_failure_still_reports_the_status() {
        let err = remote(None)
            .check(http_response(500, ""), "/v1/runners")
            .unwrap_err()
            .to_string();
        assert!(err.contains("500"), "{err}");
    }

    fn unauth_response() -> Response {
        http_response(401, "")
    }

    fn http_response(status: u16, body: &str) -> Response {
        Response::from(
            http::Response::builder()
                .status(status)
                .body(body.to_string())
                .unwrap(),
        )
    }
}

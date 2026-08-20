//! HTTP client for the Croniq server API.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("server error: {status} — {body}")]
    Server { status: u16, body: String },

    /// The server returned `409 Conflict` from the poll endpoint —
    /// another runner process is already registered under the same
    /// `runner_id`. Reported separately so the runner loop can count
    /// consecutive occurrences and bail with a clear diagnostic
    /// instead of masking an operator misconfiguration as a transient
    /// error (see issue #134 sub-item 1).
    #[error(
        "poll instance conflict — another runner is already registered with this runner_id: {body}"
    )]
    PollInstanceConflict { body: String },

    /// The server returned `403 Forbidden` from a work endpoint — the
    /// authenticated credential is bound to a *different* `runner_id`
    /// than the one this request named (issue #436). Unlike a 5xx this
    /// is **permanent**: retrying cannot clear it. An operator has to
    /// give the runner its own `runner_id` or release the stale binding
    /// with `DELETE /v1/runners/{id}`.
    #[error(
        "work ownership denied on {endpoint} — this credential does not own the runner_id it \
         named. Give the runner its own runner_id, or release the existing binding with \
         DELETE /v1/runners/{{id}}: {body}"
    )]
    WorkOwnershipDenied {
        endpoint: &'static str,
        body: String,
    },

    /// The server returned `401 Unauthorized` — the API key was rejected.
    ///
    /// Lifted out of [`ClientError::Server`] because retrying cannot clear
    /// it: the credential is read once at construction and never re-read, so
    /// every subsequent request presents the same rejected key (issue #473).
    /// Not fatal on the first occurrence the way a `403` is — key rotation
    /// hands over through an expiry window (issue #471), and a narrow race
    /// around that handover should not take a healthy runner down — but a
    /// streak of them is a credential that is simply gone.
    #[error(
        "unauthorized on {endpoint} — the API key was rejected. It may have been revoked, \
         or its rotation grace window may have elapsed. Restart the runner with the \
         current key: {body}"
    )]
    Unauthorized {
        endpoint: &'static str,
        body: String,
    },
}

/// Map a non-2xx response from a work endpoint to a [`ClientError`].
///
/// `403` and `401` are lifted out of the generic [`ClientError::Server`]
/// bucket so callers can tell "an operator must intervene" from "transient".
/// The two differ in how fast the run loop gives up: a `403` is fatal at
/// once, a `401` is budgeted (see `runner::update_auth_streak`). Every other
/// status stays transient.
fn work_endpoint_error(endpoint: &'static str, status: u16, body: String) -> ClientError {
    match status {
        403 => ClientError::WorkOwnershipDenied { endpoint, body },
        401 => ClientError::Unauthorized { endpoint, body },
        _ => ClientError::Server { status, body },
    }
}

/// Low-level HTTP client for Croniq API endpoints.
pub struct CroniqClient {
    http: Client,
    base_url: String,
    auth_header: Option<String>,
}

#[derive(Serialize)]
pub struct PollRequest {
    pub runner_id: String,
    pub capabilities: Vec<String>,
    pub max_inflight: u32,
    pub inflight: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

#[derive(Deserialize)]
pub struct PollResponse {
    pub work: Vec<WorkAssignment>,
    #[serde(default)]
    pub cancel: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct WorkAssignment {
    pub execution_id: String,
    pub job_key: String,
    pub fire_at: String,
    /// Original logical fire time (RFC 3339). `None` when the server predates
    /// the field — the SDK must not fall back to `fire_at`.
    #[serde(default)]
    pub scheduled_for: Option<String>,
    pub attempt: u32,
    pub metadata: serde_json::Value,
    pub timeout: String,
}

#[derive(Serialize)]
pub struct AckRequest {
    pub runner_id: String,
    pub execution_id: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
    pub attempt: u32,
}

#[derive(Serialize)]
pub struct RenewRequest {
    pub runner_id: String,
    pub execution_id: String,
}

#[derive(Clone, Serialize)]
pub struct WorkEvent {
    pub level: Option<String>,
    pub message: String,
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub fields: std::collections::HashMap<String, String>,
}

impl CroniqClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            http: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            auth_header: None,
        }
    }

    pub fn with_api_key(mut self, key: &str) -> Self {
        self.auth_header = Some(format!("ApiKey {key}"));
        self
    }

    pub fn with_bearer(mut self, token: &str) -> Self {
        self.auth_header = Some(format!("Bearer {token}"));
        self
    }

    fn add_auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(ref auth) = self.auth_header {
            req.header("authorization", auth)
        } else {
            req
        }
    }

    /// Poll for work with long-poll support.
    pub async fn poll(&self, req: &PollRequest) -> Result<PollResponse, ClientError> {
        let resp = self
            .add_auth(self.http.post(format!("{}/v1/work/poll", self.base_url)))
            .json(req)
            .timeout(std::time::Duration::from_secs(35))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            // 409 on the poll endpoint specifically means another
            // runner already holds this runner_id — surface it as a
            // dedicated variant so the runner loop can distinguish
            // "transient" from "operator must intervene".
            if status == 409 {
                return Err(ClientError::PollInstanceConflict { body });
            }
            return Err(work_endpoint_error("/v1/work/poll", status, body));
        }

        Ok(resp.json().await?)
    }

    /// Acknowledge execution completion.
    pub async fn ack(&self, req: &AckRequest) -> Result<(), ClientError> {
        let resp = self
            .add_auth(self.http.post(format!("{}/v1/work/ack", self.base_url)))
            .json(req)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(work_endpoint_error("/v1/work/ack", status, body));
        }

        Ok(())
    }

    /// Renew a work item lease.
    ///
    /// Since #447 the endpoint is a real per-execution lease: `404` means
    /// the execution is no longer leased by this runner and `409` means
    /// it has already reached a terminal state — both routine when a
    /// renew races the runner's own completion. `403` is the ownership
    /// refusal and surfaces as [`ClientError::WorkOwnershipDenied`].
    pub async fn renew(&self, req: &RenewRequest) -> Result<(), ClientError> {
        let resp = self
            .add_auth(self.http.post(format!("{}/v1/work/renew", self.base_url)))
            .json(req)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(work_endpoint_error("/v1/work/renew", status, body));
        }

        Ok(())
    }

    /// Push structured log events for an execution.
    pub async fn push_events(
        &self,
        execution_id: &str,
        events: &[WorkEvent],
    ) -> Result<(), ClientError> {
        let resp = self
            .add_auth(
                self.http
                    .post(format!("{}/v1/work/{}/events", self.base_url, execution_id)),
            )
            .json(events)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(work_endpoint_error("/v1/work/{id}/events", status, body));
        }

        Ok(())
    }

    /// Register a job on the server (runner self-registration).
    pub async fn register_job(&self, req: &RegisterJobRequest) -> Result<(), ClientError> {
        let resp = self
            .add_auth(
                self.http
                    .post(format!("{}/v1/jobs/register", self.base_url)),
            )
            .json(req)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(ClientError::Server { status, body });
        }

        Ok(())
    }
}

#[derive(Serialize)]
pub struct RegisterJobRequest {
    pub job_key: String,
    pub schedule: String,
    pub timezone: Option<String>,
    pub timeout: Option<String>,
    pub runner_id: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub description: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_assignment_parses_scheduled_for() {
        let json = serde_json::json!({
            "execution_id": "e1",
            "job_key": "billing:report",
            "fire_at": "2026-06-08T00:05:00Z",
            "scheduled_for": "2026-06-01T06:00:00Z",
            "attempt": 3,
            "metadata": {},
            "timeout": "15m"
        });
        let wa: WorkAssignment = serde_json::from_value(json).unwrap();
        assert_eq!(wa.scheduled_for.as_deref(), Some("2026-06-01T06:00:00Z"));
    }

    #[test]
    fn work_assignment_scheduled_for_absent_is_none() {
        // A poll response from an older server that never emits the field.
        let json = serde_json::json!({
            "execution_id": "e1",
            "job_key": "billing:report",
            "fire_at": "2026-06-08T00:05:00Z",
            "attempt": 1,
            "metadata": {},
            "timeout": "5m"
        });
        let wa: WorkAssignment = serde_json::from_value(json).unwrap();
        assert!(wa.scheduled_for.is_none());
    }

    #[test]
    fn work_endpoint_error_lifts_403_out_of_the_transient_bucket() {
        // 403 is the ownership refusal from #436 — permanent, so it must
        // not be mistaken for a retryable server error.
        let err = work_endpoint_error("/v1/work/poll", 403, "forbidden".into());
        assert!(matches!(
            err,
            ClientError::WorkOwnershipDenied {
                endpoint: "/v1/work/poll",
                ..
            }
        ));
        let rendered = err.to_string();
        assert!(rendered.contains("DELETE /v1/runners/{id}"), "{rendered}");
    }

    #[test]
    fn work_endpoint_error_lifts_401_out_of_the_transient_bucket() {
        // The SDK reads its key once and never re-reads it, so a rejected
        // credential cannot fix itself — treating 401 as retryable is what
        // left runners spinning forever (issue #473).
        let err = work_endpoint_error("/v1/work/poll", 401, "unauthorized".into());
        assert!(matches!(
            err,
            ClientError::Unauthorized {
                endpoint: "/v1/work/poll",
                ..
            }
        ));
        let rendered = err.to_string();
        assert!(rendered.contains("revoked"), "{rendered}");
        assert!(rendered.contains("Restart the runner"), "{rendered}");
    }

    #[test]
    fn work_endpoint_error_keeps_other_statuses_transient() {
        for status in [404, 409, 500, 503] {
            let err = work_endpoint_error("/v1/work/renew", status, String::new());
            assert!(
                matches!(err, ClientError::Server { status: s, .. } if s == status),
                "status {status} must stay transient"
            );
        }
    }
}

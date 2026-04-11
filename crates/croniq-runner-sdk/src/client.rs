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

#[derive(Serialize)]
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
            return Err(ClientError::Server { status, body });
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
            return Err(ClientError::Server { status, body });
        }

        Ok(())
    }

    /// Renew a work item lease.
    pub async fn renew(&self, req: &RenewRequest) -> Result<(), ClientError> {
        let resp = self
            .add_auth(self.http.post(format!("{}/v1/work/renew", self.base_url)))
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

    /// Push structured log events for an execution.
    pub async fn push_events(
        &self,
        execution_id: &str,
        events: &[WorkEvent],
    ) -> Result<(), ClientError> {
        let resp = self
            .add_auth(
                self.http.post(format!(
                    "{}/v1/work/{}/events",
                    self.base_url, execution_id
                )),
            )
            .json(events)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(ClientError::Server { status, body });
        }

        Ok(())
    }

    /// Register a job on the server (runner self-registration).
    pub async fn register_job(&self, req: &RegisterJobRequest) -> Result<(), ClientError> {
        let resp = self
            .add_auth(self.http.post(format!("{}/v1/jobs/register", self.base_url)))
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

//! Producer-side trigger client — fire jobs on demand via `POST /v1/trigger`.
//!
//! The runner (consumer) side of the protocol — [`CroniqRunner`](crate::CroniqRunner)
//! — polls for work and executes handlers. The *producer* side fires a job on
//! demand, so the *same* registered handler can serve both a periodic schedule
//! and near-real-time, event-driven execution without a second execution or
//! observability path.
//!
//! This client is a first-class wrapper for that endpoint, at parity with the
//! .NET SDK's `ICroniqTriggerClient`. It is deliberately **independent of the
//! runner**: triggering requires the `jobs:trigger` (or `admin`) scope, which
//! runner poll keys typically do not carry, so the trigger client accepts its
//! own API key / bearer token instead of reusing the runner's credentials.
//!
//! # Quick start
//!
//! ```rust,no_run
//! use croniq_runner_sdk::TriggerClient;
//!
//! # async fn demo() -> Result<(), Box<dyn std::error::Error>> {
//! let client = TriggerClient::builder("http://localhost:4000")
//!     .api_key("croniq_trigger_key")
//!     .build();
//!
//! let result = client.trigger("billing:invoice-generate").send().await?;
//! println!(
//!     "execution {} (queued={}, deduplicated={})",
//!     result.execution_id, result.queued, result.deduplicated,
//! );
//! # Ok(())
//! # }
//! ```
//!
//! # Full request
//!
//! Optional fields are attached fluently and omitted from the JSON body when
//! unset (never sent as `null`):
//!
//! ```rust,no_run
//! # use croniq_runner_sdk::TriggerClient;
//! use std::collections::HashMap;
//!
//! # async fn demo(client: TriggerClient) -> Result<(), Box<dyn std::error::Error>> {
//! let result = client
//!     .trigger("reports:nightly")
//!     .metadata(HashMap::from([("tenant".into(), "acme".into())]))
//!     .require(vec!["gpu".into()])
//!     .prefer(vec!["eu-west".into()])
//!     .timeout("15m")
//!     .idempotency_key("evt-2026-07-14-001")
//!     .send()
//!     .await?;
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Default per-request timeout for trigger calls, mirroring the .NET SDK's
/// `CroniqClientOptions.RequestTimeout`.
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Errors surfaced by [`TriggerClient`].
#[derive(Debug, Error)]
pub enum TriggerError {
    /// `job_key` was empty or whitespace-only. Rejected client-side before any
    /// request is sent (mirrors the .NET client's `ArgumentException`).
    #[error("job_key must not be empty")]
    EmptyJobKey,

    /// Transport, timeout, or response-decoding failure from the HTTP layer.
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// The server rejected the trigger with `429 Too Many Requests`: the job's
    /// queued executions are at its `max_queue_depth` cap (default 10), so
    /// accepting more would bypass the same per-job backpressure the scheduler
    /// enforces (issue #299). A dedicated variant — distinct from
    /// [`TriggerError::Server`] — so a producer batching or retrying triggers
    /// can observe the backpressure and slow down rather than pile queued work
    /// up unbounded.
    #[error("queue overflow (429): job is at its max_queue_depth cap — {body}")]
    QueueOverflow {
        /// Raw response body, if any.
        body: String,
    },

    /// The server returned a non-success status other than `429`.
    #[error("server error: {status} — {body}")]
    Server {
        /// HTTP status code.
        status: u16,
        /// Raw response body, if any.
        body: String,
    },
}

/// Result of an on-demand job trigger (`POST /v1/trigger`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TriggerResult {
    /// Identifier of the execution the trigger resolved to. On a dedup hit
    /// (see [`deduplicated`](TriggerResult::deduplicated)) this is the
    /// *existing* execution's id, not a new one.
    pub execution_id: String,

    /// Server work-queue depth after the trigger was processed. Unchanged on a
    /// dedup hit (nothing is enqueued then).
    #[serde(default)]
    pub queued: i64,

    /// `true` when the server coalesced this trigger onto an existing execution
    /// because the request carried an `idempotency_key` it had already seen
    /// (issue #279). Always `false` on servers without idempotency-key support
    /// — they omit the field and it defaults to `false` here.
    #[serde(default)]
    pub deduplicated: bool,
}

/// Wire body for `POST /v1/trigger`. All optional fields skip serialization
/// when unset so they are omitted from the JSON body rather than sent as
/// `null`.
#[derive(Debug, Clone, Serialize)]
struct TriggerRequest {
    job_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<HashMap<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    require: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prefer: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timeout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    idempotency_key: Option<String>,
}

/// Producer-side client for firing Croniq jobs on demand.
///
/// Construct via [`TriggerClient::builder`]. Cheap to clone — clones share the
/// underlying connection pool. See the [module docs](crate::trigger) for usage.
#[derive(Debug, Clone)]
pub struct TriggerClient {
    http: Client,
    base_url: String,
    auth_header: Option<String>,
    request_timeout: Duration,
}

impl TriggerClient {
    /// Start building a trigger client for the given Croniq server base URL
    /// (e.g. `http://localhost:4000`).
    pub fn builder(server_url: &str) -> TriggerClientBuilder {
        TriggerClientBuilder {
            server_url: server_url.to_string(),
            api_key: None,
            bearer_token: None,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
        }
    }

    /// Begin a trigger for `job_key` (e.g. `billing:invoice-generate`). Attach
    /// optional fields fluently, then [`send`](TriggerRequestBuilder::send):
    ///
    /// ```rust,no_run
    /// # use croniq_runner_sdk::TriggerClient;
    /// # async fn demo(client: TriggerClient) -> Result<(), Box<dyn std::error::Error>> {
    /// let result = client.trigger("etl:data-sync").timeout("30s").send().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn trigger(&self, job_key: impl Into<String>) -> TriggerRequestBuilder<'_> {
        TriggerRequestBuilder {
            client: self,
            body: TriggerRequest {
                job_key: job_key.into(),
                metadata: None,
                require: None,
                prefer: None,
                timeout: None,
                idempotency_key: None,
            },
        }
    }
}

/// Builder for a [`TriggerClient`]. See [`TriggerClient::builder`].
pub struct TriggerClientBuilder {
    server_url: String,
    api_key: Option<String>,
    bearer_token: Option<String>,
    request_timeout: Duration,
}

impl TriggerClientBuilder {
    /// API key sent as `Authorization: ApiKey {key}`. Needs the `jobs:trigger`
    /// (or `admin`) scope. Takes precedence over [`bearer_token`] when both are
    /// set, matching the .NET SDK.
    ///
    /// [`bearer_token`]: TriggerClientBuilder::bearer_token
    pub fn api_key(mut self, key: &str) -> Self {
        self.api_key = Some(key.to_string());
        self
    }

    /// Bearer token sent as `Authorization: Bearer {token}`. Ignored when an
    /// [`api_key`](TriggerClientBuilder::api_key) is also set.
    pub fn bearer_token(mut self, token: &str) -> Self {
        self.bearer_token = Some(token.to_string());
        self
    }

    /// Per-request timeout for trigger calls. Default: 30 seconds.
    pub fn request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Build the [`TriggerClient`].
    pub fn build(self) -> TriggerClient {
        // ApiKey wins over Bearer when both are supplied (parity with .NET).
        let auth_header = self
            .api_key
            .map(|k| format!("ApiKey {k}"))
            .or_else(|| self.bearer_token.map(|t| format!("Bearer {t}")));

        TriggerClient {
            http: Client::new(),
            base_url: self.server_url.trim_end_matches('/').to_string(),
            auth_header,
            request_timeout: self.request_timeout,
        }
    }
}

/// A pending trigger request. Attach optional fields, then call
/// [`send`](TriggerRequestBuilder::send). Returned by [`TriggerClient::trigger`].
#[must_use = "a trigger request does nothing until `.send().await` is called"]
pub struct TriggerRequestBuilder<'a> {
    client: &'a TriggerClient,
    body: TriggerRequest,
}

impl TriggerRequestBuilder<'_> {
    /// Metadata forwarded to the handler as a JSON object, merged over the
    /// job's DSL metadata. Values may be any JSON (strings, numbers, bools,
    /// nested objects/arrays). Keys starting with `__` are reserved.
    pub fn metadata(mut self, metadata: HashMap<String, serde_json::Value>) -> Self {
        self.body.metadata = Some(metadata);
        self
    }

    /// Capabilities a runner **must** have to be assigned this execution.
    ///
    /// Leave it unset to inherit the job's `runner { require … }` from the
    /// server-side configuration, so the trigger routes like a scheduled fire;
    /// setting it overrides that for this execution.
    pub fn require(mut self, require: Vec<String>) -> Self {
        self.body.require = Some(require);
        self
    }

    /// Capabilities used to *prefer* runners when several are eligible.
    ///
    /// Unset inherits the job's `runner { prefer … }`; setting it overrides.
    pub fn prefer(mut self, prefer: Vec<String>) -> Self {
        self.body.prefer = Some(prefer);
        self
    }

    /// Execution timeout as a server duration string (e.g. `"30s"`, `"5m"`).
    ///
    /// Leave it unset to inherit the job's configured `timeout`, so a manual
    /// fire is bounded like a scheduled one (issue #551); the server falls
    /// back to `5m` only when the job declares none either. Setting it — to
    /// `"5m"` included — is an explicit override.
    pub fn timeout(mut self, timeout: impl Into<String>) -> Self {
        self.body.timeout = Some(timeout.into());
        self
    }

    /// Optional dedup key (issue #279), scoped per `job_key`. Servers with
    /// trigger-idempotency support coalesce repeat triggers carrying the same
    /// key onto the existing execution (see
    /// [`TriggerResult::deduplicated`]); older servers ignore it.
    pub fn idempotency_key(mut self, key: impl Into<String>) -> Self {
        self.body.idempotency_key = Some(key.into());
        self
    }

    /// Send the trigger. The job's registered handler runs on the next eligible
    /// runner, exactly like a scheduled fire.
    ///
    /// # Errors
    ///
    /// - [`TriggerError::EmptyJobKey`] if `job_key` is blank (no request sent).
    /// - [`TriggerError::QueueOverflow`] on `429` (per-job queue cap, #299).
    /// - [`TriggerError::Server`] on any other non-success status.
    /// - [`TriggerError::Http`] on transport / timeout / decode failure.
    pub async fn send(self) -> Result<TriggerResult, TriggerError> {
        if self.body.job_key.trim().is_empty() {
            return Err(TriggerError::EmptyJobKey);
        }

        let client = self.client;
        let mut req = client
            .http
            .post(format!("{}/v1/trigger", client.base_url))
            .json(&self.body)
            .timeout(client.request_timeout);
        if let Some(ref auth) = client.auth_header {
            req = req.header("authorization", auth);
        }

        let resp = req.send().await?;
        let status = resp.status();
        if !status.is_success() {
            let code = status.as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(if code == 429 {
                TriggerError::QueueOverflow { body }
            } else {
                TriggerError::Server { status: code, body }
            });
        }

        Ok(resp.json::<TriggerResult>().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn omits_unset_optional_fields() {
        let client = TriggerClient::builder("http://example.test:4000").build();
        let req = client.trigger("etl:data-sync");
        let body = serde_json::to_value(&req.body).unwrap();

        assert_eq!(body["job_key"], "etl:data-sync");
        let obj = body.as_object().unwrap();
        assert!(!obj.contains_key("metadata"));
        assert!(!obj.contains_key("require"));
        assert!(!obj.contains_key("prefer"));
        assert!(!obj.contains_key("timeout"));
        assert!(!obj.contains_key("idempotency_key"));
    }

    #[test]
    fn serializes_full_request_snake_case() {
        let client = TriggerClient::builder("http://example.test:4000").build();
        let req = client
            .trigger("reports:nightly")
            .metadata(HashMap::from([
                ("tenant".into(), json!("acme")),
                ("attempt".into(), json!(2)),
                ("flags".into(), json!({ "urgent": true })),
            ]))
            .require(vec!["gpu".into()])
            .prefer(vec!["eu-west".into()])
            .timeout("15m")
            .idempotency_key("evt-1");
        let body = serde_json::to_value(&req.body).unwrap();

        assert_eq!(body["job_key"], "reports:nightly");
        assert_eq!(body["metadata"]["tenant"], "acme");
        // Typed/nested metadata must survive as JSON types, not stringified.
        assert_eq!(body["metadata"]["attempt"], 2);
        assert_eq!(body["metadata"]["flags"]["urgent"], true);
        assert_eq!(body["require"][0], "gpu");
        assert_eq!(body["prefer"][0], "eu-west");
        assert_eq!(body["timeout"], "15m");
        assert_eq!(body["idempotency_key"], "evt-1");
    }

    #[tokio::test]
    async fn empty_job_key_is_rejected_without_a_request() {
        let client = TriggerClient::builder("http://127.0.0.1:1").build();
        let err = client.trigger("   ").send().await.unwrap_err();
        assert!(matches!(err, TriggerError::EmptyJobKey));
    }

    #[test]
    fn api_key_takes_precedence_over_bearer() {
        let client = TriggerClient::builder("http://example.test")
            .bearer_token("tok")
            .api_key("key")
            .build();
        assert_eq!(client.auth_header.as_deref(), Some("ApiKey key"));
    }

    #[test]
    fn bearer_used_when_no_api_key() {
        let client = TriggerClient::builder("http://example.test")
            .bearer_token("tok")
            .build();
        assert_eq!(client.auth_header.as_deref(), Some("Bearer tok"));
    }

    #[test]
    fn no_auth_header_when_unconfigured() {
        let client = TriggerClient::builder("http://example.test").build();
        assert_eq!(client.auth_header, None);
    }

    #[test]
    fn builder_defaults_and_base_url_trim() {
        let client = TriggerClient::builder("http://example.test:4000/").build();
        assert_eq!(client.base_url, "http://example.test:4000");
        assert_eq!(client.request_timeout, DEFAULT_REQUEST_TIMEOUT);
    }

    #[test]
    fn missing_deduplicated_flag_parses_as_false() {
        let result: TriggerResult =
            serde_json::from_str(r#"{"execution_id":"exec-1","queued":0}"#).unwrap();
        assert!(!result.deduplicated);
        assert_eq!(result.execution_id, "exec-1");
    }

    #[test]
    fn deduplicated_flag_is_parsed() {
        let result: TriggerResult =
            serde_json::from_str(r#"{"execution_id":"exec-1","queued":4,"deduplicated":true}"#)
                .unwrap();
        assert!(result.deduplicated);
        assert_eq!(result.queued, 4);
    }

    // ── Offline HTTP round-trip tests ────────────────────────────────────
    //
    // The shared trigger-conformance suite (tests/trigger_conformance.rs) also
    // drives the wire path, but it no-ops unless the language-agnostic
    // `sdks/conformance/cases-trigger/*.yaml` cases are present, so it gives no
    // coverage on this crate in isolation. These tests pin the status → error
    // mapping (200 success / 429 → QueueOverflow / other non-2xx → Server) with
    // a minimal scripted TcpListener — no `wiremock`/`httpmock` dep, matching
    // the hand-rolled mock the conformance harness already uses.

    /// One-shot HTTP server: replies to the first request with `status`/`body`,
    /// then returns its base URL. Drains the request headers first so reqwest
    /// finishes writing before it sees the response.
    async fn spawn_once(status: u16, reason: &'static str, body: &'static str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                let mut buf = Vec::new();
                let mut tmp = [0u8; 1024];
                loop {
                    match sock.read(&mut tmp).await {
                        Ok(0) => break,
                        Ok(n) => {
                            buf.extend_from_slice(&tmp[..n]);
                            if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let resp = format!(
                    "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len(),
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.flush().await;
            }
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn send_returns_result_on_success() {
        let url = spawn_once(
            200,
            "OK",
            r#"{"execution_id":"exec-9","queued":3,"deduplicated":true}"#,
        )
        .await;
        let client = TriggerClient::builder(&url).build();
        let result = client.trigger("billing:invoice").send().await.unwrap();
        assert_eq!(result.execution_id, "exec-9");
        assert_eq!(result.queued, 3);
        assert!(result.deduplicated);
    }

    #[tokio::test]
    async fn send_maps_429_to_queue_overflow() {
        let url = spawn_once(429, "Too Many Requests", "job at max_queue_depth").await;
        let client = TriggerClient::builder(&url).build();
        let err = client.trigger("billing:invoice").send().await.unwrap_err();
        match err {
            TriggerError::QueueOverflow { body } => assert!(body.contains("max_queue_depth")),
            other => panic!("expected QueueOverflow, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn send_maps_other_non_success_to_server_error() {
        let url = spawn_once(500, "Internal Server Error", "boom").await;
        let client = TriggerClient::builder(&url).build();
        let err = client.trigger("billing:invoice").send().await.unwrap_err();
        match err {
            TriggerError::Server { status, body } => {
                assert_eq!(status, 500);
                assert!(body.contains("boom"));
            }
            other => panic!("expected Server, got {other:?}"),
        }
    }
}

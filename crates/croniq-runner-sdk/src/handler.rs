//! Handler dispatch and execution context.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};

use crate::client::{ClientError, CroniqClient, WorkEvent};
use crate::enrichment::{enrich_event, serialize_tags};
use crate::log_writer::{HttpPusher, LogWriter, LogWriterInner};

/// Context passed to job handlers during execution.
#[derive(Clone)]
pub struct ExecutionContext {
    pub(crate) client: Arc<CroniqClient>,
    /// Lazily-spawned streaming log writer. Initialised on the first
    /// `log_writer()` call so handlers that don't need streaming pay no
    /// cost. Cloned across every `ExecutionContext` clone so all writer
    /// handles for one execution share one flusher task. See issue #115.
    pub(crate) log_writer_slot: Arc<OnceLock<Arc<LogWriterInner>>>,
    pub execution_id: String,
    pub job_key: String,
    /// The trigger's original logical fire time — stable across retries and
    /// dead-letter replays. Use this (not wall-clock now) for time-relative
    /// job logic like "the month being reported". `None` when the server
    /// predates the field; the SDK never falls back to the queue fire time.
    pub scheduled_for: Option<chrono::DateTime<chrono::Utc>>,
    pub attempt: u32,
    pub metadata: serde_json::Value,
    pub timeout: String,
    pub runner_id: String,
    pub runner_tags: Vec<String>,
}

impl std::fmt::Debug for ExecutionContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecutionContext")
            .field("execution_id", &self.execution_id)
            .field("job_key", &self.job_key)
            .field("scheduled_for", &self.scheduled_for)
            .field("attempt", &self.attempt)
            .field("timeout", &self.timeout)
            .field("runner_id", &self.runner_id)
            .field("runner_tags", &self.runner_tags)
            .finish()
    }
}

impl ExecutionContext {
    /// Push structured log events for this execution.
    ///
    /// Three fields are auto-injected into every event's `fields` so log
    /// queries can filter without the call site threading values through:
    ///
    /// - `job_key` — the job that produced the event
    /// - `runner_id` — which runner instance handled it
    /// - `runner_tags` — JSON array of the runner's self-declared tags
    ///   (`["env=prod","team=ops"]`); skipped when the runner has no tags
    ///
    /// Existing keys in the caller's event are preserved — auto-injection
    /// uses `entry().or_insert_with(...)` so explicit values win.
    ///
    /// This call **awaits the HTTP POST inline**. For high-volume,
    /// long-running jobs that stream stdout/stderr line by line, prefer
    /// [`ExecutionContext::log_writer`] which buffers events and posts
    /// them asynchronously, avoiding the backpressure-vs-end-of-run
    /// trade-off documented in issue #115.
    pub async fn push_log_events(&self, events: &[WorkEvent]) -> Result<(), ClientError> {
        if events.is_empty() {
            return Ok(());
        }
        let serialized_tags = serialize_tags(&self.runner_tags);
        let enriched: Vec<WorkEvent> = events
            .iter()
            .map(|e| {
                enrich_event(
                    e,
                    &self.job_key,
                    &self.runner_id,
                    serialized_tags.as_deref(),
                )
            })
            .collect();
        self.client.push_events(&self.execution_id, &enriched).await
    }

    /// Push a single log line. Errors are swallowed with a `tracing::warn` so
    /// callers don't need to handle the Result for fire-and-forget logging.
    ///
    /// Like [`push_log_events`](Self::push_log_events), this awaits the HTTP
    /// POST inline. For high-volume scenarios, see
    /// [`ExecutionContext::log_writer`].
    pub async fn log(&self, level: &str, message: impl Into<String>) {
        let event = WorkEvent {
            level: Some(level.into()),
            message: message.into(),
            fields: Default::default(),
        };
        if let Err(e) = self.push_log_events(&[event]).await {
            tracing::warn!(
                execution_id = %self.execution_id,
                error = %e,
                "failed to push log event"
            );
        }
    }

    /// Return a streaming log writer for this execution (issue #115).
    ///
    /// The writer enqueues events into a bounded channel; a background
    /// task batches and POSTs them to the server. `send()` only suspends
    /// on channel capacity, never on HTTP, so a long-running subprocess's
    /// stdout reader will not deadlock when the server is slow.
    ///
    /// The first call spawns the flusher task. Subsequent calls (and
    /// clones of the returned [`LogWriter`]) share that single task. The
    /// runner awaits the writer's drain (up to 5s) before sending the
    /// `ack` for this execution, so all queued events are server-side by
    /// the time the execution is marked complete.
    ///
    /// # Mixing with `log()` / `push_log_events()`
    ///
    /// You may mix paths within one handler, but the server may receive
    /// events out-of-order relative to client-side issue order because
    /// timestamps are assigned on receipt. For strict ordering pick one
    /// path per handler.
    pub fn log_writer(&self) -> LogWriter {
        let inner = self.log_writer_slot.get_or_init(|| {
            // Explicit Arc → Arc<dyn Trait> coercion via let-binding —
            // `Arc::clone` cannot infer the unsizing on its own.
            let client = Arc::clone(&self.client);
            let pusher: Arc<dyn HttpPusher> = client;
            Arc::new(LogWriterInner::spawn(
                pusher,
                self.execution_id.clone(),
                self.job_key.clone(),
                self.runner_id.clone(),
                self.runner_tags.clone(),
            ))
        });
        inner.handle()
    }
}

/// A boxed async handler function.
pub type HandlerFn = Arc<
    dyn Fn(ExecutionContext) -> Pin<Box<dyn Future<Output = Result<(), HandlerError>> + Send>>
        + Send
        + Sync,
>;

#[derive(Debug, thiserror::Error)]
pub enum HandlerError {
    #[error("{0}")]
    Failed(String),
}

impl HandlerError {
    pub fn msg(s: impl Into<String>) -> Self {
        Self::Failed(s.into())
    }
}

/// Registry of job handlers keyed by job_key.
pub struct HandlerRegistry {
    handlers: HashMap<String, HandlerFn>,
    default_handler: Option<HandlerFn>,
}

impl Default for HandlerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl HandlerRegistry {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            default_handler: None,
        }
    }

    pub fn register<F, Fut>(&mut self, job_key: &str, handler: F)
    where
        F: Fn(ExecutionContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), HandlerError>> + Send + 'static,
    {
        let handler = Arc::new(move |ctx: ExecutionContext| {
            Box::pin(handler(ctx)) as Pin<Box<dyn Future<Output = _> + Send>>
        });
        self.handlers.insert(job_key.to_string(), handler);
    }

    pub fn set_default<F, Fut>(&mut self, handler: F)
    where
        F: Fn(ExecutionContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), HandlerError>> + Send + 'static,
    {
        self.default_handler = Some(Arc::new(move |ctx: ExecutionContext| {
            Box::pin(handler(ctx)) as Pin<Box<dyn Future<Output = _> + Send>>
        }));
    }

    pub fn get(&self, job_key: &str) -> Option<&HandlerFn> {
        self.handlers.get(job_key).or(self.default_handler.as_ref())
    }

    pub fn job_keys(&self) -> Vec<String> {
        self.handlers.keys().cloned().collect()
    }
}

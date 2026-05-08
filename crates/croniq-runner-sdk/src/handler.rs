//! Handler dispatch and execution context.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::client::{ClientError, CroniqClient, WorkEvent};

/// Context passed to job handlers during execution.
#[derive(Clone)]
pub struct ExecutionContext {
    pub(crate) client: Arc<CroniqClient>,
    pub execution_id: String,
    pub job_key: String,
    pub attempt: u32,
    pub metadata: serde_json::Value,
    pub timeout: String,
}

impl std::fmt::Debug for ExecutionContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecutionContext")
            .field("execution_id", &self.execution_id)
            .field("job_key", &self.job_key)
            .field("attempt", &self.attempt)
            .field("timeout", &self.timeout)
            .finish()
    }
}

impl ExecutionContext {
    /// Push structured log events for this execution.
    ///
    /// `job_key` is automatically injected into every event's `fields` so log
    /// entries are filterable by job even when the raw message doesn't carry it.
    pub async fn push_log_events(&self, events: &[WorkEvent]) -> Result<(), ClientError> {
        if events.is_empty() {
            return Ok(());
        }
        let enriched: Vec<WorkEvent> = events
            .iter()
            .map(|e| {
                let mut fields = e.fields.clone();
                fields
                    .entry("job_key".into())
                    .or_insert_with(|| self.job_key.clone());
                WorkEvent {
                    level: e.level.clone(),
                    message: e.message.clone(),
                    fields,
                }
            })
            .collect();
        self.client.push_events(&self.execution_id, &enriched).await
    }

    /// Push a single log line. Errors are swallowed with a `tracing::warn` so
    /// callers don't need to handle the Result for fire-and-forget logging.
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

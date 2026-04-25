//! Handler dispatch and execution context.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::client::WorkAssignment;

/// Context passed to job handlers during execution.
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    pub execution_id: String,
    pub job_key: String,
    pub attempt: u32,
    pub metadata: serde_json::Value,
    pub timeout: String,
}

impl From<WorkAssignment> for ExecutionContext {
    fn from(w: WorkAssignment) -> Self {
        Self {
            execution_id: w.execution_id,
            job_key: w.job_key,
            attempt: w.attempt,
            metadata: w.metadata,
            timeout: w.timeout,
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

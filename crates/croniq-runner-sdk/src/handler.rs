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
    pub runner_id: String,
    pub runner_tags: Vec<String>,
}

impl std::fmt::Debug for ExecutionContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecutionContext")
            .field("execution_id", &self.execution_id)
            .field("job_key", &self.job_key)
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

/// JSON-encode the runner's tag list for inclusion as a single
/// `runner_tags` log field. Returns `None` when the runner has no tags.
fn serialize_tags(tags: &[String]) -> Option<String> {
    if tags.is_empty() {
        return None;
    }
    serde_json::to_string(tags).ok()
}

/// Auto-inject `job_key`, `runner_id`, and `runner_tags` into a log event's
/// fields without overwriting caller-provided values.
fn enrich_event(
    event: &WorkEvent,
    job_key: &str,
    runner_id: &str,
    serialized_tags: Option<&str>,
) -> WorkEvent {
    let mut fields = event.fields.clone();
    fields
        .entry("job_key".into())
        .or_insert_with(|| job_key.to_string());
    fields
        .entry("runner_id".into())
        .or_insert_with(|| runner_id.to_string());
    if let Some(tags) = serialized_tags {
        fields
            .entry("runner_tags".into())
            .or_insert_with(|| tags.to_string());
    }
    WorkEvent {
        level: event.level.clone(),
        message: event.message.clone(),
        fields,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn event(level: &str, message: &str) -> WorkEvent {
        WorkEvent {
            level: Some(level.into()),
            message: message.into(),
            fields: HashMap::new(),
        }
    }

    #[test]
    fn enrich_event_injects_three_fields_when_tags_present() {
        let tags = serialize_tags(&["env=prod".into(), "team=ops".into()]);
        let enriched = enrich_event(
            &event("info", "hello"),
            "billing:invoice",
            "shell-runner-1",
            tags.as_deref(),
        );
        assert_eq!(enriched.fields.get("job_key").unwrap(), "billing:invoice");
        assert_eq!(enriched.fields.get("runner_id").unwrap(), "shell-runner-1");
        // JSON-array string so downstream log indexers can parse it.
        assert_eq!(
            enriched.fields.get("runner_tags").unwrap(),
            r#"["env=prod","team=ops"]"#
        );
    }

    #[test]
    fn enrich_event_skips_runner_tags_when_runner_has_none() {
        let enriched = enrich_event(
            &event("info", "hello"),
            "billing:invoice",
            "shell-runner-1",
            serialize_tags(&[]).as_deref(),
        );
        assert!(!enriched.fields.contains_key("runner_tags"));
        assert_eq!(enriched.fields.get("runner_id").unwrap(), "shell-runner-1");
    }

    #[test]
    fn enrich_event_does_not_overwrite_caller_provided_fields() {
        let mut e = event("warn", "hi");
        e.fields.insert("job_key".into(), "explicit:value".into());
        e.fields
            .insert("runner_id".into(), "explicit-runner".into());

        let tags = serialize_tags(&["env=prod".into()]);
        let enriched = enrich_event(&e, "auto:job", "auto-runner", tags.as_deref());
        assert_eq!(enriched.fields.get("job_key").unwrap(), "explicit:value");
        assert_eq!(enriched.fields.get("runner_id").unwrap(), "explicit-runner");
        assert_eq!(
            enriched.fields.get("runner_tags").unwrap(),
            r#"["env=prod"]"#
        );
    }

    #[test]
    fn serialize_tags_empty_returns_none() {
        assert!(serialize_tags(&[]).is_none());
    }

    #[test]
    fn serialize_tags_round_trips_as_json_array() {
        let s = serialize_tags(&["a=1".into(), "b=2".into()]).unwrap();
        let back: Vec<String> = serde_json::from_str(&s).unwrap();
        assert_eq!(back, vec!["a=1", "b=2"]);
    }
}

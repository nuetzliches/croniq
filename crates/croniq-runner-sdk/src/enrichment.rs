//! Shared event-enrichment helpers used by both the direct
//! `ExecutionContext::push_log_events` path and the streaming
//! `LogWriter` flusher (issue #115).
//!
//! Three fields are auto-injected into every event's `fields` so log
//! queries can filter without the call site threading values through:
//!
//! - `job_key` — the job that produced the event
//! - `runner_id` — which runner instance handled it
//! - `runner_tags` — JSON array of the runner's self-declared tags
//!   (`["env=prod","team=ops"]`); skipped when the runner has no tags
//!
//! Existing keys in the caller's event are preserved — auto-injection
//! uses `entry().or_insert_with(...)` so explicit values win.

use crate::client::WorkEvent;

/// JSON-encode the runner's tag list for inclusion as a single
/// `runner_tags` log field. Returns `None` when the runner has no tags.
pub(crate) fn serialize_tags(tags: &[String]) -> Option<String> {
    if tags.is_empty() {
        return None;
    }
    serde_json::to_string(tags).ok()
}

/// Auto-inject `job_key`, `runner_id`, and `runner_tags` into a log event's
/// fields without overwriting caller-provided values.
pub(crate) fn enrich_event(
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

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

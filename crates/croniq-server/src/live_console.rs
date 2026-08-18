//! In-process pub/sub for server tracing events powering the Live Console
//! (issue #141).
//!
//! `ConsoleHub` is a thin fan-out: a bounded ring buffer of recent events
//! plus a `tokio::sync::broadcast` channel for live subscribers. The
//! [`LiveConsoleLayer`] is a `tracing` `Layer` that funnels every event
//! emitted by the server through the hub in addition to the existing
//! stderr / OTLP sinks.
//!
//! What's intentionally NOT here:
//!
//! - **Persistence.** The ring buffer is in-memory and bounded. Production
//!   long-term log search belongs in OTLP / Loki, not in this hub. The
//!   ring exists so a newly-connected dashboard can backfill the last
//!   ~minute of context before the live tail.
//! - **Per-execution log multiplexing.** That data is served by the
//!   existing `GET /v1/executions/{id}/logs` endpoint and the
//!   `useExecutionLogs` hook. The hub only carries server-side tracing
//!   events. The UI composes the two streams.
//!
//! Redaction: the stream carries the raw server tracing feed, so it is
//! treated as an admin-only surface (`GET /v1/events/stream` requires the
//! `admin` scope). On top of that, [`ConsoleHub::push`] drops events on
//! secret-bearing targets ([`REDACTED_TARGETS`]) and blanks the values of
//! secret-looking fields ([`SECRET_FIELD_MARKERS`]) before they reach a
//! subscriber. That covers structured fields and whole targets; it cannot
//! see a credential interpolated into a `message` on some unrelated
//! target, so "never log secrets" remains the primary rule and this is the
//! backstop.
//!
//! Threading: `ConsoleHub` is `Send + Sync` and cheap to clone (`Arc`).
//! Push from the tracing layer is non-blocking (broadcast `send` returns
//! immediately when there are no subscribers).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde::Serialize;
use tokio::sync::broadcast;
use tracing::{Event, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

/// How many recent events to keep in the ring buffer.
const RING_CAPACITY: usize = 1000;

/// Tracing targets whose events never reach the hub at all.
///
/// These modules legitimately handle single-use credentials (reset /
/// invitation tokens, SMTP details, OIDC exchange payloads). Even when the
/// call sites are careful today, a future `tracing::info!` there would be
/// one field away from broadcasting a credential to every console
/// subscriber — so the whole target is dropped rather than filtered
/// field-by-field. Sub-targets (`croniq::email::smtp`) are covered too.
/// `croniq::invitations` has no call sites yet and is listed pre-emptively —
/// invitation tokens have the same shape as reset tokens, so if that module
/// ever grows a log line it starts out on the safe side.
///
/// These events still reach stderr and OTLP; only the in-process fan-out
/// to the dashboard drops them.
const REDACTED_TARGETS: &[&str] = &[
    "croniq::password_reset",
    "croniq::email",
    "croniq::oidc",
    "croniq::invitations",
];

/// Substrings that mark a structured field name as secret-bearing. Matched
/// case-insensitively against the field name — not the value, which is
/// exactly the point: we do not want to pattern-match credentials, we want
/// to never forward a field that is *named* like one.
///
/// Deliberately narrow. `key` on its own is not here because Croniq logs
/// `job_key` / `idempotency_key` everywhere and those are public
/// identifiers (see AGENTS.md on the CodeQL false-positive class).
const SECRET_FIELD_MARKERS: &[&str] = &[
    "token",
    "secret",
    "password",
    "credential",
    "api_key",
    "apikey",
    "authorization",
    "confirm_url",
    "accept_url",
    "webhook_url",
];

/// What a redacted field's value is replaced with.
const REDACTED_PLACEHOLDER: &str = "[redacted]";

/// True when `target` is one of [`REDACTED_TARGETS`] or a child of one.
fn target_is_redacted(target: &str) -> bool {
    REDACTED_TARGETS.iter().any(|t| {
        target
            .strip_prefix(*t)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with("::"))
    })
}

/// True when a structured field name looks like it carries a credential.
fn field_is_secret(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    SECRET_FIELD_MARKERS.iter().any(|m| lower.contains(m))
}

/// Per-subscriber broadcast lag tolerance. A subscriber that falls more
/// than this many events behind gets a `Lagged` error and we drop the
/// gap silently — far cheaper than blocking the writer or growing the
/// buffer unboundedly.
const BROADCAST_CAPACITY: usize = 256;

/// One server event suitable for the Live Console UI. Field shapes are
/// JSON-friendly and stable enough to consume from the dashboard without
/// schema generation.
#[derive(Clone, Debug, Serialize)]
pub struct ConsoleEvent {
    /// UTC timestamp the event was observed at.
    pub ts: DateTime<Utc>,
    /// Lowercased tracing level: `trace` / `debug` / `info` / `warn` / `error`.
    pub level: String,
    /// Tracing target (`croniq_server::scheduler`, `croniq::audit`, …).
    pub target: String,
    /// The event's `message` field, if any. Empty when the call site
    /// only emitted structured fields.
    pub message: String,
    /// Remaining structured fields as a JSON object. Strings are
    /// emitted verbatim; non-string types are rendered via `Debug`. The
    /// object is empty when the event had no fields besides `message`.
    pub fields: serde_json::Map<String, serde_json::Value>,
}

/// Cheap-to-clone handle to the live console fan-out. Held by
/// `ServerState` and by the `LiveConsoleLayer` registered with the
/// tracing subscriber.
pub struct ConsoleHub {
    tx: broadcast::Sender<ConsoleEvent>,
    ring: Mutex<VecDeque<ConsoleEvent>>,
}

impl ConsoleHub {
    pub fn new() -> Arc<Self> {
        let (tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        Arc::new(Self {
            tx,
            ring: Mutex::new(VecDeque::with_capacity(RING_CAPACITY)),
        })
    }

    /// Subscribe to live events. New subscribers do NOT get backfill —
    /// for that, call [`Self::snapshot`] first and forward those, then
    /// start consuming the receiver. The Live Console SSE handler does
    /// exactly that.
    pub fn subscribe(&self) -> broadcast::Receiver<ConsoleEvent> {
        self.tx.subscribe()
    }

    /// Snapshot of the last `n` events (most recent last). Cheap clone —
    /// each `ConsoleEvent` is small.
    pub fn snapshot(&self, n: usize) -> Vec<ConsoleEvent> {
        let ring = self.ring.lock().unwrap();
        let len = ring.len();
        let start = len.saturating_sub(n);
        ring.iter().skip(start).cloned().collect()
    }

    /// Push one event. Internal — `LiveConsoleLayer` is the only caller.
    ///
    /// This is also the redaction chokepoint: events on a secret-bearing
    /// target are dropped outright and secret-looking fields have their
    /// values replaced before anything lands in the ring buffer or reaches
    /// a subscriber. Doing it here (rather than at the call sites) means a
    /// credential logged somewhere new still never leaves the process via
    /// the console — the last line of defence behind "don't log secrets".
    fn push(&self, mut event: ConsoleEvent) {
        if target_is_redacted(&event.target) {
            return;
        }
        for (name, value) in event.fields.iter_mut() {
            if field_is_secret(name) {
                *value = serde_json::Value::String(REDACTED_PLACEHOLDER.to_string());
            }
        }
        {
            let mut ring = self.ring.lock().unwrap();
            if ring.len() >= RING_CAPACITY {
                ring.pop_front();
            }
            ring.push_back(event.clone());
        }
        // SendError just means "no subscribers right now" — that's fine,
        // the ring buffer still has the event for the next connect.
        let _ = self.tx.send(event);
    }
}

/// Tracing `Layer` that forwards every event to a [`ConsoleHub`].
///
/// The layer is filtered by the global `EnvFilter` installed alongside
/// the stderr `fmt` layer — so `RUST_LOG=debug` shows debug in the
/// Live Console too, and the default `info` filter is what dashboards
/// see by default.
pub struct LiveConsoleLayer {
    hub: Arc<ConsoleHub>,
}

impl LiveConsoleLayer {
    pub fn new(hub: Arc<ConsoleHub>) -> Self {
        Self { hub }
    }
}

impl<S: Subscriber> Layer<S> for LiveConsoleLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        // Bail before visiting fields — a dropped target costs nothing.
        if target_is_redacted(meta.target()) {
            return;
        }

        let mut visitor = EventVisitor::default();
        event.record(&mut visitor);

        let ev = ConsoleEvent {
            ts: Utc::now(),
            level: meta.level().to_string().to_lowercase(),
            target: meta.target().to_string(),
            message: visitor.message,
            fields: visitor.fields,
        };
        self.hub.push(ev);
    }
}

/// Visits a tracing event's fields, extracting `message` separately and
/// folding the rest into a JSON object. The `tracing` field API doesn't
/// expose typed values uniformly, so non-`message` fields are stringified
/// via `Debug` — that matches the stderr `fmt` layer's behaviour and
/// avoids losing information for `i64` / `bool` / structured types.
#[derive(Default)]
struct EventVisitor {
    message: String,
    fields: serde_json::Map<String, serde_json::Value>,
}

impl tracing::field::Visit for EventVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.fields.insert(
                field.name().to_string(),
                serde_json::Value::String(value.to_string()),
            );
        }
    }
    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.fields
            .insert(field.name().to_string(), serde_json::Value::Bool(value));
    }
    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.fields
            .insert(field.name().to_string(), serde_json::Value::from(value));
    }
    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.fields
            .insert(field.name().to_string(), serde_json::Value::from(value));
    }
    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        self.fields
            .insert(field.name().to_string(), serde_json::Value::from(value));
    }
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let rendered = format!("{value:?}");
        if field.name() == "message" {
            self.message = rendered;
        } else {
            self.fields.insert(
                field.name().to_string(),
                serde_json::Value::String(rendered),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing::Level;
    use tracing_subscriber::prelude::*;

    #[test]
    fn snapshot_returns_recent_events_in_order() {
        let hub = ConsoleHub::new();
        for i in 0..5 {
            hub.push(ConsoleEvent {
                ts: Utc::now(),
                level: "info".into(),
                target: "test".into(),
                message: format!("ev-{i}"),
                fields: Default::default(),
            });
        }
        let snap = hub.snapshot(3);
        assert_eq!(snap.len(), 3);
        assert_eq!(snap[0].message, "ev-2");
        assert_eq!(snap[2].message, "ev-4");
    }

    #[test]
    fn ring_buffer_drops_oldest_at_capacity() {
        let hub = ConsoleHub::new();
        for i in 0..(RING_CAPACITY + 50) {
            hub.push(ConsoleEvent {
                ts: Utc::now(),
                level: "info".into(),
                target: "test".into(),
                message: format!("ev-{i}"),
                fields: Default::default(),
            });
        }
        let snap = hub.snapshot(RING_CAPACITY);
        assert_eq!(snap.len(), RING_CAPACITY);
        assert_eq!(snap[0].message, "ev-50");
        assert_eq!(
            snap[RING_CAPACITY - 1].message,
            format!("ev-{}", RING_CAPACITY + 49)
        );
    }

    #[tokio::test]
    async fn subscribers_receive_subsequent_events() {
        let hub = ConsoleHub::new();
        let mut rx = hub.subscribe();
        hub.push(ConsoleEvent {
            ts: Utc::now(),
            level: "warn".into(),
            target: "test".into(),
            message: "hello".into(),
            fields: Default::default(),
        });
        let got = rx.recv().await.unwrap();
        assert_eq!(got.message, "hello");
        assert_eq!(got.level, "warn");
    }

    #[tokio::test]
    async fn layer_captures_tracing_events_into_hub() {
        let hub = ConsoleHub::new();
        let layer = LiveConsoleLayer::new(Arc::clone(&hub));
        let subscriber = tracing_subscriber::registry().with(layer);
        let _guard = tracing::subscriber::set_default(subscriber);

        tracing::event!(Level::INFO, runner_id = "r1", "runner reconnected");
        // Give the layer a tick to land (synchronous, but be safe).
        tokio::task::yield_now().await;

        let snap = hub.snapshot(10);
        let found = snap.iter().find(|e| e.message == "runner reconnected");
        let ev = found.expect("event not captured");
        assert_eq!(ev.level, "info");
        assert_eq!(
            ev.fields.get("runner_id").and_then(|v| v.as_str()),
            Some("r1")
        );
    }

    #[test]
    fn redacted_target_matching_covers_children_not_prefixes() {
        assert!(target_is_redacted("croniq::password_reset"));
        assert!(target_is_redacted("croniq::email"));
        assert!(target_is_redacted("croniq::email::smtp"));
        assert!(target_is_redacted("croniq::oidc"));
        // Not a child — `::` boundary required.
        assert!(!target_is_redacted("croniq::emails"));
        assert!(!target_is_redacted("croniq::audit"));
        assert!(!target_is_redacted("croniq_server::scheduler"));
    }

    #[test]
    fn secret_field_names_are_detected_public_identifiers_are_not() {
        assert!(field_is_secret("token"));
        assert!(field_is_secret("confirm_url"));
        assert!(field_is_secret("reset_token"));
        assert!(field_is_secret("API_KEY"));
        assert!(field_is_secret("password_hash"));
        assert!(field_is_secret("client_secret"));
        // Public identifiers Croniq logs on purpose must stay visible.
        assert!(!field_is_secret("job_key"));
        assert!(!field_is_secret("runner_id"));
        assert!(!field_is_secret("execution_id"));
        assert!(!field_is_secret("reset_id"));
    }

    #[tokio::test]
    async fn events_on_a_redacted_target_never_reach_the_hub() {
        let hub = ConsoleHub::new();
        let layer = LiveConsoleLayer::new(Arc::clone(&hub));
        let subscriber = tracing_subscriber::registry().with(layer);
        let _guard = tracing::subscriber::set_default(subscriber);

        tracing::event!(
            target: "croniq::password_reset",
            Level::INFO,
            user_id = "u1",
            "password reset issued"
        );
        tracing::event!(target: "croniq::email", Level::INFO, to = "a@b.c", "email skipped");
        tracing::event!(Level::INFO, job_key = "nightly", "kept");
        tokio::task::yield_now().await;

        let snap = hub.snapshot(10);
        assert!(
            !snap
                .iter()
                .any(|e| target_is_redacted(&e.target) || e.message.contains("password reset")),
            "secret-bearing target leaked into the hub: {snap:?}"
        );
        // The non-redacted event on the same subscriber still lands, so
        // this isn't passing because nothing was captured at all.
        assert!(snap.iter().any(|e| e.message == "kept"));
    }

    #[tokio::test]
    async fn secret_looking_fields_are_blanked_before_fan_out() {
        let hub = ConsoleHub::new();
        let layer = LiveConsoleLayer::new(Arc::clone(&hub));
        let subscriber = tracing_subscriber::registry().with(layer);
        let _guard = tracing::subscriber::set_default(subscriber);

        tracing::event!(
            Level::WARN,
            job_key = "nightly",
            token = "croniq_pwr_supersecretvalue",
            confirm_url = "https://host/password-reset/confirm?token=croniq_pwr_supersecretvalue",
            "suspicious call site"
        );
        tokio::task::yield_now().await;

        let snap = hub.snapshot(10);
        let ev = snap
            .iter()
            .find(|e| e.message == "suspicious call site")
            .expect("event not captured");
        assert_eq!(
            ev.fields.get("token").and_then(|v| v.as_str()),
            Some(REDACTED_PLACEHOLDER)
        );
        assert_eq!(
            ev.fields.get("confirm_url").and_then(|v| v.as_str()),
            Some(REDACTED_PLACEHOLDER)
        );
        // Public identifiers on the same event are untouched.
        assert_eq!(
            ev.fields.get("job_key").and_then(|v| v.as_str()),
            Some("nightly")
        );
        let rendered = serde_json::to_string(ev).unwrap();
        assert!(
            !rendered.contains("supersecretvalue"),
            "secret survived serialisation: {rendered}"
        );
    }
}

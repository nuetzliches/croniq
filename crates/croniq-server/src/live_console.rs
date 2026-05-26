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
    fn push(&self, event: ConsoleEvent) {
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
        let mut visitor = EventVisitor::default();
        event.record(&mut visitor);

        let meta = event.metadata();
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
}

//! `GET /v1/events/stream` — Server-Sent Events live tail of the server
//! tracing stream, powering the Live Console (issue #141).
//!
//! Each event is emitted as an `event: log` frame with a JSON-encoded
//! [`crate::live_console::ConsoleEvent`] payload. On connect, the server
//! first replays a bounded snapshot of recent events so a freshly opened
//! console shows context; the live tail then continues from the next
//! event the tracing subscriber observes.
//!
//! Filtering happens server-side for level (cheap) and client-side for
//! free-text search (cheaper than encoding regex semantics in a query
//! param, and dominated by network cost anyway).

use std::collections::HashSet;
use std::convert::Infallible;
use std::sync::Arc;

use axum::{
    Extension,
    extract::{Query, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
};
use croniq_auth::CallerContext;
use croniq_auth::context::Scope;
use futures_core::Stream;
use serde::Deserialize;
use tokio::sync::broadcast::error::RecvError;

use super::ServerState;
use crate::api::audit;
use crate::api::auth_middleware::require_scope;
use crate::live_console::ConsoleEvent;

const SNAPSHOT_DEFAULT: usize = 200;
const SNAPSHOT_MAX: usize = 1000;

#[derive(Debug, Deserialize, Default)]
pub struct EventStreamQuery {
    /// Comma-separated list of levels to include. Empty/unset = all.
    /// Valid values: `trace`, `debug`, `info`, `warn`, `error`.
    #[serde(default)]
    pub levels: Option<String>,
    /// How many backfill events to ship before the live tail starts.
    /// Capped at `SNAPSHOT_MAX`.
    #[serde(default)]
    pub snapshot: Option<usize>,
}

impl EventStreamQuery {
    fn level_set(&self) -> Option<HashSet<String>> {
        self.levels.as_deref().and_then(|raw| {
            let set: HashSet<String> = raw
                .split(',')
                .map(|s| s.trim().to_ascii_lowercase())
                .filter(|s| !s.is_empty())
                .collect();
            if set.is_empty() { None } else { Some(set) }
        })
    }
}

fn level_matches(filter: &Option<HashSet<String>>, event_level: &str) -> bool {
    match filter {
        None => true,
        Some(set) => set.contains(event_level),
    }
}

fn into_sse(event: &ConsoleEvent) -> Event {
    match Event::default().event("log").json_data(event) {
        Ok(e) => e,
        Err(_) => Event::default().event("log").data("{}"),
    }
}

/// `GET /v1/events/stream`
///
/// Scope: `admin`. This is not the per-job execution log — it is the raw
/// server-wide tracing feed (audit lines, auth failures with user ids, job
/// stderr, config diagnostics), plus a replay buffer of everything the
/// server logged recently. That is an operator-of-the-whole-server tool, so
/// it sits behind the admin wildcard rather than a read scope that role
/// defaults hand to viewers. Per-execution logs remain available to
/// `executions:read` via `GET /v1/executions/{id}/logs`.
pub async fn handle_events_stream(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Query(q): Query<EventStreamQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    require_scope(&ctx, Scope::ADMIN)?;

    let hub = state
        .console_hub
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?
        .clone();

    let snapshot_size = q.snapshot.unwrap_or(SNAPSHOT_DEFAULT).min(SNAPSHOT_MAX);
    let level_filter = q.level_set();

    // Audit the open. Best-effort: skip when there's no store (test
    // mode) and don't fail the stream on write errors — the audit::record
    // helper already logs failures internally.
    if let Some(store) = state.store.as_ref() {
        audit::record(store, &ctx, "console.opened", "console", None, None);
    }

    let snapshot = hub.snapshot(snapshot_size);
    let mut rx = hub.subscribe();

    let stream = async_stream::stream! {
        // Replay backfill first, filtered.
        for ev in snapshot {
            if level_matches(&level_filter, &ev.level) {
                yield Ok(into_sse(&ev));
            }
        }
        // Live tail.
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    if level_matches(&level_filter, &ev.level) {
                        yield Ok(into_sse(&ev));
                    }
                }
                // Subscriber fell behind — drop the gap, keep going. The
                // client missed some events but the connection stays
                // alive; for a UI tail that's the right trade-off.
                Err(RecvError::Lagged(_)) => continue,
                // Sender dropped — server is shutting down.
                Err(RecvError::Closed) => break,
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_filter_parses_csv() {
        let q = EventStreamQuery {
            levels: Some("warn, error ,INFO".into()),
            snapshot: None,
        };
        let set = q.level_set().unwrap();
        assert!(set.contains("warn"));
        assert!(set.contains("error"));
        assert!(set.contains("info"));
        assert_eq!(set.len(), 3);
    }

    #[test]
    fn level_filter_empty_means_all() {
        let q = EventStreamQuery {
            levels: Some(" , , ".into()),
            snapshot: None,
        };
        assert!(q.level_set().is_none());
        assert!(level_matches(&None, "info"));
    }

    #[test]
    fn level_matches_honours_set() {
        let set: HashSet<String> = ["warn".to_string(), "error".to_string()]
            .into_iter()
            .collect();
        assert!(level_matches(&Some(set.clone()), "warn"));
        assert!(!level_matches(&Some(set), "info"));
    }
}

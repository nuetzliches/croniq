//! `GET /v1/executions/stream` — the runs of the last few minutes, live.
//!
//! Feeds the dashboard's live timeline: one bar per run, sliding right to
//! left past a fixed "now" line. Its narrowest window is five seconds, and at
//! that scale the 1–3 s a polled list lags behind would put a fresh run on
//! screen a fifth of the way across already. Hence a stream.
//!
//! Push, but not from the state transitions themselves: executions change
//! state in the scheduler, the work protocol, the watchdog, cancel and replay,
//! and threading a change bus through all of them is a larger change than
//! this view justifies. Instead each connection re-reads the window every
//! [`TICK`] and sends a frame only when it differs from the last one — so the
//! latency is one tick, and an idle system costs two indexed queries per tick
//! and no traffic beyond the heartbeat.
//!
//! Every frame carries the server's `now`. The client places bars by server
//! timestamps against its own clock; at a five-second window a one-second
//! skew between the two would visibly misplace every bar, so it measures the
//! offset from this field rather than trusting `Date.now()`.

use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    Extension,
    extract::State,
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
};
use chrono::{DateTime, Utc};
use croniq_auth::CallerContext;
use croniq_auth::context::Scope;
use croniq_store::models::{Execution, ExecutionFilter, ExecutionState};
use futures_core::Stream;
use serde::Serialize;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

use super::ServerState;
use crate::api::auth_middleware::require_scope;
use crate::store::DynStore;

/// How often a connection re-reads the window. A quarter second is 32 px at
/// the five-second window — the most a new bar can appear late.
const TICK: Duration = Duration::from_millis(250);

/// How far back a frame reaches. The widest window the dashboard offers is
/// five minutes; the slack keeps a bar from being dropped while its left end
/// is still sliding out of view.
const HORIZON: chrono::Duration = chrono::Duration::seconds(5 * 60 + 30);

/// A frame is resent unchanged after this long, so the client's clock-offset
/// estimate keeps getting fresh samples while nothing runs.
const HEARTBEAT: Duration = Duration::from_secs(5);

/// Caps on the two reads. A system with more than this many runs in five
/// minutes has outgrown a per-job timeline anyway; the cap keeps a burst from
/// turning every tick into a large query.
const ACTIVE_LIMIT: u32 = 500;
const RECENT_LIMIT: u32 = 2000;

/// One run as the timeline needs it — no metadata, no error text. The error
/// is on the Runs screen, one click away, and leaving it out keeps a frame
/// small enough to resend four times a second.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct LiveRun {
    id: Uuid,
    job_key: String,
    state: ExecutionState,
    attempt: u32,
    runner_id: Option<String>,
    fire_at: DateTime<Utc>,
    claimed_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
}

impl From<&Execution> for LiveRun {
    fn from(e: &Execution) -> Self {
        Self {
            id: e.id,
            job_key: e.job_key.clone(),
            state: e.state,
            attempt: e.attempt,
            runner_id: e.runner_id.clone(),
            fire_at: e.fire_at,
            claimed_at: e.claimed_at,
            completed_at: e.completed_at,
        }
    }
}

#[derive(Serialize)]
struct Frame<'a> {
    now: DateTime<Utc>,
    runs: &'a [LiveRun],
}

fn is_active(state: ExecutionState) -> bool {
    matches!(state, ExecutionState::Queued | ExecutionState::Claimed)
}

/// The window, per connection.
///
/// Two reads cover nearly everything: whatever is queued or running, and
/// whatever was *created* within the horizon. What they miss is a run that
/// started before the horizon and finished inside it — a ten-minute job that
/// ended a minute ago. It belongs on screen (its bar reaches the right edge),
/// but it is neither active any more nor recent by `created_at`. So the
/// window remembers which runs it last saw active, fetches any that dropped
/// out of both reads by id, and keeps them until they age past the horizon.
#[derive(Default)]
pub(crate) struct LiveWindow {
    active: HashSet<Uuid>,
    finished_long_runs: HashMap<Uuid, Execution>,
}

impl LiveWindow {
    pub(crate) fn read(
        &mut self,
        store: &DynStore,
        now: DateTime<Utc>,
    ) -> Result<Vec<LiveRun>, croniq_store::traits::StoreError> {
        let horizon_start = now - HORIZON;
        let mut by_id: HashMap<Uuid, Execution> = HashMap::new();
        for e in store.list_executions(&ExecutionFilter {
            states: vec![ExecutionState::Queued, ExecutionState::Claimed],
            limit: Some(ACTIVE_LIMIT),
            ..Default::default()
        })? {
            by_id.insert(e.id, e);
        }
        for e in store.list_executions(&ExecutionFilter {
            since: Some(horizon_start),
            limit: Some(RECENT_LIMIT),
            ..Default::default()
        })? {
            by_id.entry(e.id).or_insert(e);
        }

        for id in std::mem::take(&mut self.active) {
            if by_id.contains_key(&id) || self.finished_long_runs.contains_key(&id) {
                continue;
            }
            // Gone from both reads: finished, and created before the horizon.
            if let Some(e) = store.get_execution(id)? {
                self.finished_long_runs.insert(id, e);
            }
        }
        self.finished_long_runs
            .retain(|_, e| e.completed_at.is_some_and(|at| at >= horizon_start));
        for (id, e) in &self.finished_long_runs {
            by_id.entry(*id).or_insert_with(|| e.clone());
        }

        self.active = by_id
            .values()
            .filter(|e| is_active(e.state))
            .map(|e| e.id)
            .collect();

        let mut runs: Vec<LiveRun> = by_id
            .values()
            // A run that ended before the horizon is off every window.
            .filter(|e| is_active(e.state) || e.completed_at.is_none_or(|at| at >= horizon_start))
            .map(LiveRun::from)
            .collect();
        // Stable order, so "unchanged" compares equal and is not resent.
        runs.sort_by(|a, b| a.fire_at.cmp(&b.fire_at).then(a.id.cmp(&b.id)));
        Ok(runs)
    }
}

/// `GET /v1/executions/stream`
///
/// Scope: `executions:read` — the same rows `GET /v1/executions` returns,
/// narrowed to a window and to the fields a timeline draws.
pub async fn handle_executions_stream(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    require_scope(&ctx, Scope::EXECUTIONS_READ)?;
    let store = state.store.clone().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    // A task per connection rather than a stream combinator: the window keeps
    // state between ticks, and a plain loop says that more plainly. It ends
    // when the client goes away and the send fails.
    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(4);
    tokio::spawn(async move {
        let mut window = LiveWindow::default();
        let mut last: Option<Vec<LiveRun>> = None;
        let mut last_sent = tokio::time::Instant::now();
        let mut ticker = tokio::time::interval(TICK);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            if tx.is_closed() {
                break;
            }
            let now = Utc::now();
            // A failed read skips the tick; the next one tries again, and the
            // client keeps drawing from what it has.
            let Ok(runs) = window.read(&store, now) else {
                continue;
            };
            let changed = last.as_ref() != Some(&runs);
            if !changed && last_sent.elapsed() < HEARTBEAT {
                continue;
            }
            let data = match serde_json::to_string(&Frame { now, runs: &runs }) {
                Ok(data) => data,
                Err(_) => continue,
            };
            if tx
                .send(Ok(Event::default().event("executions").data(data)))
                .await
                .is_err()
            {
                break;
            }
            last = Some(runs);
            last_sent = tokio::time::Instant::now();
        }
    });

    Ok(Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::default()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::sqlite_store;
    use croniq_store::sqlite::SqliteStore;

    fn seed(store: &DynStore, job_key: &str, created: DateTime<Utc>) -> Uuid {
        let id = Uuid::new_v4();
        store
            .create_execution(&Execution {
                id,
                job_key: job_key.into(),
                fire_at: created,
                scheduled_for: created,
                attempt: 1,
                state: ExecutionState::Queued,
                runner_id: None,
                claimed_at: None,
                started_at: None,
                completed_at: None,
                duration_ms: None,
                error: None,
                dead_reason: None,
                idempotency_key: None,
                metadata: HashMap::new(),
                created_at: created,
            })
            .unwrap();
        id
    }

    fn finish(store: &DynStore, id: Uuid, at: DateTime<Utc>) {
        store.claim_execution(id, "r1", at).unwrap();
        assert!(
            store
                .complete_execution(
                    id,
                    Some("r1"),
                    ExecutionState::Completed,
                    Some(1),
                    None,
                    None,
                    at
                )
                .unwrap()
        );
    }

    fn ids(runs: &[LiveRun]) -> HashSet<Uuid> {
        runs.iter().map(|r| r.id).collect()
    }

    #[test]
    fn covers_active_and_recent_but_not_old_finished_runs() {
        let store = sqlite_store(SqliteStore::in_memory().unwrap());
        let now = Utc::now();

        let running_old = seed(&store, "a", now - chrono::Duration::hours(1));
        store
            .claim_execution(running_old, "r1", now - chrono::Duration::minutes(59))
            .unwrap();
        let recent_done = seed(&store, "b", now - chrono::Duration::seconds(30));
        finish(&store, recent_done, now - chrono::Duration::seconds(20));
        let old_done = seed(&store, "c", now - chrono::Duration::hours(1));
        finish(&store, old_done, now - chrono::Duration::minutes(50));

        let runs = LiveWindow::default().read(&store, now).unwrap();
        assert_eq!(ids(&runs), HashSet::from([running_old, recent_done]));
    }

    /// The case the two reads alone miss: a run created before the horizon
    /// that finishes inside it. It has to stay on screen with its end time,
    /// not vanish the moment it stops being active.
    #[test]
    fn keeps_a_long_run_that_finishes_inside_the_window() {
        let store = sqlite_store(SqliteStore::in_memory().unwrap());
        let now = Utc::now();
        let long = seed(&store, "long", now - chrono::Duration::minutes(20));
        store
            .claim_execution(long, "r1", now - chrono::Duration::minutes(20))
            .unwrap();

        let mut window = LiveWindow::default();
        assert!(ids(&window.read(&store, now).unwrap()).contains(&long));

        store
            .complete_execution(
                long,
                Some("r1"),
                ExecutionState::Completed,
                Some(1),
                None,
                None,
                now,
            )
            .unwrap();
        let later = now + chrono::Duration::seconds(1);
        let runs = window.read(&store, later).unwrap();
        let run = runs
            .iter()
            .find(|r| r.id == long)
            .expect("finished long run kept");
        assert_eq!(run.state, ExecutionState::Completed);
        assert!(run.completed_at.is_some());

        // ...and is let go once it has aged out of the widest window.
        let much_later = now + HORIZON + chrono::Duration::seconds(1);
        assert!(!ids(&window.read(&store, much_later).unwrap()).contains(&long));
    }

    #[test]
    fn identical_reads_compare_equal() {
        let store = sqlite_store(SqliteStore::in_memory().unwrap());
        let now = Utc::now();
        seed(&store, "a", now);
        seed(&store, "b", now);
        let mut window = LiveWindow::default();
        assert_eq!(
            window.read(&store, now).unwrap(),
            window.read(&store, now).unwrap()
        );
    }
}

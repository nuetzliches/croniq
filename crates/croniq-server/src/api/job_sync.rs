//! One way into the running scheduler for everything that mutates a
//! store-managed job.
//!
//! The scheduler ticks over an in-memory trigger map. The store is a separate
//! thing, and a write to one does not reach the other — that gap is what made a
//! deleted job keep firing until the next reload (issues #634 / #635). The fix
//! there was one `SchedulerCommand::RemoveJob` in `DELETE /v1/jobs/{key}`,
//! which left every *other* mutation path with the same gap: `PUT
//! /v1/jobs/{key}`, activate, deactivate, and all four of the MCP tools that
//! mirror them wrote the store and told the scheduler nothing (issue #653).
//!
//! So the command goes here rather than at each call site. [`sync_job`] reads
//! the job's current rows back out of the store and derives the command from
//! them, which means a caller never has to know *which* command its edit
//! implies — it says "this job changed" and the answer comes from the same
//! state a reader would see.
//!
//! ### What counts as "should be firing"
//!
//! Three rows have to agree before the scheduler is told to fire a job:
//!
//! | Condition | Source | On failure |
//! |---|---|---|
//! | a trigger exists and is `enabled` | `trigger_definitions` | `RemoveJob` |
//! | the job is `is_active` | `job_definitions` | `RemoveJob` |
//! | the schedule parses | `trigger_from_definition` | `RemoveJob` |
//!
//! `is_active` is the one worth naming. Nothing in the scheduler reads it —
//! `grep is_active crates/croniq-server/src/scheduler.rs` finds only the
//! maintenance window — so before this module the flag was decoration:
//! `POST /v1/jobs/{key}/deactivate` returned a job with `is_active: false` and
//! the scheduler went on firing it. Removing the job from the scheduler *is*
//! how the flag is implemented.
//!
//! A job with no `job_definitions` row at all (a bare trigger, which
//! `POST /v1/schedules` can create) is treated as active. The flag can only
//! suppress firing where there is a row to carry it.

use std::collections::HashMap;
use std::sync::Arc;

use crate::api::ServerState;
use crate::loader::{job_config_from_definition, trigger_from_definition};
use crate::scheduler::SchedulerCommand;
use croniq_store::models::TriggerDefinition;

/// Mirror a job's current store state into the running scheduler.
///
/// Call this after any write to a job's definition or trigger rows. It reads
/// both back, decides whether the job should be firing, and sends `AddJob` or
/// `RemoveJob` accordingly. Also refreshes the job's `config_faults` entry, so
/// an edit that fixes an unresolvable calendar clears the fault without waiting
/// for a reload.
///
/// Silent no-op when the server has no scheduler channel (the storeless test
/// setups) or no store. Send failures are ignored for the same reason the other
/// call sites ignore them: the store write already happened and the scheduler
/// receiver only disappears at shutdown.
pub async fn sync_job(state: &ServerState, job_key: &str) {
    sync_job_with(state, job_key, None).await
}

/// [`sync_job`], told which trigger to apply.
///
/// A job may hold several triggers — `trigger_definitions` has no
/// `UNIQUE(job_key)`, `POST /v1/schedules` never refuses a second one, and
/// `jobs.rs` anticipates the case. Looking one up by job key therefore answers
/// "some enabled trigger", not "the one that just changed": the lookup is
/// ordered by `created_at`, so editing the *newer* of two schedules pushed the
/// older one's expression and the scheduler ran a schedule the API had not been
/// asked for (issue #713).
///
/// A caller that has just written a specific row passes it here. `None` keeps
/// the lookup, which is right for the paths that changed the *job* rather than
/// a trigger — activate, deactivate, `PUT /v1/jobs` — where any enabled trigger
/// is the job's schedule.
pub async fn sync_job_with(state: &ServerState, job_key: &str, edited: Option<&TriggerDefinition>) {
    let Some(tx) = state.scheduler_tx.as_ref() else {
        return;
    };
    let Some(store) = state.store.as_ref() else {
        return;
    };

    // The job's own row decides whether it may fire at all. A missing row is
    // not a reason to stop: `POST /v1/schedules` can create a trigger for a key
    // that has no `job_definitions` row, and that job fires today.
    let job_def = store.get_job_definition(job_key).unwrap_or_else(|e| {
        tracing::warn!(
            job_key = %job_key,
            error = %e,
            "could not read the job definition while syncing the scheduler"
        );
        None
    });
    if let Some(ref def) = job_def
        && !def.is_active
    {
        remove_job(state, job_key);
        return;
    }

    let usable = |t: &TriggerDefinition| t.managed_by != "dsl" && t.enabled;

    let trigger_def = match edited {
        // The caller wrote this row; it is the one to apply, if it may fire at
        // all. A disabled or DSL-managed row falls through to `None` below,
        // which takes the job out of the scheduler — which is what disabling
        // the schedule means.
        Some(t) => Some(t.clone()).filter(usable),
        None => store
            .list_triggers(Some(job_key))
            .unwrap_or_else(|e| {
                tracing::warn!(
                    job_key = %job_key,
                    error = %e,
                    "could not read the job's triggers while syncing the scheduler"
                );
                Vec::new()
            })
            .into_iter()
            // A DSL-managed row is not ours to push: the Croniqfile owns that
            // key and a reload is what updates it.
            .find(usable),
    };

    let Some(trigger_def) = trigger_def else {
        remove_job(state, job_key);
        return;
    };

    let resolved = state.resolved_calendars().await;
    let Some(built) = trigger_from_definition(&trigger_def, &resolved, chrono::Utc::now()) else {
        // An unparseable schedule cannot be scheduled. Leaving the old trigger
        // in place would keep firing the *previous* expression, which is worse
        // than not firing: the operator sees their edit accepted and the old
        // schedule running.
        remove_job(state, job_key);
        return;
    };

    let job_config = job_config_from_definition(&trigger_def, job_def.as_ref());
    let _ = tx.send(SchedulerCommand::AddJob {
        job: Box::new(job_config),
        trigger: Box::new(built.trigger),
    });
    state.set_config_fault(&trigger_def.job_key, built.config_fault);
}

/// Take a job out of the running scheduler.
///
/// For deletion, where there is nothing left to read back. Every other caller
/// wants [`sync_job`], which reaches this on its own when the job should stop
/// firing.
pub fn remove_job(state: &ServerState, job_key: &str) {
    if let Some(tx) = state.scheduler_tx.as_ref() {
        let _ = tx.send(SchedulerCommand::RemoveJob {
            job_key: job_key.to_string(),
        });
    }
}

/// Adapter that lets the in-process MCP server reach [`sync_job`].
///
/// `croniq-mcp` cannot name `SchedulerCommand` — it does not depend on
/// `croniq-server`, and the dependency runs the other way — so it declares the
/// [`croniq_mcp::JobSync`] trait and calls it after a mutation. This is the
/// implementation `croniq-server` hands it, which is why the MCP tools and the
/// HTTP handlers now reach the scheduler through the same function.
pub struct SchedulerJobSync {
    state: Arc<ServerState>,
    /// One lock per job key, so two syncs for the same job cannot interleave.
    ///
    /// Each notification spawns a task that re-reads the store and sends a
    /// command. Two of them for the same key — `deactivate_job` then
    /// `activate_job` in quick succession — had no ordering, so whichever
    /// finished last won and the scheduler could be left holding the earlier
    /// state (issue #726). The store is correct either way, which is what made
    /// it silent.
    ///
    /// A map of mutexes rather than one global lock: syncing two unrelated jobs
    /// in parallel is fine, and a single lock would serialise every MCP
    /// mutation on a busy server. Entries are never removed — one `Mutex` per
    /// job key the MCP tools have touched is a few dozen bytes against a job
    /// count that is already bounded by the store.
    locks: std::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl SchedulerJobSync {
    pub fn new(state: Arc<ServerState>) -> Self {
        Self {
            state,
            locks: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// The lock for one job key, created on first use.
    fn lock_for(&self, job_key: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.locks.lock().unwrap_or_else(|e| e.into_inner());
        Arc::clone(locks.entry(job_key.to_string()).or_default())
    }
}

impl croniq_mcp::JobSync for SchedulerJobSync {
    fn job_changed(&self, job_key: &str) {
        // The MCP tool handlers are async but the trait is not: it is called
        // from `rmcp`'s dispatch, and making it async would put an
        // `async_trait` boundary between croniq-mcp and every embedder for one
        // fire-and-forget notification. `sync_job` needs an executor for the
        // calendar read, so it gets one — the tool has already written the
        // store and its answer does not depend on this landing.
        //
        // Under this job's lock, so two notifications for the same key apply in
        // the order they were made. Each re-reads the store, so the last one to
        // run decides — which is correct only if "last" means "most recent"
        // (issue #726).
        let state = Arc::clone(&self.state);
        let lock = self.lock_for(job_key);
        let job_key = job_key.to_string();
        tokio::spawn(async move {
            let _guard = lock.lock().await;
            sync_job(&state, &job_key).await;
        });
    }

    fn job_removed(&self, job_key: &str) {
        remove_job(&self.state, job_key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::ServerState;
    use crate::store::DynStore;
    use crate::store::sqlite_store;
    use croniq_runner::AppState;
    use croniq_store::models::{JobDefinition, TriggerDefinition};
    use croniq_store::sqlite::SqliteStore;
    use tokio::sync::mpsc;

    fn job(key: &str, is_active: bool) -> JobDefinition {
        JobDefinition {
            job_key: key.into(),
            description: None,
            assigned_runner_id: None,
            is_active,
            metadata: Default::default(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            timeout: Some("90s".into()),
            max_retries: Some(7),
            dead_letter_enabled: None,
            dead_letter_retention: None,
            dead_letter_operator_hint: None,
            dead_letter_replay_max_age: None,
            tags: Vec::new(),
        }
    }

    fn trigger(key: &str, enabled: bool) -> TriggerDefinition {
        TriggerDefinition {
            trigger_id: format!("t-{key}"),
            job_key: key.into(),
            cron_expression: Some("5m".into()),
            timezone: None,
            calendar: None,
            window: None,
            not_before: None,
            not_after: None,
            enabled,
            managed_by: "api".into(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    fn wired() -> (
        Arc<ServerState>,
        DynStore,
        mpsc::UnboundedReceiver<SchedulerCommand>,
    ) {
        let store: DynStore = sqlite_store(SqliteStore::in_memory().unwrap());
        let (comp_tx, _comp_rx) = mpsc::unbounded_channel();
        let (sched_tx, sched_rx) = mpsc::unbounded_channel();
        let mut state = ServerState::with_auth(
            AppState::new(),
            comp_tx,
            Some(crate::api::test_auth::jwt()),
            Some(Arc::clone(&store)),
        );
        Arc::get_mut(&mut state).unwrap().scheduler_tx = Some(sched_tx);
        (state, store, sched_rx)
    }

    #[tokio::test]
    async fn an_active_job_with_an_enabled_trigger_is_pushed() {
        let (state, store, mut rx) = wired();
        store
            .create_job_definition(&job("etl:nightly", true))
            .unwrap();
        store.create_trigger(&trigger("etl:nightly", true)).unwrap();

        sync_job(&state, "etl:nightly").await;

        match rx.try_recv().expect("no command pushed") {
            SchedulerCommand::AddJob { job, trigger } => {
                assert_eq!(job.key, "etl:nightly");
                assert!(trigger.next_fire_at.is_some());
            }
            other => panic!("expected AddJob, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn the_pushed_config_carries_the_job_row_not_the_defaults() {
        // The regression this guards: every caller but `register` built the
        // JobConfig with `job_config_from_definition(def, None)`, so the
        // job's own `timeout` and `max_retries` never reached the scheduler
        // and it ran with the 5m / 3 defaults instead (issue #653).
        let (state, store, mut rx) = wired();
        store
            .create_job_definition(&job("etl:nightly", true))
            .unwrap();
        store.create_trigger(&trigger("etl:nightly", true)).unwrap();

        sync_job(&state, "etl:nightly").await;

        match rx.try_recv().expect("no command pushed") {
            SchedulerCommand::AddJob { job, .. } => {
                assert_eq!(job.timeout.as_deref(), Some("90s"));
                assert_eq!(job.retry.max_attempts, 7);
            }
            other => panic!("expected AddJob, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_deactivated_job_is_removed_from_the_scheduler() {
        // `is_active` has no reader in the scheduler; taking the job out of it
        // is how the flag is implemented. Before #653, deactivate wrote the
        // store and the job kept firing.
        let (state, store, mut rx) = wired();
        store
            .create_job_definition(&job("etl:nightly", false))
            .unwrap();
        store.create_trigger(&trigger("etl:nightly", true)).unwrap();

        sync_job(&state, "etl:nightly").await;

        match rx.try_recv().expect("no command pushed") {
            SchedulerCommand::RemoveJob { job_key } => assert_eq!(job_key, "etl:nightly"),
            other => panic!("expected RemoveJob, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_disabled_trigger_removes_the_job() {
        let (state, store, mut rx) = wired();
        store
            .create_job_definition(&job("etl:nightly", true))
            .unwrap();
        store
            .create_trigger(&trigger("etl:nightly", false))
            .unwrap();

        sync_job(&state, "etl:nightly").await;

        match rx.try_recv().expect("no command pushed") {
            SchedulerCommand::RemoveJob { job_key } => assert_eq!(job_key, "etl:nightly"),
            other => panic!("expected RemoveJob, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_bare_trigger_with_no_job_row_still_fires() {
        // `POST /v1/schedules` can create a trigger for a key with no
        // `job_definitions` row. Absent `is_active` must not read as inactive.
        let (state, store, mut rx) = wired();
        store.create_trigger(&trigger("bare:key", true)).unwrap();

        sync_job(&state, "bare:key").await;

        match rx.try_recv().expect("no command pushed") {
            SchedulerCommand::AddJob { job, .. } => assert_eq!(job.key, "bare:key"),
            other => panic!("expected AddJob, got {other:?}"),
        }
    }

    /// A job may hold more than one trigger — nothing enforces otherwise — and
    /// the lookup by job key is ordered by `created_at`. So editing the newer
    /// of two schedules used to push the older one's expression: the store said
    /// one thing and the scheduler ran another, and the next edit flipped it
    /// back (issue #713).
    #[tokio::test]
    async fn the_edited_trigger_is_the_one_applied() {
        let (state, store, mut rx) = wired();
        store
            .create_job_definition(&job("etl:nightly", true))
            .unwrap();

        let mut older = trigger("etl:nightly", true);
        older.trigger_id = "t-older".into();
        older.cron_expression = Some("1h".into());
        older.created_at = chrono::Utc::now() - chrono::Duration::hours(2);
        store.create_trigger(&older).unwrap();

        let mut newer = trigger("etl:nightly", true);
        newer.trigger_id = "t-newer".into();
        newer.cron_expression = Some("10m".into());
        store.create_trigger(&newer).unwrap();

        sync_job_with(&state, "etl:nightly", Some(&newer)).await;

        match rx.try_recv().expect("no command pushed") {
            SchedulerCommand::AddJob { job, .. } => {
                assert_eq!(
                    job.schedule_summary, "10m",
                    "the edited trigger decides, not the oldest one"
                );
            }
            other => panic!("expected AddJob, got {other:?}"),
        }
    }

    /// Disabling the schedule a caller just edited takes the job out, even
    /// though another enabled trigger might exist — the caller named this row,
    /// and "this schedule is off" is what they said.
    #[tokio::test]
    async fn an_edited_trigger_that_is_disabled_removes_the_job() {
        let (state, store, mut rx) = wired();
        store
            .create_job_definition(&job("etl:nightly", true))
            .unwrap();
        let disabled = trigger("etl:nightly", false);
        store.create_trigger(&disabled).unwrap();

        sync_job_with(&state, "etl:nightly", Some(&disabled)).await;

        match rx.try_recv().expect("no command pushed") {
            SchedulerCommand::RemoveJob { job_key } => assert_eq!(job_key, "etl:nightly"),
            other => panic!("expected RemoveJob, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_dsl_managed_trigger_is_left_to_the_croniqfile() {
        let (state, store, mut rx) = wired();
        let mut t = trigger("dsl:job", true);
        t.managed_by = "dsl".into();
        store.create_trigger(&t).unwrap();

        sync_job(&state, "dsl:job").await;

        match rx.try_recv().expect("no command pushed") {
            SchedulerCommand::RemoveJob { job_key } => assert_eq!(job_key, "dsl:job"),
            other => panic!("expected RemoveJob, got {other:?}"),
        }
    }
}

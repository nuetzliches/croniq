//! Scheduler loop: ticks the trigger state machines and enqueues due jobs.
//!
//! Each tick:
//! 1. Evaluates all armed triggers against the current time.
//! 2. For triggers that are due: creates an `Execution` in the store,
//!    enqueues a `WorkItem` in the runner queue, advances the trigger.
//! 3. Persists the updated trigger state (via `JobState` in the store).

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

use chrono::{DateTime, Utc};
#[allow(unused_imports)]
use chrono_tz;
use croniq_bridge::{job_execution_metadata, job_to_work_item};
use croniq_config::compile::{ExecutionMode, JobConfig};
use croniq_runner::{AppState, EphemeralTally};
use croniq_scheduler::schedule::Schedule;
use croniq_scheduler::trigger::{PendingFire, Trigger, TriggerState};
use croniq_store::models::{
    Execution, ExecutionState, JobRegisterFire, JobState, JobStatus, MaintenanceState,
};
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::quota::QuotaGuard;
use crate::register_fire::{self, PendingRegisterFire};
use crate::store::DynStore;

/// How long an unacknowledged ephemeral dispatch stays tracked before it is
/// pruned (issue #263). Comfortably exceeds any realistic job timeout, so a
/// runner that dies mid-execution — and therefore never reports the
/// completion that would clear the id — simply ages out instead of leaking
/// the in-memory tracking map.
const EPHEMERAL_TRACKING_MAX_AGE_HOURS: i64 = 1;

/// Commands that can be sent to the scheduler to modify its state at runtime.
#[derive(Debug)]
pub enum SchedulerCommand {
    /// Add or replace a job + trigger in the scheduler.
    AddJob {
        job: Box<JobConfig>,
        trigger: Box<Trigger>,
    },
    /// Remove a job from the scheduler.
    RemoveJob { job_key: String },
    /// Replace the full trigger + job set (hot-reload).
    ///
    /// The ack sender fires once the swap is applied so callers (e.g. the
    /// admin reload endpoint) can wait for completion before responding.
    Reload {
        triggers: HashMap<String, Trigger>,
        jobs: Vec<JobConfig>,
        ack: oneshot::Sender<()>,
    },
}

/// Liveness signal for the scheduler task (issue #248).
///
/// The scheduler task records a timestamp after every *successful* tick. A
/// tick that times out (a hung store call or a wedged lock) deliberately does
/// **not** update it, so a stalled scheduler surfaces as a stale
/// `croniq_scheduler_last_tick_timestamp` on `/metrics` — distinct from a
/// healthy "nothing was due" tick — even though the HTTP server stays up.
#[derive(Debug, Default)]
pub struct SchedulerHeartbeat {
    /// Unix seconds of the last successful tick. `0` = no tick completed yet.
    pub last_tick_unix: AtomicI64,
    /// Total successful ticks since process start.
    pub ticks_total: AtomicU64,
}

impl SchedulerHeartbeat {
    /// Record a completed tick at `now`.
    pub fn record_tick(&self, now: DateTime<Utc>) {
        self.last_tick_unix
            .store(now.timestamp(), Ordering::Relaxed);
        self.ticks_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Unix seconds of the last successful tick (`0` if none yet).
    pub fn last_tick_unix(&self) -> i64 {
        self.last_tick_unix.load(Ordering::Relaxed)
    }

    /// Total successful ticks since process start.
    pub fn ticks_total(&self) -> u64 {
        self.ticks_total.load(Ordering::Relaxed)
    }
}

/// The result of a single scheduler tick.
#[derive(Debug, Clone)]
pub struct TickResult {
    /// Job keys that were fired in this tick.
    pub fired: Vec<FiredExecution>,
}

/// A single job execution fired by the scheduler.
#[derive(Debug, Clone)]
pub struct FiredExecution {
    pub execution_id: Uuid,
    pub job_key: String,
    pub fire_at: DateTime<Utc>,
    pub attempt: u32,
    /// Whether this fire was dispatched as an ephemeral (non-persisted)
    /// execution. The scheduler task accumulates per-job ephemeral dispatch
    /// counts and folds them into the periodic heartbeat at `INFO`, since the
    /// per-fire dispatch itself only logs at `DEBUG` (issue #275).
    pub is_ephemeral: bool,
}

/// The scheduler loop state.
///
/// Holds the trigger map and references to the store and runner queue.
/// Call `tick()` repeatedly (e.g. every second) from an async task.
pub struct SchedulerLoop {
    pub triggers: HashMap<String, Trigger>,
    jobs: HashMap<String, JobConfig>,
    store: DynStore,
    runner: Arc<AppState>,
    quota: QuotaGuard,
    /// `run_on_register` fires this loop still owes (issue #555). Armed from
    /// the store on every config load, drained by `tick` once each entry's
    /// gate-permitted instant arrives.
    pending_register_fires: Vec<PendingRegisterFire>,
    /// Shared global maintenance switch (from `ServerState`). Defaults to an
    /// all-off handle; wired by `set_maintenance_handle` at boot.
    maintenance: Arc<std::sync::RwLock<MaintenanceState>>,
}

impl SchedulerLoop {
    pub fn new(
        triggers: HashMap<String, Trigger>,
        jobs: Vec<JobConfig>,
        store: DynStore,
        runner: Arc<AppState>,
    ) -> Self {
        let jobs = jobs.into_iter().map(|j| (j.key.clone(), j)).collect();
        let mut loop_ = Self {
            triggers,
            jobs,
            store,
            runner,
            quota: QuotaGuard::new(),
            pending_register_fires: Vec::new(),
            maintenance: Arc::new(std::sync::RwLock::new(MaintenanceState::default())),
        };
        // Boot is an adoption event like any other config load (issue #555).
        // Arming here rather than at the call site is what keeps the next
        // construction path from silently skipping it.
        loop_.arm_register_fires(Utc::now());
        loop_
    }

    /// Wire the shared maintenance switch (from `ServerState`) so the tick can
    /// freeze dispatch while maintenance is active. Until this is called the
    /// scheduler uses an all-off handle (never paused).
    pub fn set_maintenance_handle(
        &mut self,
        maintenance: Arc<std::sync::RwLock<MaintenanceState>>,
    ) {
        self.maintenance = maintenance;
    }

    /// Override the per-job per-minute trigger rate (useful for benchmarking
    /// large trigger counts, where the default 60/min would otherwise throttle
    /// the very firing the benchmark is measuring).
    pub fn set_quota_defaults(&mut self, max_per_minute: u32) {
        for key in self.jobs.keys() {
            self.quota
                .set_quota(key, crate::quota::JobQuota { max_per_minute });
        }
    }

    /// Hot-reload: update jobs and triggers from a newly loaded config.
    ///
    /// Preserves trigger state (fire_count, next_fire_at) for jobs that
    /// still exist. New jobs get fresh triggers; removed jobs are dropped.
    pub fn reload(&mut self, new_triggers: HashMap<String, Trigger>, new_jobs: Vec<JobConfig>) {
        let now = Utc::now();
        let new_jobs_map: HashMap<String, JobConfig> =
            new_jobs.into_iter().map(|j| (j.key.clone(), j)).collect();

        let mut merged = HashMap::new();
        for (key, mut new_trigger) in new_triggers {
            if let Some(old_trigger) = self.triggers.get(&key) {
                // Preserve runtime state from the old trigger
                new_trigger.fire_count = old_trigger.fire_count;
                new_trigger.last_fired_at = old_trigger.last_fired_at;
                if old_trigger.state == TriggerState::Exhausted {
                    // `Exhausted` is terminal only for non-recurring schedules
                    // (`once` / `disabled`). A recurring schedule that was
                    // somehow exhausted must not be frozen by a reload — keep
                    // the freshly-built trigger's Armed state + next_fire_at so
                    // it recovers (issue #249).
                    let recurring = !matches!(
                        new_trigger.schedule,
                        Schedule::Once { .. } | Schedule::Disabled
                    );
                    if !recurring {
                        new_trigger.state = TriggerState::Exhausted;
                        new_trigger.next_fire_at = None;
                    }
                } else if let Some(pending) = old_trigger.next_fire_at {
                    // Carry the pending fire over so a reload neither skips
                    // nor double-fires — but only while it can still belong
                    // to the schedule just loaded. A shortened interval used
                    // to stay silent until the *old*, longer fire elapsed
                    // (#535).
                    if new_trigger.carry_over_pending_fire(pending, now)
                        == PendingFire::HealedOutlivedSchedule
                    {
                        tracing::info!(
                            job_key = %key,
                            pending = %pending,
                            next_fire_at = ?new_trigger.next_fire_at,
                            schedule = %new_trigger.schedule.summary(),
                            "reload: pending fire outlived its schedule (shortened?) — recomputed (#535)"
                        );
                    }
                }
            }
            merged.insert(key, new_trigger);
        }

        let added = merged
            .keys()
            .filter(|k| !self.triggers.contains_key(*k))
            .count();
        let removed = self
            .triggers
            .keys()
            .filter(|k| !merged.contains_key(*k))
            .count();

        self.triggers = merged;
        self.jobs = new_jobs_map;

        // A reload is an adoption event: a job may have appeared, or its
        // definition may have changed under an unchanged key (issue #555).
        self.arm_register_fires(now);

        tracing::info!(
            total = self.triggers.len(),
            added,
            removed,
            "configuration reloaded"
        );
    }

    /// Re-decide which `run_on_register` jobs owe an adoption fire, and when
    /// (issue #555).
    ///
    /// Runs on every config load — boot and each reload — and rebuilds the
    /// pending set from scratch rather than merging into it: the store, not
    /// this loop's memory, is the record of what has already fired, so a
    /// re-plan always agrees with what a fresh boot would decide. A fire
    /// already dispatched has its hash recorded and is therefore not re-armed;
    /// a deferred one is re-armed at the same instant.
    ///
    /// Also prunes records for jobs that have dropped the directive, so
    /// re-adding it later counts as a fresh adoption
    /// ([`register_fire::stale_records`]).
    ///
    /// Store errors are logged, not fatal: a load that cannot read the records
    /// leaves the pending set empty and tries again on the next reload, rather
    /// than firing every such job blind.
    pub fn arm_register_fires(&mut self, now: DateTime<Utc>) {
        let rows = match self.store.list_register_fires() {
            Ok(rows) => rows,
            Err(e) => {
                tracing::error!(
                    error = %e,
                    "run_on_register: could not read adoption records — no adoption fires \
                     armed for this config load"
                );
                self.pending_register_fires.clear();
                return;
            }
        };

        let recorded = register_fire::recorded_hashes(rows);
        let jobs: Vec<JobConfig> = self.jobs.values().cloned().collect();

        for job_key in register_fire::stale_records(&jobs, &recorded) {
            match self.store.delete_register_fire(&job_key) {
                Ok(()) => tracing::info!(
                    job_key = %job_key,
                    "run_on_register: directive removed — forgetting the adoption record so a \
                     later re-add fires again"
                ),
                Err(e) => tracing::warn!(
                    job_key = %job_key,
                    error = %e,
                    "run_on_register: could not forget the adoption record"
                ),
            }
        }

        self.pending_register_fires = register_fire::plan(&jobs, &self.triggers, &recorded, now);
        if !self.pending_register_fires.is_empty() {
            tracing::info!(
                count = self.pending_register_fires.len(),
                job_keys = %self
                    .pending_register_fires
                    .iter()
                    .map(|p| p.job_key.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                "run_on_register: adoption fires armed"
            );
        }
    }

    /// Dispatch every armed adoption fire whose instant has arrived.
    ///
    /// Takes the same path a manual `POST /v1/trigger` does — an execution row
    /// then a work item, without touching the trigger's own `next_fire_at` —
    /// because that is what an adoption fire is: an extra fire, not a
    /// scheduled one. The job's `singleton` / `max_concurrent` guard rides in
    /// on the row metadata and is enforced at claim time exactly as for a
    /// scheduled fire.
    ///
    /// An entry that cannot be dispatched right now (queue at its per-job
    /// depth cap, or a failed store write) stays pending and is retried on the
    /// next tick. Dropping it would lose the reconcile the operator asked for;
    /// firing past the cap would defeat the guard the cap exists for.
    async fn dispatch_due_register_fires(
        &mut self,
        now: DateTime<Utc>,
        fired: &mut Vec<FiredExecution>,
    ) {
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pending_register_fires)
            .into_iter()
            .partition(|p| p.due_at <= now);
        self.pending_register_fires = later;

        for pending in due {
            let Some(job) = self.jobs.get(&pending.job_key).cloned() else {
                tracing::warn!(
                    job_key = %pending.job_key,
                    "run_on_register: job vanished before its adoption fire — dropping it"
                );
                continue;
            };

            let is_ephemeral = job.execution_mode == ExecutionMode::Ephemeral;

            // Per-job queue-depth cap, the same guard the scheduled fire and
            // `POST /v1/trigger` (#299) apply. Ephemeral jobs are exempt for
            // the reason given in `tick`: they self-bound to one queued item.
            if !is_ephemeral {
                let max_depth = job.max_queue_depth.unwrap_or(10) as usize;
                let queued = self
                    .runner
                    .queue
                    .read()
                    .await
                    .count_for_job(&pending.job_key);
                if queued >= max_depth {
                    tracing::debug!(
                        job_key = %pending.job_key,
                        queued,
                        max = max_depth,
                        "run_on_register: queue at its depth cap — adoption fire stays pending"
                    );
                    self.pending_register_fires.push(pending);
                    continue;
                }
            }

            let execution_id = Uuid::new_v4();
            let exec_id_str = execution_id.to_string();

            if !is_ephemeral {
                let execution = Execution {
                    id: execution_id,
                    job_key: job.key.clone(),
                    fire_at: now,
                    // Adoption fire: the logical time is the moment of
                    // adoption itself, like a manual trigger.
                    scheduled_for: now,
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
                    metadata: job_execution_metadata(&job),
                    created_at: now,
                };
                if let Err(e) = self.store.create_execution(&execution) {
                    // Enqueueing anyway would hand a runner an execution_id
                    // with no backing row, so its completion could never be
                    // recorded. Keep it pending and retry next tick.
                    tracing::error!(
                        job_key = %job.key,
                        error = %e,
                        "run_on_register: could not persist the adoption execution — staying pending"
                    );
                    self.pending_register_fires.push(pending);
                    continue;
                }
            }

            let mut item = job_to_work_item(&job, &exec_id_str, now, now, 1);
            crate::trace_propagation::inject_into_metadata(&mut item.metadata);
            if is_ephemeral {
                // Replace-latest, matching the scheduled ephemeral path
                // (issue #263): a runner gap must not accumulate stale
                // non-persisted work.
                let replaced = {
                    let mut q = self.runner.queue.write().await;
                    let replaced = q.remove_job(&job.key);
                    q.enqueue(item);
                    replaced
                };
                self.runner.work_notify.notify_waiters();
                self.runner.forget_ephemeral(&replaced).await;
                self.runner
                    .record_ephemeral(
                        &exec_id_str,
                        now,
                        chrono::Duration::hours(EPHEMERAL_TRACKING_MAX_AGE_HOURS),
                    )
                    .await;
                self.runner.record_ephemeral_fired(&job.key, 1).await;
                self.runner
                    .record_ephemeral_superseded(&job.key, replaced.len() as u64)
                    .await;
            } else {
                self.runner.queue.write().await.enqueue(item);
                self.runner.work_notify.notify_waiters();
            }

            // Recorded only now: a crash before this point leaves the job
            // un-reconciled and the next boot fires again, which is the safe
            // direction for a reconciler.
            if let Err(e) = self.store.upsert_register_fire(&JobRegisterFire {
                job_key: job.key.clone(),
                config_hash: pending.config_hash.clone(),
                fired_at: now,
            }) {
                tracing::error!(
                    job_key = %job.key,
                    error = %e,
                    "run_on_register: adoption fire dispatched but could not be recorded — \
                     the next config load will fire it again"
                );
            }

            fired.push(FiredExecution {
                execution_id,
                job_key: job.key.clone(),
                fire_at: now,
                attempt: 1,
                is_ephemeral,
            });

            tracing::info!(
                job_key = %job.key,
                execution_id = %execution_id,
                reason = pending.reason.as_str(),
                config_hash = %pending.config_hash,
                "run_on_register: adoption fire dispatched"
            );
        }
    }

    /// Process a runtime command (add/remove job, or full reload).
    pub fn apply_command(&mut self, cmd: SchedulerCommand) {
        match cmd {
            SchedulerCommand::AddJob { job, trigger } => {
                let key = job.key.clone();
                tracing::info!(job_key = %key, "scheduler: job added via API");
                self.jobs.insert(key.clone(), *job);
                self.triggers.insert(key, *trigger);
            }
            SchedulerCommand::RemoveJob { job_key } => {
                tracing::info!(job_key = %job_key, "scheduler: job removed via API");
                self.jobs.remove(&job_key);
                self.triggers.remove(&job_key);
            }
            SchedulerCommand::Reload {
                triggers,
                jobs,
                ack,
            } => {
                self.reload(triggers, jobs);
                // The receiver may have dropped (caller lost interest);
                // ignore send failure.
                let _ = ack.send(());
            }
        }
    }

    /// Apply a runtime command, then bring `snapshot` into line with the
    /// result for the key it touched.
    ///
    /// `snapshot` is the trigger map the HTTP side reads — the dashboard
    /// forecast, and `metrics::known_job_keys`, which decides from it which
    /// jobs may emit per-job series at all (issue #470). It used to be written
    /// only at boot and on reload, while `AddJob`/`RemoveJob` reached the
    /// scheduler's own map alone. So a job registered through the API had no
    /// `croniq_job_*` series until the next reload, and on a server without
    /// `--watch` or a `SIGHUP` that meant indefinitely: an operator's
    /// `croniq_job_overdue == 1` alert silently covered none of their
    /// dynamically registered jobs (issue #505).
    ///
    /// Syncing here, rather than at each of the API routes that can send a
    /// command, is what keeps the next such route from forgetting to.
    pub async fn apply_command_synced(
        &mut self,
        cmd: SchedulerCommand,
        snapshot: &tokio::sync::RwLock<HashMap<String, Trigger>>,
    ) {
        // Read the key before `apply_command` consumes the command.
        let touched = match &cmd {
            SchedulerCommand::AddJob { job, .. } => Some(job.key.clone()),
            SchedulerCommand::RemoveJob { job_key } => Some(job_key.clone()),
            // Reload replaces the whole map, and `reload::apply_*` writes the
            // snapshot itself as part of that.
            SchedulerCommand::Reload { .. } => None,
        };
        self.apply_command(cmd);
        let Some(key) = touched else { return };
        // Whatever the scheduler now thinks about this key, the snapshot
        // agrees — including "it is gone".
        let mut snapshot = snapshot.write().await;
        match self.triggers.get(&key) {
            Some(trigger) => {
                snapshot.insert(key, trigger.clone());
            }
            None => {
                snapshot.remove(&key);
            }
        }
    }

    /// Evaluate all triggers at `now`, fire due ones, return results.
    ///
    /// The span is emitted at `trace` (not the `#[instrument]` default of
    /// `info`) because the scheduler loop opens it once per second,
    /// unconditionally, whether or not a job fired. At `info` that idle
    /// per-second span floods any persistent OTLP trace backend with pure
    /// scheduler-heartbeat noise (issue #310). `trace` matches the per-fire
    /// `tracing::trace!` inside the loop and mirrors the log-side denoise
    /// (per-fire logs at debug/trace; a throttled `info` heartbeat carries
    /// liveness — #275). Operators who want the per-tick span opt back in via
    /// `RUST_LOG=…=trace`; real work stays visible through the WARN events
    /// here and the separate `info`-level completion span.
    #[tracing::instrument(level = "trace", skip(self), fields(now = %now, trigger_count = self.triggers.len()))]
    pub async fn tick(&mut self, now: DateTime<Utc>) -> TickResult {
        let mut fired = Vec::new();

        // Global maintenance freezes dispatch. We still advance each due
        // trigger's schedule below (mark_fired) so no catch-up backlog builds
        // up, but emit no execution or work item while the switch is active.
        let maintenance_active = self
            .maintenance
            .read()
            .map(|m| m.is_active(now))
            .unwrap_or(false);

        // Adoption fires first (issue #555): they are the fire an operator is
        // watching for right after a deploy. Unlike a scheduled fire, a frozen
        // one is *held*, not advanced past — there is no schedule to fall
        // behind, and dropping it would lose the reconcile entirely.
        if !maintenance_active && !self.pending_register_fires.is_empty() {
            self.dispatch_due_register_fires(now, &mut fired).await;
        }

        for trigger in self.triggers.values_mut() {
            let Some(fire_at) = trigger.evaluate(now) else {
                continue;
            };

            if maintenance_active {
                trigger.mark_fired(fire_at, now);
                continue;
            }

            // Per-fire trace event — gives operators a single "decided to
            // fire" record per trigger inside the parent `tick` span,
            // without holding a `!Send` `EnteredSpan` across the queue
            // RwLock awaits below.
            tracing::trace!(job_key = %trigger.job_key, fire_at = %fire_at, "evaluating trigger for fire");

            let job = match self.jobs.get(&trigger.job_key) {
                Some(j) => j,
                None => {
                    tracing::warn!(job_key = %trigger.job_key, "trigger fired for unknown job");
                    trigger.mark_fired(fire_at, now);
                    continue;
                }
            };

            let is_ephemeral = job.execution_mode == ExecutionMode::Ephemeral;

            // Backpressure guards (queue-depth + per-minute rate limit) apply
            // only to persisted (`queued`) jobs. Ephemeral jobs are
            // fire-and-forget and self-bound to a single queued item by the
            // replace-latest enqueue below, so subjecting them to the
            // queue-depth cap would wedge them permanently the moment a runner
            // restart let non-persisted work pile up past the cap — the job
            // would then sit `overdue` forever even after the runner returns
            // (issue #263).
            if !is_ephemeral {
                // Queue overflow protection: skip if too many queued executions for this job.
                // Per-job max_queue_depth overrides the default of 10. The
                // `count_for_job` lookup is O(1) — replaces a previous scan that
                // peeked the first 1000 items every tick per trigger.
                let max_depth = job.max_queue_depth.unwrap_or(10) as usize;
                let queued_count = self
                    .runner
                    .queue
                    .read()
                    .await
                    .count_for_job(&trigger.job_key);
                if queued_count >= max_depth {
                    tracing::warn!(
                        job_key = %trigger.job_key,
                        queued = queued_count,
                        max = max_depth,
                        "skipping execution — queue overflow"
                    );
                    trigger.mark_fired(fire_at, now);
                    continue;
                }

                // Quota check: per-job per-minute trigger rate. Self-heals via
                // a sliding window, so unlike the old parallel cap it can't
                // wedge a drained job (the quota-guard leak alongside #263).
                if !self.quota.allow(&trigger.job_key) {
                    tracing::debug!(job_key = %trigger.job_key, "skipping execution — per-minute rate limit exceeded");
                    trigger.mark_fired(fire_at, now);
                    continue;
                }
            }

            let execution_id = Uuid::new_v4();
            let exec_id_str = execution_id.to_string();

            // Build the post-fire trigger state and the corresponding
            // JobState row before any mutation, so we can persist the
            // "this trigger has fired" record together with the new
            // execution. Trigger state is rolled back if the DB write
            // fails so a transient store error doesn't desync memory
            // from disk.
            let prev_trigger_state = (
                trigger.fire_count,
                trigger.next_fire_at,
                trigger.last_fired_at,
                trigger.state,
            );
            trigger.mark_fired(fire_at, now);
            let status = if trigger.state == TriggerState::Exhausted {
                JobStatus::Exhausted
            } else {
                JobStatus::Active
            };
            let job_state = JobState {
                job_key: job.key.clone(),
                next_fire_at: trigger.next_fire_at,
                last_fired_at: Some(fire_at),
                fire_count: trigger.fire_count,
                status,
                updated_at: now,
            };

            // 1. Persist the execution record + advance the job state in
            //    a single transaction (queued mode). For ephemeral jobs
            //    there is no execution row to write, so we only upsert the
            //    job state.
            //
            //    Without the transaction, a crash between the two writes
            //    would leave an execution in the DB while `next_fire_at`
            //    still pointed at the old fire time → on restart the same
            //    trigger would fire again and produce a duplicate.
            let persist_result: Result<(), _> = if is_ephemeral {
                self.store.upsert_job_state(&job_state)
            } else {
                let execution = Execution {
                    id: execution_id,
                    job_key: job.key.clone(),
                    fire_at,
                    // Fresh scheduler fire: logical time equals the fire time.
                    scheduled_for: fire_at,
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
                    // Compiled job metadata plus the effective runner
                    // capabilities. Shared with the adoption fire below so
                    // the two rows cannot drift (see `job_execution_metadata`).
                    metadata: job_execution_metadata(job),
                    created_at: now,
                };
                self.store
                    .create_execution_and_advance_job_state(&execution, &job_state)
            };

            if let Err(e) = persist_result {
                tracing::error!(job_key = %job.key, error = %e, "failed to persist execution+job_state — rolling back trigger");
                // Roll back the in-memory advance so the next tick can retry.
                let (fire_count, next_fire_at, last_fired_at, state) = prev_trigger_state;
                trigger.fire_count = fire_count;
                trigger.next_fire_at = next_fire_at;
                trigger.last_fired_at = last_fired_at;
                trigger.state = state;
                continue;
            }

            // 2. Enqueue work item for the runner (always attempt 1 for scheduler-fired jobs).
            //    Stamp the active tick-span's W3C traceparent into the
            //    work metadata so the runner SDK's execute-span links
            //    back into this trace instead of starting an orphan
            //    root span. No-op when the `otlp` feature is off or no
            //    valid OTel context is in scope.
            let mut item = job_to_work_item(job, &exec_id_str, fire_at, fire_at, 1);
            crate::trace_propagation::inject_into_metadata(&mut item.metadata);
            if is_ephemeral {
                // Keep only the latest fire: drop any earlier, still-unclaimed
                // ephemeral item for this job before enqueuing the new one, so
                // a runner gap can't accumulate stale non-persisted work
                // (issue #263). Both ops happen under one write lock so a poll
                // can't observe a transient empty queue between them.
                let replaced = {
                    let mut q = self.runner.queue.write().await;
                    let replaced = q.remove_job(&job.key);
                    q.enqueue(item);
                    replaced
                };
                self.runner.work_notify.notify_waiters();
                // Replaced ids will never report a completion — stop tracking
                // them — then track the new dispatch so the completion
                // processor recognises it on the (expected) store miss.
                self.runner.forget_ephemeral(&replaced).await;
                self.runner
                    .record_ephemeral(
                        &exec_id_str,
                        now,
                        chrono::Duration::hours(EPHEMERAL_TRACKING_MAX_AGE_HOURS),
                    )
                    .await;
                // Tally both halves of the replacement for the heartbeat
                // (issue #541): this fire, and the older ones it just
                // dropped out of the queue unclaimed.
                self.runner.record_ephemeral_fired(&job.key, 1).await;
                self.runner
                    .record_ephemeral_superseded(&job.key, replaced.len() as u64)
                    .await;
            } else {
                self.runner.queue.write().await.enqueue(item);
                self.runner.work_notify.notify_waiters();
            }

            fired.push(FiredExecution {
                execution_id,
                job_key: job.key.clone(),
                fire_at,
                attempt: 1,
                is_ephemeral,
            });

            if is_ephemeral {
                tracing::debug!(
                    job_key = %job.key,
                    execution_id = %execution_id,
                    "ephemeral execution dispatched"
                );
            } else {
                tracing::info!(
                    job_key = %job.key,
                    execution_id = %execution_id,
                    "execution queued"
                );
            }
        }

        TickResult { fired }
    }
}

/// Render the per-job ephemeral tallies for the scheduler heartbeat
/// (issues #275, #541).
///
/// `[]` when nothing ephemeral fired. Otherwise one `; `-separated entry per
/// job, `fired` and `dispatched` always shown so the two ends of the hop can
/// be compared at a glance, `dropped` / `superseded` only when non-zero:
///
/// ```text
/// ephemeral=[beat:tick fired=300 dispatched=299 superseded=1]
/// ```
///
/// `fired=N dispatched=0` is the signature of issue #539 — fires that never
/// reach a runner — and the reason this line reports both numbers rather than
/// the fire count alone.
pub fn render_ephemeral_stats(stats: &BTreeMap<String, EphemeralTally>) -> String {
    if stats.is_empty() {
        return "[]".to_string();
    }
    let body = stats
        .iter()
        .map(|(job_key, t)| {
            let mut entry = format!("{job_key} fired={} dispatched={}", t.fired, t.dispatched);
            if t.dropped > 0 {
                entry.push_str(&format!(" dropped={}", t.dropped));
            }
            if t.superseded > 0 {
                entry.push_str(&format!(" superseded={}", t.superseded));
            }
            entry
        })
        .collect::<Vec<_>>()
        .join("; ");
    format!("[{body}]")
}

#[cfg(test)]
mod tests {
    use chrono::Duration as ChronoDuration;
    use croniq_config::compile::{DeadLetterConfig, RetryConfig, RunnerConfig};
    use croniq_runner::AppState;
    use croniq_scheduler::{misfire::MisfirePolicy, schedule::Schedule, trigger::TriggerState};
    use croniq_store::sqlite::SqliteStore;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::store::{DynStore, sqlite_store};

    fn make_job(key: &str) -> JobConfig {
        JobConfig {
            key: key.into(),
            namespace: "test".into(),
            name: key.split(':').nth(1).unwrap_or(key).into(),
            variant: None,
            description: None,
            schedule: croniq_config::schedule::CompiledSchedule::Disabled,
            schedule_summary: "every 10 seconds".into(),
            timezone: None,
            calendar: None,
            window: None,
            not_before: None,
            not_after: None,
            runner: RunnerConfig::default(),
            retry: RetryConfig::default(),
            timeout: Some("5m".into()),
            dead_letter: DeadLetterConfig::default(),
            metadata: Default::default(),
            execution_mode: croniq_config::compile::ExecutionMode::default(),
            catch_up: croniq_config::compile::CatchUpPolicy::default(),
            queue_ttl: None,
            max_queue_depth: None,
            keep_last: None,
            max_concurrent: None,
            concurrency_group: None,
            tags: vec![],
            run_on_register: false,
        }
    }

    fn make_trigger_due_now(job_key: &str) -> Trigger {
        let schedule = Schedule::Interval { seconds: 10 };
        let mut trigger = Trigger::new(
            job_key.into(),
            schedule,
            chrono_tz::UTC,
            None,
            None,
            MisfirePolicy::FireNow,
            Utc::now() - ChronoDuration::seconds(60),
        );
        trigger.next_fire_at = Some(Utc::now() - ChronoDuration::seconds(5));
        trigger.state = TriggerState::Armed;
        trigger
    }

    fn make_trigger_future(job_key: &str) -> Trigger {
        let schedule = Schedule::Interval { seconds: 3600 };
        Trigger::new(
            job_key.into(),
            schedule,
            chrono_tz::UTC,
            None,
            None,
            MisfirePolicy::FireNow,
            Utc::now(),
        )
    }

    fn make_store() -> DynStore {
        sqlite_store(SqliteStore::in_memory().unwrap())
    }

    fn make_runner() -> Arc<AppState> {
        AppState::new()
    }

    #[tokio::test]
    async fn tick_fires_overdue_trigger() {
        let store = make_store();
        let runner = make_runner();
        let mut triggers = HashMap::new();
        triggers.insert("test:job".into(), make_trigger_due_now("test:job"));

        let mut scheduler = SchedulerLoop::new(
            triggers,
            vec![make_job("test:job")],
            store,
            Arc::clone(&runner),
        );

        let result = scheduler.tick(Utc::now()).await;
        assert_eq!(result.fired.len(), 1);
        assert_eq!(result.fired[0].job_key, "test:job");
        assert!(
            !result.fired[0].is_ephemeral,
            "a queued job must not be flagged ephemeral"
        );
    }

    #[tokio::test]
    async fn adding_a_job_through_a_command_reaches_the_shared_snapshot() {
        // Issue #505: AddJob only ever reached the scheduler's private map, so
        // a job registered through the API was missing from the snapshot
        // `metrics::known_job_keys` filters on — and lost every
        // `croniq_job_*` series until the next reload.
        let mut scheduler = SchedulerLoop::new(HashMap::new(), vec![], make_store(), make_runner());
        let snapshot = tokio::sync::RwLock::new(HashMap::new());

        scheduler
            .apply_command_synced(
                SchedulerCommand::AddJob {
                    job: Box::new(make_job("test:job")),
                    trigger: Box::new(make_trigger_due_now("test:job")),
                },
                &snapshot,
            )
            .await;

        assert!(scheduler.triggers.contains_key("test:job"));
        assert!(
            snapshot.read().await.contains_key("test:job"),
            "the snapshot the metrics filter reads must know the job too"
        );
    }

    #[tokio::test]
    async fn removing_a_job_through_a_command_clears_it_from_the_snapshot() {
        // The other direction matters just as much: a stale entry would let a
        // deleted job keep emitting series, which is the false positive #470
        // set out to remove.
        let mut triggers = HashMap::new();
        triggers.insert("test:job".to_string(), make_trigger_due_now("test:job"));
        let mut scheduler = SchedulerLoop::new(
            triggers.clone(),
            vec![make_job("test:job")],
            make_store(),
            make_runner(),
        );
        let snapshot = tokio::sync::RwLock::new(triggers);

        scheduler
            .apply_command_synced(
                SchedulerCommand::RemoveJob {
                    job_key: "test:job".into(),
                },
                &snapshot,
            )
            .await;

        assert!(!scheduler.triggers.contains_key("test:job"));
        assert!(snapshot.read().await.is_empty());
    }

    #[tokio::test]
    async fn a_command_for_an_unknown_job_leaves_the_snapshot_alone() {
        // RemoveJob for a key the scheduler never had must not invent an entry
        // or disturb its neighbours.
        let mut keep = HashMap::new();
        keep.insert("other:job".to_string(), make_trigger_due_now("other:job"));
        let mut scheduler = SchedulerLoop::new(
            keep.clone(),
            vec![make_job("other:job")],
            make_store(),
            make_runner(),
        );
        let snapshot = tokio::sync::RwLock::new(keep);

        scheduler
            .apply_command_synced(
                SchedulerCommand::RemoveJob {
                    job_key: "never:existed".into(),
                },
                &snapshot,
            )
            .await;

        let snapshot = snapshot.read().await;
        assert_eq!(snapshot.len(), 1);
        assert!(snapshot.contains_key("other:job"));
    }

    #[tokio::test]
    async fn tick_skips_future_trigger() {
        let store = make_store();
        let runner = make_runner();
        let mut triggers = HashMap::new();
        triggers.insert("test:job".into(), make_trigger_future("test:job"));

        let mut scheduler = SchedulerLoop::new(triggers, vec![make_job("test:job")], store, runner);

        let result = scheduler.tick(Utc::now()).await;
        assert!(result.fired.is_empty());
    }

    #[tokio::test]
    async fn tick_enqueues_work_item() {
        let store = make_store();
        let runner = make_runner();
        let mut triggers = HashMap::new();
        triggers.insert("test:job".into(), make_trigger_due_now("test:job"));

        let mut scheduler = SchedulerLoop::new(
            triggers,
            vec![make_job("test:job")],
            store,
            Arc::clone(&runner),
        );

        scheduler.tick(Utc::now()).await;

        let q = runner.queue.read().await;
        assert_eq!(q.len(), 1);
    }

    #[tokio::test]
    async fn tick_creates_execution_in_store() {
        let store = make_store();
        let runner = make_runner();
        let mut triggers = HashMap::new();
        triggers.insert("test:job".into(), make_trigger_due_now("test:job"));

        let mut scheduler = SchedulerLoop::new(
            triggers,
            vec![make_job("test:job")],
            Arc::clone(&store),
            runner,
        );

        let result = scheduler.tick(Utc::now()).await;
        let exec_id = result.fired[0].execution_id;

        let execution = store.get_execution(exec_id).unwrap().unwrap();
        assert_eq!(execution.job_key, "test:job");
        assert_eq!(execution.state, ExecutionState::Queued);
        assert_eq!(execution.attempt, 1);
    }

    #[tokio::test]
    async fn tick_advances_trigger_state() {
        let store = make_store();
        let runner = make_runner();
        let mut triggers = HashMap::new();
        triggers.insert("test:job".into(), make_trigger_due_now("test:job"));

        let mut scheduler = SchedulerLoop::new(triggers, vec![make_job("test:job")], store, runner);

        let result = scheduler.tick(Utc::now()).await;

        // Trigger fires and goes back to Armed (async execution model)
        assert_eq!(result.fired.len(), 1);
        let trigger = &scheduler.triggers["test:job"];
        assert_eq!(trigger.fire_count, 1);
    }

    #[tokio::test]
    async fn tick_multiple_jobs_fires_all_due() {
        let store = make_store();
        let runner = make_runner();
        let mut triggers = HashMap::new();
        triggers.insert("test:a".into(), make_trigger_due_now("test:a"));
        triggers.insert("test:b".into(), make_trigger_due_now("test:b"));
        triggers.insert("test:c".into(), make_trigger_future("test:c"));

        let mut scheduler = SchedulerLoop::new(
            triggers,
            vec![make_job("test:a"), make_job("test:b"), make_job("test:c")],
            store,
            Arc::clone(&runner),
        );

        let result = scheduler.tick(Utc::now()).await;
        assert_eq!(result.fired.len(), 2);

        let q = runner.queue.read().await;
        assert_eq!(q.len(), 2);
    }

    // ── run_on_register adoption fires (issue #555) ──────────────────────────

    /// A `run_on_register` job whose schedule is an hour out, so nothing but
    /// the adoption fire can produce an execution in these tests.
    fn make_adopting_job(key: &str) -> JobConfig {
        JobConfig {
            run_on_register: true,
            ..make_job(key)
        }
    }

    fn adopting_scheduler(store: DynStore, runner: Arc<AppState>, job: JobConfig) -> SchedulerLoop {
        let mut triggers = HashMap::new();
        triggers.insert(job.key.clone(), make_trigger_future(&job.key));
        SchedulerLoop::new(triggers, vec![job], store, runner)
    }

    #[tokio::test]
    async fn adoption_fire_dispatches_on_first_registration() {
        let store = make_store();
        let runner = make_runner();
        let mut scheduler = adopting_scheduler(
            Arc::clone(&store),
            Arc::clone(&runner),
            make_adopting_job("test:job"),
        );

        let result = scheduler.tick(Utc::now()).await;

        assert_eq!(result.fired.len(), 1, "the adoption fire is a fire");
        assert_eq!(result.fired[0].job_key, "test:job");
        let execution = store
            .get_execution(result.fired[0].execution_id)
            .unwrap()
            .expect("adoption fire persists an execution row like any other");
        assert_eq!(execution.state, ExecutionState::Queued);
        assert_eq!(execution.attempt, 1);
        assert_eq!(runner.queue.read().await.count_for_job("test:job"), 1);
    }

    #[tokio::test]
    async fn adoption_fire_leaves_the_trigger_untouched() {
        // It is an extra fire, not a scheduled one: consuming the schedule's
        // next_fire_at would make the deploy *delay* the next real run.
        let store = make_store();
        let mut scheduler = adopting_scheduler(store, make_runner(), make_adopting_job("test:job"));
        let before = scheduler.triggers["test:job"].next_fire_at;

        scheduler.tick(Utc::now()).await;

        let trigger = &scheduler.triggers["test:job"];
        assert_eq!(trigger.fire_count, 0);
        assert_eq!(trigger.next_fire_at, before);
    }

    #[tokio::test]
    async fn adoption_fire_does_not_repeat_on_the_next_tick() {
        let store = make_store();
        let mut scheduler = adopting_scheduler(store, make_runner(), make_adopting_job("test:job"));

        assert_eq!(scheduler.tick(Utc::now()).await.fired.len(), 1);
        assert_eq!(scheduler.tick(Utc::now()).await.fired.len(), 0);
    }

    #[tokio::test]
    async fn adoption_fire_does_not_repeat_on_a_restart() {
        // The whole point of persisting the hash: a restart storm would fire
        // every such job at once, and so would every `--watch` save.
        let store = make_store();
        let mut first = adopting_scheduler(
            Arc::clone(&store),
            make_runner(),
            make_adopting_job("test:job"),
        );
        assert_eq!(first.tick(Utc::now()).await.fired.len(), 1);

        let mut rebooted = adopting_scheduler(
            Arc::clone(&store),
            make_runner(),
            make_adopting_job("test:job"),
        );
        assert_eq!(rebooted.tick(Utc::now()).await.fired.len(), 0);
    }

    #[tokio::test]
    async fn adoption_fire_repeats_when_the_definition_changes() {
        let store = make_store();
        let mut first = adopting_scheduler(
            Arc::clone(&store),
            make_runner(),
            make_adopting_job("test:job"),
        );
        assert_eq!(first.tick(Utc::now()).await.fired.len(), 1);

        // The deploy the directive exists for: the job's definition changed,
        // so the reconcile has to run now rather than at its next fire.
        let changed = JobConfig {
            timeout: Some("30m".into()),
            ..make_adopting_job("test:job")
        };
        let mut redeployed = adopting_scheduler(Arc::clone(&store), make_runner(), changed.clone());
        assert_eq!(redeployed.tick(Utc::now()).await.fired.len(), 1);

        // …and once, not on every tick thereafter.
        assert_eq!(redeployed.tick(Utc::now()).await.fired.len(), 0);
    }

    #[tokio::test]
    async fn a_cosmetic_edit_does_not_produce_an_adoption_fire() {
        let store = make_store();
        let mut first = adopting_scheduler(
            Arc::clone(&store),
            make_runner(),
            make_adopting_job("test:job"),
        );
        assert_eq!(first.tick(Utc::now()).await.fired.len(), 1);

        let reworded = JobConfig {
            description: Some("now with a longer explanation".into()),
            ..make_adopting_job("test:job")
        };
        let mut redeployed = adopting_scheduler(Arc::clone(&store), make_runner(), reworded);
        assert_eq!(
            redeployed.tick(Utc::now()).await.fired.len(),
            0,
            "rewording a description must not re-run a credential rotation"
        );
    }

    #[tokio::test]
    async fn a_job_without_the_directive_produces_no_adoption_fire() {
        let store = make_store();
        let mut scheduler = adopting_scheduler(store, make_runner(), make_job("test:job"));
        assert_eq!(scheduler.tick(Utc::now()).await.fired.len(), 0);
    }

    #[tokio::test]
    async fn adoption_fire_carries_the_jobs_concurrency_guard_and_capabilities() {
        // The guard is enforced at claim time off the row metadata, so the
        // adoption fire has to stamp it exactly as a scheduled fire does —
        // otherwise `singleton` silently does not apply to it.
        let store = make_store();
        let runner = make_runner();
        let mut job = make_adopting_job("test:job");
        job.metadata
            .insert("__max_concurrent".into(), "1".to_string());
        job.runner.require = vec!["credentials".into()];

        let mut scheduler = adopting_scheduler(Arc::clone(&store), Arc::clone(&runner), job);
        let result = scheduler.tick(Utc::now()).await;

        let execution = store
            .get_execution(result.fired[0].execution_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            execution
                .metadata
                .get("__max_concurrent")
                .map(String::as_str),
            Some("1")
        );
        assert_eq!(
            execution.metadata.get("__require").map(String::as_str),
            Some(r#"["credentials"]"#)
        );
    }

    #[tokio::test]
    async fn adoption_fire_of_an_ephemeral_job_persists_no_row() {
        let store = make_store();
        let runner = make_runner();
        let mut job = make_adopting_job("beat:tick");
        job.execution_mode = ExecutionMode::Ephemeral;

        let mut scheduler = adopting_scheduler(Arc::clone(&store), Arc::clone(&runner), job);
        let result = scheduler.tick(Utc::now()).await;

        assert_eq!(result.fired.len(), 1);
        assert!(result.fired[0].is_ephemeral);
        assert!(
            store
                .get_execution(result.fired[0].execution_id)
                .unwrap()
                .is_none(),
            "ephemeral executions are never persisted"
        );
        assert_eq!(runner.queue.read().await.count_for_job("beat:tick"), 1);
    }

    #[tokio::test]
    async fn adoption_fire_is_held_during_maintenance_not_dropped() {
        // A scheduled fire is advanced past while dispatch is frozen (no
        // catch-up backlog). An adoption fire has no schedule to fall behind,
        // and dropping it would lose the reconcile entirely.
        let store = make_store();
        let mut scheduler = adopting_scheduler(
            Arc::clone(&store),
            make_runner(),
            make_adopting_job("test:job"),
        );
        let maintenance = Arc::new(std::sync::RwLock::new(MaintenanceState {
            manual_active: true,
            ..MaintenanceState::default()
        }));
        scheduler.set_maintenance_handle(Arc::clone(&maintenance));

        assert_eq!(scheduler.tick(Utc::now()).await.fired.len(), 0);

        maintenance.write().unwrap().manual_active = false;
        assert_eq!(scheduler.tick(Utc::now()).await.fired.len(), 1);
    }

    #[tokio::test]
    async fn adoption_fire_waits_for_its_gate_to_open() {
        let store = make_store();
        let now = Utc::now();
        let mut scheduler = adopting_scheduler(
            Arc::clone(&store),
            make_runner(),
            make_adopting_job("test:job"),
        );
        // Re-arm with a gate that opens in an hour, at a controlled `now`.
        scheduler.triggers.get_mut("test:job").unwrap().not_before =
            Some(now + ChronoDuration::hours(1));
        scheduler.arm_register_fires(now);

        assert_eq!(scheduler.tick(now).await.fired.len(), 0, "deferred");
        assert_eq!(
            scheduler
                .tick(now + ChronoDuration::hours(1))
                .await
                .fired
                .len(),
            1,
            "and fired once the gate opened"
        );
    }

    #[tokio::test]
    async fn adoption_fire_stays_pending_while_the_queue_is_at_its_cap() {
        // Firing past the cap would defeat the guard; dropping the fire would
        // lose the reconcile. So it waits.
        let store = make_store();
        let runner = make_runner();
        let mut job = make_adopting_job("test:job");
        job.max_queue_depth = Some(1);

        let mut scheduler =
            adopting_scheduler(Arc::clone(&store), Arc::clone(&runner), job.clone());
        runner.queue.write().await.enqueue(job_to_work_item(
            &job,
            "occupant",
            Utc::now(),
            Utc::now(),
            1,
        ));

        assert_eq!(scheduler.tick(Utc::now()).await.fired.len(), 0);

        runner.queue.write().await.remove_job("test:job");
        assert_eq!(scheduler.tick(Utc::now()).await.fired.len(), 1);
    }

    #[tokio::test]
    async fn reload_arms_an_adoption_fire_for_a_changed_definition() {
        // The `--watch` / SIGHUP path: the key is unchanged, so nothing else
        // in the reload notices, but the definition is not the one that fired.
        let store = make_store();
        let mut scheduler = adopting_scheduler(
            Arc::clone(&store),
            make_runner(),
            make_adopting_job("test:job"),
        );
        assert_eq!(scheduler.tick(Utc::now()).await.fired.len(), 1);

        let changed = JobConfig {
            timeout: Some("30m".into()),
            ..make_adopting_job("test:job")
        };
        let mut triggers = HashMap::new();
        triggers.insert("test:job".to_string(), make_trigger_future("test:job"));
        scheduler.reload(triggers, vec![changed]);

        assert_eq!(scheduler.tick(Utc::now()).await.fired.len(), 1);
    }

    #[tokio::test]
    async fn dropping_the_directive_forgets_the_record_so_a_re_add_fires() {
        let store = make_store();
        let mut scheduler = adopting_scheduler(
            Arc::clone(&store),
            make_runner(),
            make_adopting_job("test:job"),
        );
        assert_eq!(scheduler.tick(Utc::now()).await.fired.len(), 1);
        assert_eq!(store.list_register_fires().unwrap().len(), 1);

        // Directive removed: the record describes a contract that is gone.
        let mut triggers = HashMap::new();
        triggers.insert("test:job".to_string(), make_trigger_future("test:job"));
        scheduler.reload(triggers.clone(), vec![make_job("test:job")]);
        assert!(store.list_register_fires().unwrap().is_empty());
        assert_eq!(scheduler.tick(Utc::now()).await.fired.len(), 0);

        // Added back: a fresh adoption, so it fires — even though nothing else
        // about the job changed.
        scheduler.reload(triggers, vec![make_adopting_job("test:job")]);
        assert_eq!(scheduler.tick(Utc::now()).await.fired.len(), 1);
    }

    #[tokio::test]
    async fn reload_does_not_forget_the_record_of_a_job_absent_from_the_config() {
        // Same judgement `restore_trigger_states` makes about orphan
        // `job_states`: this pass cannot tell "deleted" from "commented out".
        let store = make_store();
        let mut scheduler = adopting_scheduler(
            Arc::clone(&store),
            make_runner(),
            make_adopting_job("test:job"),
        );
        assert_eq!(scheduler.tick(Utc::now()).await.fired.len(), 1);

        scheduler.reload(HashMap::new(), vec![]);
        assert_eq!(store.list_register_fires().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn reload_rearms_exhausted_recurring_trigger() {
        // Regression for #249(b): a hot-reload must not freeze a recurring
        // trigger that was stuck Exhausted — it should pick up the freshly
        // built Armed trigger instead.
        let store = make_store();
        let runner = make_runner();

        let mut old = make_trigger_future("test:job"); // recurring (Interval)
        old.state = TriggerState::Exhausted;
        old.next_fire_at = None;
        let mut triggers = HashMap::new();
        triggers.insert("test:job".to_string(), old);

        let mut scheduler = SchedulerLoop::new(triggers, vec![make_job("test:job")], store, runner);

        let mut new_triggers = HashMap::new();
        new_triggers.insert("test:job".to_string(), make_trigger_future("test:job"));
        scheduler.reload(new_triggers, vec![make_job("test:job")]);

        let t = &scheduler.triggers["test:job"];
        assert_eq!(t.state, TriggerState::Armed);
        assert!(t.next_fire_at.is_some());
    }

    #[tokio::test]
    async fn reload_recomputes_a_pending_fire_the_new_schedule_outlived() {
        // Regression for #535: `--watch` picks up `every 1 hour` -> `every 1
        // minute`, but the pending hourly fire was carried over unconditionally
        // and the job stayed silent until it elapsed — up to a day for a
        // daily -> hourly edit.
        let store = make_store();
        let runner = make_runner();

        let old = make_trigger_future("test:job"); // Interval 1h, fires in 1h
        let pending = old.next_fire_at.unwrap();
        let mut triggers = HashMap::new();
        triggers.insert("test:job".to_string(), old);

        let mut scheduler = SchedulerLoop::new(triggers, vec![make_job("test:job")], store, runner);

        let mut new_triggers = HashMap::new();
        new_triggers.insert(
            "test:job".to_string(),
            Trigger::new(
                "test:job".into(),
                Schedule::Interval { seconds: 60 },
                chrono_tz::UTC,
                None,
                None,
                MisfirePolicy::FireNow,
                Utc::now(),
            ),
        );
        scheduler.reload(new_triggers, vec![make_job("test:job")]);

        let t = &scheduler.triggers["test:job"];
        assert_eq!(t.state, TriggerState::Armed);
        let next = t.next_fire_at.unwrap();
        assert!(
            next < pending,
            "pending hourly fire {pending} must not survive the switch to every 1 minute"
        );
        assert!(next <= Utc::now() + ChronoDuration::seconds(61));
    }

    #[tokio::test]
    async fn reload_carries_over_a_still_valid_pending_fire() {
        // The common case: an unrelated edit reloads the file. The pending
        // fire must survive, or a `--watch` save could postpone every job.
        let store = make_store();
        let runner = make_runner();

        let old = make_trigger_due_now("test:job"); // overdue by 5s
        let pending = old.next_fire_at.unwrap();
        let mut triggers = HashMap::new();
        triggers.insert("test:job".to_string(), old);

        let mut scheduler = SchedulerLoop::new(triggers, vec![make_job("test:job")], store, runner);

        let mut new_triggers = HashMap::new();
        new_triggers.insert("test:job".to_string(), make_trigger_due_now("test:job"));
        scheduler.reload(new_triggers, vec![make_job("test:job")]);

        assert_eq!(scheduler.triggers["test:job"].next_fire_at, Some(pending));
    }

    #[tokio::test]
    async fn reload_keeps_exhausted_once_trigger_terminal() {
        let store = make_store();
        let runner = make_runner();

        let make_once = || {
            Trigger::new(
                "test:once".into(),
                Schedule::Once {
                    at: Utc::now() + ChronoDuration::hours(1),
                },
                chrono_tz::UTC,
                None,
                None,
                MisfirePolicy::FireNow,
                Utc::now(),
            )
        };

        let mut old = make_once();
        old.state = TriggerState::Exhausted;
        old.next_fire_at = None;
        let mut triggers = HashMap::new();
        triggers.insert("test:once".to_string(), old);

        let mut scheduler =
            SchedulerLoop::new(triggers, vec![make_job("test:once")], store, runner);

        let mut new_triggers = HashMap::new();
        new_triggers.insert("test:once".to_string(), make_once());
        scheduler.reload(new_triggers, vec![make_job("test:once")]);

        let t = &scheduler.triggers["test:once"];
        assert_eq!(t.state, TriggerState::Exhausted);
        assert!(t.next_fire_at.is_none());
    }

    #[test]
    fn heartbeat_records_tick() {
        let hb = SchedulerHeartbeat::default();
        assert_eq!(hb.last_tick_unix(), 0);
        assert_eq!(hb.ticks_total(), 0);

        let now = Utc::now();
        hb.record_tick(now);
        assert_eq!(hb.last_tick_unix(), now.timestamp());
        assert_eq!(hb.ticks_total(), 1);

        hb.record_tick(now + ChronoDuration::seconds(1));
        assert_eq!(hb.ticks_total(), 2);
    }

    #[tokio::test]
    async fn tick_with_empty_triggers() {
        let store = make_store();
        let runner = make_runner();

        let mut scheduler = SchedulerLoop::new(HashMap::new(), vec![], store, runner);

        let result = scheduler.tick(Utc::now()).await;
        assert!(result.fired.is_empty());
    }

    // ─── Ephemeral jobs (issue #263) ────────────────────────────────

    fn make_ephemeral_job(key: &str) -> JobConfig {
        let mut j = make_job(key);
        j.execution_mode = ExecutionMode::Ephemeral;
        j
    }

    /// Regression for #263: an ephemeral job whose work is never drained
    /// (runner absent) must keep firing instead of wedging once the
    /// accumulated queue hits the depth/quota cap. Replace-latest keeps a
    /// single queued item, and the backpressure guards are bypassed.
    #[tokio::test]
    async fn ephemeral_job_never_wedges_when_runner_absent() {
        let store = make_store();
        let runner = make_runner();
        let mut triggers = HashMap::new();
        triggers.insert("beat:tick".into(), make_trigger_due_now("beat:tick"));

        let mut scheduler = SchedulerLoop::new(
            triggers,
            vec![make_ephemeral_job("beat:tick")],
            store,
            Arc::clone(&runner),
        );

        // Nothing ever dequeues. Fire well past the default cap of 10.
        let mut now = Utc::now();
        let mut total_fired = 0;
        for _ in 0..25 {
            total_fired += scheduler.tick(now).await.fired.len();
            now += ChronoDuration::seconds(60); // guarantee the 10s interval is due
        }

        assert_eq!(total_fired, 25, "ephemeral job should fire on every tick");
        // Replace-latest keeps exactly one queued item …
        assert_eq!(runner.queue.read().await.count_for_job("beat:tick"), 1);
        // … and exactly one tracked ephemeral dispatch (no map leak).
        assert_eq!(runner.ephemeral_inflight.read().await.len(), 1);
    }

    /// Each ephemeral fire is flagged `is_ephemeral` on its `FiredExecution`
    /// so the scheduler task can fold per-job dispatch counts into the
    /// heartbeat at `INFO` (issue #275).
    #[tokio::test]
    async fn ephemeral_fire_is_flagged_ephemeral() {
        let store = make_store();
        let runner = make_runner();
        let mut triggers = HashMap::new();
        triggers.insert("beat:tick".into(), make_trigger_due_now("beat:tick"));

        let mut scheduler = SchedulerLoop::new(
            triggers,
            vec![make_ephemeral_job("beat:tick")],
            store,
            runner,
        );

        let result = scheduler.tick(Utc::now()).await;
        assert_eq!(result.fired.len(), 1);
        assert!(
            result.fired[0].is_ephemeral,
            "an ephemeral job must be flagged ephemeral"
        );
    }

    /// The heartbeat's ephemeral summary reports both ends of the hop, so
    /// `fired=N dispatched=0` — the #539 signature — is readable at a glance
    /// (issue #541).
    #[test]
    fn render_ephemeral_stats_shows_both_ends_of_the_hop() {
        assert_eq!(render_ephemeral_stats(&BTreeMap::new()), "[]");

        let mut stats = BTreeMap::new();
        stats.insert(
            "beat:tick".to_string(),
            EphemeralTally {
                fired: 300,
                dispatched: 0,
                ..Default::default()
            },
        );
        assert_eq!(
            render_ephemeral_stats(&stats),
            "[beat:tick fired=300 dispatched=0]"
        );

        // dropped / superseded appear only when they have something to say,
        // and jobs render in a stable (BTreeMap) order.
        stats.insert(
            "audit:sweep".to_string(),
            EphemeralTally {
                fired: 5,
                dispatched: 3,
                dropped: 1,
                superseded: 1,
            },
        );
        assert_eq!(
            render_ephemeral_stats(&stats),
            concat!(
                "[audit:sweep fired=5 dispatched=3 dropped=1 superseded=1; ",
                "beat:tick fired=300 dispatched=0]"
            )
        );
    }

    /// Every ephemeral fire is tallied, and a fire that replaces a still-queued
    /// predecessor tallies that predecessor as superseded rather than leaving
    /// it to look like a loss (issue #541).
    #[tokio::test]
    async fn ephemeral_fires_and_replacements_are_tallied() {
        let store = make_store();
        let runner = make_runner();
        let mut triggers = HashMap::new();
        triggers.insert("beat:tick".into(), make_trigger_due_now("beat:tick"));

        let mut scheduler = SchedulerLoop::new(
            triggers,
            vec![make_ephemeral_job("beat:tick")],
            store,
            Arc::clone(&runner),
        );

        // Three fires, nothing ever dequeued: one survives in the queue, two
        // were replaced.
        let mut now = Utc::now();
        for _ in 0..3 {
            scheduler.tick(now).await;
            now += ChronoDuration::seconds(60);
        }

        let stats = runner.take_ephemeral_stats().await;
        let tally = stats.get("beat:tick").expect("ephemeral job tallied");
        assert_eq!(tally.fired, 3);
        assert_eq!(tally.superseded, 2, "two fires were replaced unclaimed");
        assert_eq!(tally.dispatched, 0, "no poll ran");
        assert_eq!(tally.dropped, 0);
        // Draining is what makes the heartbeat per-interval.
        assert!(runner.take_ephemeral_stats().await.is_empty());
    }

    /// The *queued item* — not just the `FiredExecution` — carries the
    /// ephemeral flag, which is what lets the poll dispatch path skip the
    /// store claim for work that has no row (issue #539).
    #[tokio::test]
    async fn ephemeral_fire_flags_the_queued_work_item() {
        let store = make_store();
        let runner = make_runner();
        let mut triggers = HashMap::new();
        triggers.insert("beat:tick".into(), make_trigger_due_now("beat:tick"));
        triggers.insert("etl:sync".into(), make_trigger_due_now("etl:sync"));

        let mut scheduler = SchedulerLoop::new(
            triggers,
            vec![make_ephemeral_job("beat:tick"), make_job("etl:sync")],
            store,
            Arc::clone(&runner),
        );

        scheduler.tick(Utc::now()).await;

        let q = runner.queue.read().await;
        let items = q.peek_n(q.len());
        let ephemeral = items
            .iter()
            .find(|i| i.job_key == "beat:tick")
            .expect("ephemeral item queued");
        let queued = items
            .iter()
            .find(|i| i.job_key == "etl:sync")
            .expect("queued item queued");
        assert!(
            ephemeral.is_ephemeral,
            "an ephemeral fire must flag its work item — otherwise dispatch              looks for a store row that was never written"
        );
        assert!(
            !queued.is_ephemeral,
            "a persisted fire must not be flagged ephemeral"
        );
    }

    /// A fresh ephemeral fire replaces the previous still-queued one and the
    /// queued item carries the newest execution id.
    #[tokio::test]
    async fn ephemeral_fire_replaces_previous_queued_item() {
        let store = make_store();
        let runner = make_runner();
        let mut triggers = HashMap::new();
        triggers.insert("beat:tick".into(), make_trigger_due_now("beat:tick"));

        let mut scheduler = SchedulerLoop::new(
            triggers,
            vec![make_ephemeral_job("beat:tick")],
            store,
            Arc::clone(&runner),
        );

        let now = Utc::now();
        let first = scheduler.tick(now).await.fired[0].execution_id;
        let second = scheduler
            .tick(now + ChronoDuration::seconds(60))
            .await
            .fired[0]
            .execution_id;
        assert_ne!(first, second);

        let q = runner.queue.read().await;
        assert_eq!(q.count_for_job("beat:tick"), 1);
        assert_eq!(
            q.peek().unwrap().execution_id,
            second.to_string(),
            "only the latest fire stays queued"
        );
    }

    /// Guard rail: the overflow guard must still bound *queued* jobs — the
    /// #263 fix only exempts ephemeral mode.
    #[tokio::test]
    async fn queued_job_still_capped_by_overflow_guard() {
        let store = make_store();
        let runner = make_runner();
        let mut triggers = HashMap::new();
        triggers.insert("etl:sync".into(), make_trigger_due_now("etl:sync"));

        let mut scheduler = SchedulerLoop::new(
            triggers,
            vec![make_job("etl:sync")], // default = Queued
            store,
            Arc::clone(&runner),
        );

        let mut now = Utc::now();
        for _ in 0..15 {
            scheduler.tick(now).await;
            now += ChronoDuration::seconds(60);
        }
        // Default max_queue_depth is 10 — the guard stops enqueuing beyond it.
        assert_eq!(runner.queue.read().await.count_for_job("etl:sync"), 10);
    }

    /// Regression for the quota-guard leak found alongside #263: a *queued*
    /// (persisted) job whose executions are drained each tick — so the
    /// `max_queue_depth` overflow guard never trips — must keep firing well
    /// past the parallel cap. The old `QuotaGuard` incremented a monotonic
    /// `active` counter on every fire but only ever decremented it via a
    /// `release()` that was never called in production, so after
    /// `max_parallel` (10) fires the quota wedged the job `overdue` forever.
    #[tokio::test]
    async fn queued_job_keeps_firing_when_drained_past_parallel_cap() {
        let store = make_store();
        let runner = make_runner();
        let mut triggers = HashMap::new();
        triggers.insert("etl:sync".into(), make_trigger_due_now("etl:sync"));

        let mut scheduler = SchedulerLoop::new(
            triggers,
            vec![make_job("etl:sync")], // default = Queued
            store,
            Arc::clone(&runner),
        );

        // Fire far past the parallel cap of 10 (but under the 60/min rate
        // limit), draining the queue every tick so the overflow guard — the
        // *other* backpressure path — never masks the leak.
        let mut now = Utc::now();
        let mut total_fired = 0;
        for _ in 0..25 {
            total_fired += scheduler.tick(now).await.fired.len();
            // Simulate a runner claiming all queued work, clearing the
            // overflow guard's per-job queued count.
            runner.queue.write().await.drain();
            now += ChronoDuration::seconds(60);
        }

        assert_eq!(
            total_fired, 25,
            "queued job must keep firing once work is drained — quota must not leak"
        );
    }
}

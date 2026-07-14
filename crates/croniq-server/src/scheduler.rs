//! Scheduler loop: ticks the trigger state machines and enqueues due jobs.
//!
//! Each tick:
//! 1. Evaluates all armed triggers against the current time.
//! 2. For triggers that are due: creates an `Execution` in the store,
//!    enqueues a `WorkItem` in the runner queue, advances the trigger.
//! 3. Persists the updated trigger state (via `JobState` in the store).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

use chrono::{DateTime, Utc};
#[allow(unused_imports)]
use chrono_tz;
use croniq_bridge::job_to_work_item;
use croniq_config::compile::{ExecutionMode, JobConfig};
use croniq_runner::AppState;
use croniq_scheduler::schedule::Schedule;
use croniq_scheduler::trigger::{Trigger, TriggerState};
use croniq_store::models::{Execution, ExecutionState, JobState, JobStatus};
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::quota::QuotaGuard;
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
}

impl SchedulerLoop {
    pub fn new(
        triggers: HashMap<String, Trigger>,
        jobs: Vec<JobConfig>,
        store: DynStore,
        runner: Arc<AppState>,
    ) -> Self {
        let jobs = jobs.into_iter().map(|j| (j.key.clone(), j)).collect();
        Self {
            triggers,
            jobs,
            store,
            runner,
            quota: QuotaGuard::new(),
        }
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
                } else if old_trigger.next_fire_at.is_some() {
                    new_trigger.next_fire_at = old_trigger.next_fire_at;
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

        tracing::info!(
            total = self.triggers.len(),
            added,
            removed,
            "configuration reloaded"
        );
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

        for trigger in self.triggers.values_mut() {
            let Some(fire_at) = trigger.evaluate(now) else {
                continue;
            };

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
                    metadata: {
                        let mut m = job.metadata.clone();
                        if !job.runner.require.is_empty() {
                            m.insert(
                                "__require".into(),
                                serde_json::to_string(&job.runner.require).unwrap_or_default(),
                            );
                        }
                        if !job.runner.prefer.is_empty() {
                            m.insert(
                                "__prefer".into(),
                                serde_json::to_string(&job.runner.prefer).unwrap_or_default(),
                            );
                        }
                        m
                    },
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
            let mut item = job_to_work_item(job, &exec_id_str, fire_at, 1);
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
            max_concurrent: None,
            tags: vec![],
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

//! Scheduler loop: ticks the trigger state machines and enqueues due jobs.
//!
//! Each tick:
//! 1. Evaluates all armed triggers against the current time.
//! 2. For triggers that are due: creates an `Execution` in the store,
//!    enqueues a `WorkItem` in the runner queue, advances the trigger.
//! 3. Persists the updated trigger state (via `JobState` in the store).

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use croniq_config::compile::JobConfig;
#[allow(unused_imports)]
use chrono_tz;
use croniq_bridge::job_to_work_item;
use croniq_runner::AppState;
use croniq_scheduler::trigger::{Trigger, TriggerState};
use croniq_store::models::{Execution, ExecutionState, JobState, JobStatus};
use uuid::Uuid;

use crate::store::DynStore;

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
}

impl SchedulerLoop {
    pub fn new(
        triggers: HashMap<String, Trigger>,
        jobs: Vec<JobConfig>,
        store: DynStore,
        runner: Arc<AppState>,
    ) -> Self {
        let jobs = jobs.into_iter().map(|j| (j.key.clone(), j)).collect();
        Self { triggers, jobs, store, runner }
    }

    /// Hot-reload: update jobs and triggers from a newly loaded config.
    ///
    /// Preserves trigger state (fire_count, next_fire_at) for jobs that
    /// still exist. New jobs get fresh triggers; removed jobs are dropped.
    pub fn reload(
        &mut self,
        new_triggers: HashMap<String, Trigger>,
        new_jobs: Vec<JobConfig>,
    ) {
        let new_jobs_map: HashMap<String, JobConfig> =
            new_jobs.into_iter().map(|j| (j.key.clone(), j)).collect();

        let mut merged = HashMap::new();
        for (key, mut new_trigger) in new_triggers {
            if let Some(old_trigger) = self.triggers.get(&key) {
                // Preserve runtime state from the old trigger
                new_trigger.fire_count = old_trigger.fire_count;
                new_trigger.last_fired_at = old_trigger.last_fired_at;
                if old_trigger.state == TriggerState::Exhausted {
                    new_trigger.state = TriggerState::Exhausted;
                    new_trigger.next_fire_at = None;
                } else if old_trigger.next_fire_at.is_some() {
                    new_trigger.next_fire_at = old_trigger.next_fire_at;
                }
            }
            merged.insert(key, new_trigger);
        }

        let added = merged.keys().filter(|k| !self.triggers.contains_key(*k)).count();
        let removed = self.triggers.keys().filter(|k| !merged.contains_key(*k)).count();

        self.triggers = merged;
        self.jobs = new_jobs_map;

        tracing::info!(
            total = self.triggers.len(),
            added,
            removed,
            "configuration reloaded"
        );
    }

    /// Evaluate all triggers at `now`, fire due ones, return results.
    pub async fn tick(&mut self, now: DateTime<Utc>) -> TickResult {
        let mut fired = Vec::new();

        for trigger in self.triggers.values_mut() {
            let Some(fire_at) = trigger.evaluate(now) else {
                continue;
            };

            let job = match self.jobs.get(&trigger.job_key) {
                Some(j) => j,
                None => {
                    tracing::warn!(job_key = %trigger.job_key, "trigger fired for unknown job");
                    trigger.mark_fired(fire_at, now);
                    continue;
                }
            };

            // Queue overflow protection: skip if too many queued executions for this job
            const MAX_QUEUED_PER_JOB: usize = 10;
            let queued_count = self.runner.queue.read().await
                .peek_n(1000)
                .iter()
                .filter(|item| item.job_key == trigger.job_key)
                .count();
            if queued_count >= MAX_QUEUED_PER_JOB {
                tracing::warn!(
                    job_key = %trigger.job_key,
                    queued = queued_count,
                    max = MAX_QUEUED_PER_JOB,
                    "skipping execution — queue overflow"
                );
                trigger.mark_fired(fire_at, now);
                continue;
            }

            let execution_id = Uuid::new_v4();
            let exec_id_str = execution_id.to_string();

            // 1. Persist the execution record
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
                metadata: {
                    let mut m = job.metadata.clone();
                    if !job.runner.require.is_empty() {
                        m.insert("__require".into(), serde_json::to_string(&job.runner.require).unwrap_or_default());
                    }
                    if !job.runner.prefer.is_empty() {
                        m.insert("__prefer".into(), serde_json::to_string(&job.runner.prefer).unwrap_or_default());
                    }
                    m
                },
                created_at: now,
            };

            if let Err(e) = self.store.create_execution(&execution) {
                tracing::error!(job_key = %job.key, error = %e, "failed to persist execution");
                continue;
            }

            // 2. Enqueue work item for the runner (always attempt 1 for scheduler-fired jobs)
            let item = job_to_work_item(job, &exec_id_str, fire_at, 1);
            self.runner.queue.write().await.enqueue(item);
            self.runner.work_notify.notify_waiters();

            // 3. Advance the trigger first so we know its new state
            trigger.mark_fired(fire_at, now);

            // 4. Persist job state — use Exhausted for once-triggers so a restart
            //    does not re-arm them and cause double-firing.
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
            let _ = self.store.upsert_job_state(&job_state);

            fired.push(FiredExecution {
                execution_id,
                job_key: job.key.clone(),
                fire_at,
                attempt: 1,
            });

            tracing::info!(
                job_key = %job.key,
                execution_id = %execution_id,
                "execution queued"
            );
        }

        TickResult { fired }
    }
}

#[cfg(test)]
mod tests {
    use chrono::Duration as ChronoDuration;
    use croniq_config::compile::{DeadLetterConfig, RetryConfig, RunnerConfig};
    use croniq_scheduler::{
        misfire::MisfirePolicy,
        schedule::Schedule,
        trigger::TriggerState,
    };
    use croniq_runner::AppState;
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

        let mut scheduler =
            SchedulerLoop::new(triggers, vec![make_job("test:job")], store, Arc::clone(&runner));

        let result = scheduler.tick(Utc::now()).await;
        assert_eq!(result.fired.len(), 1);
        assert_eq!(result.fired[0].job_key, "test:job");
    }

    #[tokio::test]
    async fn tick_skips_future_trigger() {
        let store = make_store();
        let runner = make_runner();
        let mut triggers = HashMap::new();
        triggers.insert("test:job".into(), make_trigger_future("test:job"));

        let mut scheduler =
            SchedulerLoop::new(triggers, vec![make_job("test:job")], store, runner);

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

        let mut scheduler =
            SchedulerLoop::new(triggers, vec![make_job("test:job")], store, runner);

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
    async fn tick_with_empty_triggers() {
        let store = make_store();
        let runner = make_runner();

        let mut scheduler = SchedulerLoop::new(HashMap::new(), vec![], store, runner);

        let result = scheduler.tick(Utc::now()).await;
        assert!(result.fired.is_empty());
    }
}

//! Completion processor: handles job execution results and implements the
//! retry / dead-letter lifecycle.
//!
//! Flow:
//! ```text
//! POST /v1/complete
//!      │
//!      ▼
//! CompletionEvent
//!      │
//!      ▼
//! ExecutionPolicy::evaluate()
//!      │
//!      ├── Success        → mark Completed in store, done
//!      ├── Retry          → create new Execution + WorkItem, mark Failed
//!      ├── DeadLetter     → create DeadLetter record, mark Dead
//!      └── Dropped        → mark Failed (no dead-letter)
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use croniq_bridge::job_to_execution_policy;
use croniq_config::compile::JobConfig;
use croniq_execution::pipeline::{ExecutionOutcome, ExecutionResult};
use croniq_runner::{AppState, CompletionStatus};
use croniq_store::models::{DeadLetter, Execution, ExecutionState};
use uuid::Uuid;

use crate::loader::job_config_from_job_def;
use crate::store::DynStore;

/// Completion event forwarded from the HTTP handler.
#[derive(Debug, Clone)]
pub struct CompletionEvent {
    pub runner_id: String,
    pub execution_id: String,
    pub status: CompletionStatus,
    pub error: Option<String>,
    pub duration_ms: u64,
    pub attempt: u32,
}

/// What the completion processor decided to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessedOutcome {
    /// Execution finished successfully.
    Completed,
    /// Will retry: next attempt enqueued.
    Retrying { attempt: u32 },
    /// Exhausted all retries; moved to dead-letter queue.
    DeadLettered { reason: String },
    /// Exhausted all retries; dead-letter disabled → silently dropped.
    Dropped { reason: String },
    /// Execution ID not found in store (race or already handled).
    NotFound,
}

/// Processes completion events and updates the persistence layer.
pub struct CompletionProcessor {
    jobs: HashMap<String, JobConfig>,
    store: DynStore,
    runner: Arc<AppState>,
}

impl CompletionProcessor {
    pub fn new(
        jobs: Vec<JobConfig>,
        store: DynStore,
        runner: Arc<AppState>,
    ) -> Self {
        let jobs = jobs.into_iter().map(|j| (j.key.clone(), j)).collect();
        Self { jobs, store, runner }
    }

    /// Resolve the `JobConfig` for a job key.
    ///
    /// Fast path: DSL jobs loaded at startup. Slow path: store lookup for jobs
    /// registered via the API at runtime. Returns `None` only if the job truly
    /// does not exist anywhere.
    fn resolve_job_config(&self, job_key: &str) -> Option<JobConfig> {
        if let Some(c) = self.jobs.get(job_key) {
            return Some(c.clone());
        }
        match self.store.get_job_definition(job_key) {
            Ok(Some(def)) => {
                tracing::debug!(job_key = %job_key, "completion: synthesising config from store for API job");
                Some(job_config_from_job_def(&def))
            }
            Ok(None) => None,
            Err(e) => {
                tracing::error!(job_key = %job_key, error = %e, "completion: store error resolving job config");
                None
            }
        }
    }

    /// Process a single completion event.
    pub async fn process(&self, event: CompletionEvent) -> ProcessedOutcome {
        let now = Utc::now();

        // Parse execution ID (store uses UUID)
        let exec_uuid = match Uuid::parse_str(&event.execution_id) {
            Ok(id) => id,
            Err(_) => {
                tracing::warn!(id = %event.execution_id, "invalid execution UUID in completion");
                return ProcessedOutcome::NotFound;
            }
        };

        // Load execution from store
        let execution = match self.store.get_execution(exec_uuid) {
            Ok(Some(e)) => e,
            Ok(None) => {
                tracing::warn!(id = %exec_uuid, "execution not found for completion");
                return ProcessedOutcome::NotFound;
            }
            Err(e) => {
                tracing::error!(id = %exec_uuid, error = %e, "store read error");
                return ProcessedOutcome::NotFound;
            }
        };

        // Find the job config to look up the execution policy.
        // DSL jobs are found in the in-memory map; API jobs fall back to the store.
        let job = match self.resolve_job_config(&execution.job_key) {
            Some(c) => c,
            None => {
                tracing::warn!(job_key = %execution.job_key, "no job config for completion — job not in DSL or store");
                return ProcessedOutcome::NotFound;
            }
        };

        // Cancellations bypass the retry policy entirely
        if event.status == CompletionStatus::Cancelled {
            let _ = self.store.complete_execution(
                exec_uuid,
                ExecutionState::Cancelled,
                Some(event.duration_ms as i64),
                None,
                None,
                now,
            );
            return ProcessedOutcome::Completed;
        }

        let policy = job_to_execution_policy(&job);

        // Build an ExecutionResult for the policy evaluator
        let exec_result = ExecutionResult {
            success: event.status == CompletionStatus::Success,
            error: event.error.clone(),
            duration: Duration::from_millis(event.duration_ms),
            attempt: event.attempt,
        };

        let outcome = policy.evaluate(&exec_result);

        match outcome {
            ExecutionOutcome::Success => {
                let _ = self.store.complete_execution(
                    exec_uuid,
                    ExecutionState::Completed,
                    Some(event.duration_ms as i64),
                    None,
                    None,
                    now,
                );
                tracing::info!(id = %exec_uuid, "execution completed successfully");
                ProcessedOutcome::Completed
            }

            ExecutionOutcome::Retry { next_attempt, delay } => {
                // Mark this attempt as failed
                let _ = self.store.complete_execution(
                    exec_uuid,
                    ExecutionState::Failed,
                    Some(event.duration_ms as i64),
                    event.error.as_deref(),
                    None,
                    now,
                );

                // Create a new execution for the retry
                let retry_fire_at = now
                    + chrono::Duration::from_std(delay).unwrap_or(chrono::Duration::seconds(0));
                let retry_id = Uuid::new_v4();

                let retry_execution = Execution {
                    id: retry_id,
                    job_key: execution.job_key.clone(),
                    fire_at: retry_fire_at,
                    attempt: next_attempt,
                    state: ExecutionState::Queued,
                    runner_id: None,
                    claimed_at: None,
                    started_at: None,
                    completed_at: None,
                    duration_ms: None,
                    error: None,
                    dead_reason: None,
                    metadata: execution.metadata.clone(),
                    created_at: now,
                };

                if let Err(e) = self.store.create_execution(&retry_execution) {
                    tracing::error!(error = %e, "failed to persist retry execution");
                }

                // Enqueue the retry work item
                let item = croniq_runner::WorkItem {
                    execution_id: retry_id.to_string(),
                    job_key: execution.job_key.clone(),
                    fire_at: retry_fire_at,
                    attempt: next_attempt,
                    require: job.runner.require.clone(),
                    prefer: job.runner.prefer.clone(),
                    metadata: serde_json::json!(execution.metadata),
                    timeout: job.timeout.unwrap_or_else(|| "5m".into()),
                };
                self.runner.queue.write().await.enqueue(item);
                self.runner.work_notify.notify_waiters();

                tracing::info!(
                    id = %exec_uuid,
                    retry_id = %retry_id,
                    attempt = next_attempt,
                    "execution will retry"
                );
                ProcessedOutcome::Retrying { attempt: next_attempt }
            }

            ExecutionOutcome::DeadLetter { reason, operator_hint, expires_after } => {
                let _ = self.store.complete_execution(
                    exec_uuid,
                    ExecutionState::Dead,
                    Some(event.duration_ms as i64),
                    event.error.as_deref(),
                    Some(&reason),
                    now,
                );

                let expires_at = expires_after.map(|d| {
                    now + chrono::Duration::from_std(d).unwrap_or(chrono::Duration::zero())
                });

                let hint = operator_hint.unwrap_or_default();
                let dl = DeadLetter {
                    id: Uuid::new_v4(),
                    execution_id: exec_uuid,
                    job_key: execution.job_key.clone(),
                    fire_at: execution.fire_at,
                    attempt: execution.attempt,
                    error: event.error.clone().unwrap_or_default(),
                    dead_reason: if hint.is_empty() {
                        reason.clone()
                    } else {
                        format!("{reason} — {hint}")
                    },
                    metadata: execution.metadata.clone(),
                    created_at: now,
                    expires_at,
                };
                let _ = self.store.add_dead_letter(&dl);

                tracing::warn!(id = %exec_uuid, reason = %reason, "execution dead-lettered");
                crate::notify::notify_failure(
                    &execution.job_key, &event.execution_id,
                    event.error.as_deref().unwrap_or("unknown"), event.attempt, &reason,
                );
                ProcessedOutcome::DeadLettered { reason }
            }

            ExecutionOutcome::Dropped { reason } => {
                let _ = self.store.complete_execution(
                    exec_uuid,
                    ExecutionState::Failed,
                    Some(event.duration_ms as i64),
                    event.error.as_deref(),
                    Some(&reason),
                    now,
                );
                tracing::warn!(id = %exec_uuid, "execution dropped (dead-letter disabled)");
                crate::notify::notify_failure(
                    &execution.job_key, &event.execution_id,
                    event.error.as_deref().unwrap_or("unknown"), event.attempt, &reason,
                );
                ProcessedOutcome::Dropped { reason }
            }

            ExecutionOutcome::Cancelled => {
                let _ = self.store.complete_execution(
                    exec_uuid,
                    ExecutionState::Cancelled,
                    Some(event.duration_ms as i64),
                    None,
                    None,
                    now,
                );
                ProcessedOutcome::Completed
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use croniq_config::compile::{DeadLetterConfig, RetryConfig, RunnerConfig};
    use croniq_runner::AppState;
    use croniq_store::{
        models::{Execution, ExecutionState},
        sqlite::SqliteStore,
    };
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::store::{DynStore, sqlite_store};

    fn make_job(key: &str, max_attempts: u32) -> JobConfig {
        JobConfig {
            key: key.into(),
            namespace: "test".into(),
            name: key.split(':').nth(1).unwrap_or(key).into(),
            variant: None,
            description: None,
            schedule: croniq_config::schedule::CompiledSchedule::Disabled,
            schedule_summary: "disabled".into(),
            timezone: None,
            calendar: None,
            window: None,
            not_before: None,
            not_after: None,
            runner: RunnerConfig::default(),
            retry: RetryConfig {
                strategy: "fixed".into(),
                max_attempts,
                delay: Some("1s".into()),
                jitter: Some(0.0),
                ..RetryConfig::default()
            },
            timeout: Some("5m".into()),
            dead_letter: DeadLetterConfig::default(),
            metadata: HashMap::new(),
            execution_mode: croniq_config::compile::ExecutionMode::default(),
            catch_up: croniq_config::compile::CatchUpPolicy::default(),
            queue_ttl: None,
            max_queue_depth: None,
        }
    }

    fn make_store() -> DynStore {
        sqlite_store(SqliteStore::in_memory().unwrap())
    }

    fn make_runner() -> Arc<AppState> {
        AppState::new()
    }

    fn seed_execution(store: &DynStore, job_key: &str) -> Uuid {
        let id = Uuid::new_v4();
        store
            .create_execution(&Execution {
                id,
                job_key: job_key.into(),
                fire_at: Utc::now(),
                attempt: 1,
                state: ExecutionState::Queued,
                runner_id: None,
                claimed_at: None,
                started_at: None,
                completed_at: None,
                duration_ms: None,
                error: None,
                dead_reason: None,
                metadata: HashMap::new(),
                created_at: Utc::now(),
            })
            .unwrap();
        id
    }

    fn event(execution_id: Uuid, status: CompletionStatus, attempt: u32) -> CompletionEvent {
        CompletionEvent {
            runner_id: "runner-1".into(),
            execution_id: execution_id.to_string(),
            status,
            error: if status == CompletionStatus::Success {
                None
            } else {
                Some("test error".into())
            },
            duration_ms: 500,
            attempt,
        }
    }

    #[tokio::test]
    async fn success_marks_completed() {
        let store = make_store();
        let runner = make_runner();
        let exec_id = seed_execution(&store, "test:job");

        let processor =
            CompletionProcessor::new(vec![make_job("test:job", 3)], Arc::clone(&store), runner);

        let outcome = processor
            .process(event(exec_id, CompletionStatus::Success, 1))
            .await;

        assert_eq!(outcome, ProcessedOutcome::Completed);

        let exec = store.get_execution(exec_id).unwrap().unwrap();
        assert_eq!(exec.state, ExecutionState::Completed);
    }

    #[tokio::test]
    async fn failure_with_retries_remaining_enqueues_retry() {
        let store = make_store();
        let runner = make_runner();
        let exec_id = seed_execution(&store, "test:job");

        let processor = CompletionProcessor::new(
            vec![make_job("test:job", 3)],
            Arc::clone(&store),
            Arc::clone(&runner),
        );

        let outcome = processor
            .process(event(exec_id, CompletionStatus::Failure, 1))
            .await;

        assert!(matches!(outcome, ProcessedOutcome::Retrying { attempt: 2 }));

        // Retry is enqueued
        let q = runner.queue.read().await;
        assert_eq!(q.len(), 1);

        // Original marked Failed
        let exec = store.get_execution(exec_id).unwrap().unwrap();
        assert_eq!(exec.state, ExecutionState::Failed);
    }

    #[tokio::test]
    async fn failure_after_exhaustion_creates_dead_letter() {
        let store = make_store();
        let runner = make_runner();
        let exec_id = seed_execution(&store, "test:job");

        let processor = CompletionProcessor::new(
            vec![make_job("test:job", 1)],
            Arc::clone(&store),
            runner,
        );

        let outcome = processor
            .process(event(exec_id, CompletionStatus::Failure, 1))
            .await;

        assert!(matches!(outcome, ProcessedOutcome::DeadLettered { .. }));

        let exec = store.get_execution(exec_id).unwrap().unwrap();
        assert_eq!(exec.state, ExecutionState::Dead);
    }

    #[tokio::test]
    async fn failure_with_dead_letter_disabled_drops() {
        let store = make_store();
        let runner = make_runner();
        let exec_id = seed_execution(&store, "test:job");

        let mut job = make_job("test:job", 1);
        job.dead_letter.enabled = false;

        let processor = CompletionProcessor::new(vec![job], Arc::clone(&store), runner);

        let outcome = processor
            .process(event(exec_id, CompletionStatus::Failure, 1))
            .await;

        assert!(matches!(outcome, ProcessedOutcome::Dropped { .. }));
    }

    #[tokio::test]
    async fn api_job_without_dsl_entry_uses_store_fallback() {
        let store = make_store();
        let runner = make_runner();

        // Seed a JobDefinition in the store only — no DSL entry
        let job_key = "api:noop";
        store
            .create_job_definition(&croniq_store::models::JobDefinition {
                job_key: job_key.into(),
                description: None,
                assigned_runner_id: None,
                is_active: true,
                metadata: HashMap::new(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
                timeout: Some("1m".into()),
                max_retries: Some(1), // one attempt → dead-letter on first failure
                dead_letter_enabled: Some(true),
            })
            .unwrap();

        let exec_id = seed_execution(&store, job_key);

        // Processor has an empty DSL jobs list — must fall back to store
        let processor = CompletionProcessor::new(vec![], Arc::clone(&store), runner);

        let outcome = processor
            .process(event(exec_id, CompletionStatus::Failure, 1))
            .await;

        assert!(
            matches!(outcome, ProcessedOutcome::DeadLettered { .. }),
            "expected DeadLettered, got {outcome:?}"
        );
        let exec = store.get_execution(exec_id).unwrap().unwrap();
        assert_eq!(exec.state, ExecutionState::Dead);
    }

    #[tokio::test]
    async fn api_job_unknown_to_store_returns_not_found() {
        let store = make_store();
        let runner = make_runner();
        let exec_id = seed_execution(&store, "ghost:job");

        // No DSL entry and no JobDefinition in store → NotFound
        let processor = CompletionProcessor::new(vec![], Arc::clone(&store), runner);

        let outcome = processor
            .process(event(exec_id, CompletionStatus::Failure, 1))
            .await;

        assert_eq!(outcome, ProcessedOutcome::NotFound);
    }

    #[tokio::test]
    async fn invalid_execution_id_returns_not_found() {
        let store = make_store();
        let runner = make_runner();

        let processor = CompletionProcessor::new(vec![], Arc::clone(&store), runner);

        let outcome = processor
            .process(CompletionEvent {
                runner_id: "r1".into(),
                execution_id: "not-a-valid-uuid".into(),
                status: CompletionStatus::Success,
                error: None,
                duration_ms: 100,
                attempt: 1,
            })
            .await;

        assert_eq!(outcome, ProcessedOutcome::NotFound);
    }

    #[tokio::test]
    async fn unknown_execution_id_returns_not_found() {
        let store = make_store();
        let runner = make_runner();

        let processor = CompletionProcessor::new(vec![], Arc::clone(&store), runner);

        let outcome = processor
            .process(CompletionEvent {
                runner_id: "r1".into(),
                execution_id: Uuid::new_v4().to_string(),
                status: CompletionStatus::Success,
                error: None,
                duration_ms: 100,
                attempt: 1,
            })
            .await;

        assert_eq!(outcome, ProcessedOutcome::NotFound);
    }

    #[tokio::test]
    async fn cancelled_status_marks_cancelled() {
        let store = make_store();
        let runner = make_runner();
        let exec_id = seed_execution(&store, "test:job");

        let processor = CompletionProcessor::new(
            vec![make_job("test:job", 3)],
            Arc::clone(&store),
            runner,
        );

        let mut ev = event(exec_id, CompletionStatus::Cancelled, 1);
        ev.error = None;
        let outcome = processor.process(ev).await;

        assert_eq!(outcome, ProcessedOutcome::Completed);
        let exec = store.get_execution(exec_id).unwrap().unwrap();
        assert_eq!(exec.state, ExecutionState::Cancelled);
    }
}

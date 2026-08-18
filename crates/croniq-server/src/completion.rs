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
    /// Completion for an ephemeral execution (issue #263): the job runs in
    /// `ephemeral` mode so no execution row was ever persisted. The result
    /// is acknowledged as a no-op — there is no retry/dead-letter lifecycle
    /// to drive — rather than mis-reported as `NotFound`.
    Ephemeral,
    /// Execution ID not found in store (race or already handled).
    NotFound,
    /// Late completion ignored (issue #374): the watchdog requeued the
    /// claim before this completion arrived — the row is `queued` again
    /// or already re-claimed by another runner — so the store's
    /// compare-and-swap refused the write. No retry or dead-letter
    /// lifecycle runs; the re-run owns the execution now.
    Stale,
}

/// Processes completion events and updates the persistence layer.
pub struct CompletionProcessor {
    jobs: HashMap<String, JobConfig>,
    store: DynStore,
    runner: Arc<AppState>,
    /// Failure-alert configuration (issue #140). Empty default means no
    /// rules fire — keeping the old behaviour for installs without an
    /// `alerts {}` block. The `CRONIQ_ON_FAILURE_CMD` back-compat path
    /// is layered in at boot via [`crate::alerts::merge_legacy_env_hook`].
    alerts: croniq_config::compile::AlertsConfig,
    /// In-process throttle state keyed by `(rule_name, job_key)`. Seeded
    /// at boot from `alert_deliveries.fired_at` so a server restart
    /// doesn't reset suppression windows.
    alert_throttle: crate::alerts::ThrottleMap,
    /// Email sender used by the `email` alert channel (issue #140
    /// PR-3). Defaults to `NoopSender` — real delivery requires
    /// `CRONIQ_SMTP_URL` + `CRONIQ_SMTP_FROM` and the `smtp` cargo
    /// feature.
    email_sender: Arc<dyn crate::email::EmailSender>,
}

impl CompletionProcessor {
    pub fn new(jobs: Vec<JobConfig>, store: DynStore, runner: Arc<AppState>) -> Self {
        Self::with_alerts(
            jobs,
            store,
            runner,
            croniq_config::compile::AlertsConfig::default(),
            crate::alerts::empty_throttle_map(),
            crate::email::default_sender(),
        )
    }

    /// Construct with an explicit alerts config + throttle map + email
    /// sender.
    ///
    /// Used by `main.rs` to wire the Croniqfile `alerts {}` block (plus
    /// any synthesised legacy env-var rule) into the failure pipeline.
    /// Tests use `new()` to keep the no-alerts behaviour.
    pub fn with_alerts(
        jobs: Vec<JobConfig>,
        store: DynStore,
        runner: Arc<AppState>,
        alerts: croniq_config::compile::AlertsConfig,
        alert_throttle: crate::alerts::ThrottleMap,
        email_sender: Arc<dyn crate::email::EmailSender>,
    ) -> Self {
        let jobs = jobs.into_iter().map(|j| (j.key.clone(), j)).collect();
        Self {
            jobs,
            store,
            runner,
            alerts,
            alert_throttle,
            email_sender,
        }
    }

    /// Resolve the `JobConfig` for a job key.
    ///
    /// Fast path: DSL jobs loaded at startup. Slow path: store lookup for jobs
    /// registered via the API at runtime. Returns `None` only if the job truly
    /// does not exist anywhere.
    /// Dispatch the configured alert rules against a permanent
    /// failure event. Replaces the old single-shot
    /// `notify::notify_failure()` env-var hook — back-compat for that
    /// env var lives in [`crate::alerts::merge_legacy_env_hook`] at
    /// boot, so this method needs no special-cases.
    async fn fire_alerts(&self, job_key: &str, event: &CompletionEvent, reason: &str) {
        // Fast-path: no rules configured means no work to do. Important
        // because the evaluator otherwise walks `alerts.rules` and may
        // open a store cursor — wasteful on the common path.
        if self.alerts.rules.is_empty() {
            return;
        }
        let ctx = crate::alerts::FailureContext {
            job_key: job_key.to_string(),
            execution_id: event.execution_id.clone(),
            error: event.error.clone().unwrap_or_else(|| "unknown".to_string()),
            attempt: event.attempt,
            reason: reason.to_string(),
        };
        let _ = crate::alerts::evaluate_failure(
            &self.alerts,
            &ctx,
            &self.alert_throttle,
            &self.store,
            &self.email_sender,
        )
        .await;
    }

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
    #[tracing::instrument(
        skip(self),
        fields(
            execution_id = %event.execution_id,
            runner_id = %event.runner_id,
            attempt = event.attempt,
            duration_ms = event.duration_ms,
        ),
    )]
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
                // Ephemeral executions (issue #263) intentionally have no
                // persisted row, so the store miss here is *expected* for
                // them. If this id was dispatched as ephemeral, acknowledge
                // it as a no-op instead of warning about a "missing"
                // execution — there's no retry/dead-letter lifecycle to run.
                if self.runner.take_ephemeral(&event.execution_id).await {
                    tracing::debug!(
                        id = %exec_uuid,
                        status = ?event.status,
                        "ephemeral execution completed (not persisted)"
                    );
                    return ProcessedOutcome::Ephemeral;
                }
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

        // Fence completions on the reporting runner: after a watchdog
        // requeue the row is queued again (or claimed by another runner),
        // and the store-level compare-and-swap must reject this event.
        let fence = Some(event.runner_id.as_str());

        // Cancellations bypass the retry policy entirely
        if event.status == CompletionStatus::Cancelled {
            match self.store.complete_execution(
                exec_uuid,
                fence,
                ExecutionState::Cancelled,
                Some(event.duration_ms as i64),
                None,
                None,
                now,
            ) {
                // Expected on the common cancel path: the API handler
                // already flipped the row to `cancelled` before pushing
                // the cancel to the runner, so the CAS misses here.
                Ok(false) => {
                    tracing::debug!(id = %exec_uuid, "cancel completion on already-terminal row")
                }
                Ok(true) => {}
                Err(e) => tracing::error!(id = %exec_uuid, error = %e, "store write error"),
            }
            // The claimed → cancelled transition frees a per-job concurrency
            // slot (issue #278) — wake long-polling runners so a blocked
            // guarded item is re-evaluated promptly.
            self.runner.work_notify.notify_waiters();
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

        let processed = match outcome {
            ExecutionOutcome::Success => {
                match self.store.complete_execution(
                    exec_uuid,
                    fence,
                    ExecutionState::Completed,
                    Some(event.duration_ms as i64),
                    None,
                    None,
                    now,
                ) {
                    Ok(true) => {
                        tracing::info!(id = %exec_uuid, "execution completed successfully");
                        ProcessedOutcome::Completed
                    }
                    Ok(false) => {
                        tracing::warn!(
                            id = %exec_uuid,
                            runner_id = %event.runner_id,
                            "late completion ignored — execution was requeued by the watchdog"
                        );
                        ProcessedOutcome::Stale
                    }
                    Err(e) => {
                        tracing::error!(id = %exec_uuid, error = %e, "store write error");
                        ProcessedOutcome::Completed
                    }
                }
            }

            ExecutionOutcome::Retry {
                next_attempt,
                delay,
            } => {
                // Mark this attempt as failed. A CAS miss means the
                // watchdog already requeued this claim — the re-run owns
                // the execution, so no retry must be spawned for the
                // late event.
                match self.store.complete_execution(
                    exec_uuid,
                    fence,
                    ExecutionState::Failed,
                    Some(event.duration_ms as i64),
                    event.error.as_deref(),
                    None,
                    now,
                ) {
                    Ok(true) => {}
                    Ok(false) => {
                        tracing::warn!(
                            id = %exec_uuid,
                            runner_id = %event.runner_id,
                            "late completion ignored — execution was requeued by the watchdog"
                        );
                        self.runner.work_notify.notify_waiters();
                        return ProcessedOutcome::Stale;
                    }
                    Err(e) => {
                        tracing::error!(id = %exec_uuid, error = %e, "store write error")
                    }
                }

                // Create a new execution for the retry
                let retry_fire_at =
                    now + chrono::Duration::from_std(delay).unwrap_or(chrono::Duration::seconds(0));
                let retry_id = Uuid::new_v4();

                let retry_execution = Execution {
                    id: retry_id,
                    job_key: execution.job_key.clone(),
                    fire_at: retry_fire_at,
                    // Carry the original logical fire time forward: fire_at
                    // drifts to now+backoff on each retry, scheduled_for stays
                    // pinned so time-coupled job logic sees the same instant.
                    scheduled_for: execution.scheduled_for,
                    attempt: next_attempt,
                    state: ExecutionState::Queued,
                    runner_id: None,
                    claimed_at: None,
                    started_at: None,
                    completed_at: None,
                    duration_ms: None,
                    error: None,
                    dead_reason: None,
                    // Carry the trigger idempotency key forward (issue #279)
                    // so a repeat trigger keeps coalescing while the retry
                    // is in flight.
                    idempotency_key: execution.idempotency_key.clone(),
                    metadata: execution.metadata.clone(),
                    created_at: now,
                };

                match self.store.create_execution(&retry_execution) {
                    Ok(()) => {
                        // Enqueue the retry work item
                        let item = croniq_runner::WorkItem {
                            execution_id: retry_id.to_string(),
                            job_key: execution.job_key.clone(),
                            fire_at: retry_fire_at,
                            scheduled_for: execution.scheduled_for,
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
                        ProcessedOutcome::Retrying {
                            attempt: next_attempt,
                        }
                    }
                    Err(e) => {
                        // The retry row could not be persisted. Enqueueing
                        // anyway would hand a runner an execution_id with no
                        // backing row — its status updates, events and final
                        // completion would all target a nonexistent execution
                        // and the attempt would vanish without history. Skip
                        // the enqueue and terminate the chain instead:
                        // dead-letter the original execution (best-effort,
                        // when enabled) so the lost retry stays
                        // operator-visible and replayable.
                        tracing::error!(
                            id = %exec_uuid,
                            error = %e,
                            "failed to persist retry execution — retry not enqueued"
                        );
                        let reason =
                            format!("retry attempt {next_attempt} could not be persisted: {e}");
                        let mut dead_lettered = false;
                        if policy.dead_letter.enabled {
                            let dl = DeadLetter {
                                id: Uuid::new_v4(),
                                execution_id: exec_uuid,
                                job_key: execution.job_key.clone(),
                                fire_at: execution.fire_at,
                                scheduled_for: execution.scheduled_for,
                                attempt: execution.attempt,
                                error: event.error.clone().unwrap_or_default(),
                                dead_reason: reason.clone(),
                                metadata: execution.metadata.clone(),
                                created_at: now,
                                // The helper yields None for retention 0
                                // ("keep forever"), matching purge_expired's
                                // NULL semantics.
                                expires_at: policy.dead_letter.expires_at(now),
                            };
                            match self.store.complete_as_dead(
                                exec_uuid,
                                fence,
                                Some(event.duration_ms as i64),
                                event.error.as_deref(),
                                &dl,
                                now,
                            ) {
                                Ok(true) => dead_lettered = true,
                                // We just marked the row failed ourselves,
                                // so a CAS miss here means someone raced us
                                // to a different terminal state — leave it.
                                Ok(false) => tracing::warn!(
                                    id = %exec_uuid,
                                    "dead-letter fallback skipped — execution no longer owned by this completion"
                                ),
                                Err(e2) => tracing::error!(
                                    id = %exec_uuid,
                                    error = %e2,
                                    "dead-letter fallback after failed retry persist also failed"
                                ),
                            }
                        }
                        self.fire_alerts(&execution.job_key, &event, &reason).await;
                        if dead_lettered {
                            ProcessedOutcome::DeadLettered { reason }
                        } else {
                            ProcessedOutcome::Dropped { reason }
                        }
                    }
                }
            }

            ExecutionOutcome::DeadLetter {
                reason,
                operator_hint,
                expires_after,
            } => {
                let expires_at = expires_after.map(|d| {
                    now + chrono::Duration::from_std(d).unwrap_or(chrono::Duration::zero())
                });

                let hint = operator_hint.unwrap_or_default();
                let dead_reason = if hint.is_empty() {
                    reason.clone()
                } else {
                    format!("{reason} — {hint}")
                };
                let dl = DeadLetter {
                    id: Uuid::new_v4(),
                    execution_id: exec_uuid,
                    job_key: execution.job_key.clone(),
                    fire_at: execution.fire_at,
                    scheduled_for: execution.scheduled_for,
                    attempt: execution.attempt,
                    error: event.error.clone().unwrap_or_default(),
                    dead_reason,
                    metadata: execution.metadata.clone(),
                    created_at: now,
                    expires_at,
                };

                match self.store.complete_as_dead(
                    exec_uuid,
                    fence,
                    Some(event.duration_ms as i64),
                    event.error.as_deref(),
                    &dl,
                    now,
                ) {
                    Ok(true) => {
                        tracing::warn!(id = %exec_uuid, reason = %reason, "execution dead-lettered");
                        self.fire_alerts(&execution.job_key, &event, &reason).await;
                        ProcessedOutcome::DeadLettered { reason }
                    }
                    Ok(false) => {
                        // The watchdog requeued this claim before the
                        // completion arrived — the re-run owns the
                        // execution, so neither the dead letter nor the
                        // failure alert may fire for the late event.
                        tracing::warn!(
                            id = %exec_uuid,
                            runner_id = %event.runner_id,
                            "late completion ignored — execution was requeued by the watchdog"
                        );
                        ProcessedOutcome::Stale
                    }
                    Err(e) => {
                        tracing::error!(
                            id = %exec_uuid,
                            error = %e,
                            "failed to record dead-lettered execution — execution row may now be inconsistent with dead_letters table"
                        );
                        tracing::warn!(id = %exec_uuid, reason = %reason, "execution dead-lettered");
                        self.fire_alerts(&execution.job_key, &event, &reason).await;
                        ProcessedOutcome::DeadLettered { reason }
                    }
                }
            }

            ExecutionOutcome::Dropped { reason } => {
                match self.store.complete_execution(
                    exec_uuid,
                    fence,
                    ExecutionState::Failed,
                    Some(event.duration_ms as i64),
                    event.error.as_deref(),
                    Some(&reason),
                    now,
                ) {
                    Ok(false) => {
                        tracing::warn!(
                            id = %exec_uuid,
                            runner_id = %event.runner_id,
                            "late completion ignored — execution was requeued by the watchdog"
                        );
                        ProcessedOutcome::Stale
                    }
                    Ok(true) | Err(_) => {
                        tracing::warn!(id = %exec_uuid, "execution dropped (dead-letter disabled)");
                        self.fire_alerts(&execution.job_key, &event, &reason).await;
                        ProcessedOutcome::Dropped { reason }
                    }
                }
            }

            ExecutionOutcome::Cancelled => {
                let _ = self.store.complete_execution(
                    exec_uuid,
                    fence,
                    ExecutionState::Cancelled,
                    Some(event.duration_ms as i64),
                    None,
                    None,
                    now,
                );
                ProcessedOutcome::Completed
            }
        };

        // Every processed completion moves an execution out of `claimed`,
        // freeing a per-job concurrency slot (issue #278). Wake long-polling
        // runners so a guarded item blocked on that slot is re-evaluated
        // without waiting for the poll timeout. (The retry arm re-enqueues
        // and already notified above; an extra wake is harmless.)
        self.runner.work_notify.notify_waiters();

        processed
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
            keep_last: None,
            max_concurrent: None,
            tags: vec![],
        }
    }

    fn make_store() -> DynStore {
        sqlite_store(SqliteStore::in_memory().unwrap())
    }

    fn make_runner() -> Arc<AppState> {
        AppState::new()
    }

    /// Persist a queued execution and claim it for `runner-1` — the runner
    /// every [`event`] reports as — so completions pass the store's
    /// claimed-state CAS guard like a real dispatched execution would.
    fn seed_execution(store: &DynStore, job_key: &str) -> Uuid {
        let id = Uuid::new_v4();
        store
            .create_execution(&Execution {
                id,
                job_key: job_key.into(),
                fire_at: Utc::now(),
                scheduled_for: Utc::now(),
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
                created_at: Utc::now(),
            })
            .unwrap();
        store.claim_execution(id, "runner-1", Utc::now()).unwrap();
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

        let processor =
            CompletionProcessor::new(vec![make_job("test:job", 1)], Arc::clone(&store), runner);

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

    /// Store test double: delegates every call to an inner store but fails
    /// `create_execution` once armed. Lets tests drive the
    /// retry-persist-failure path without a real database error.
    mod failing_store {
        use std::collections::HashMap;
        use std::sync::atomic::{AtomicBool, Ordering};

        use chrono::{DateTime, Utc};
        use croniq_store::models::*;
        use croniq_store::traits::*;
        use uuid::Uuid;

        use crate::store::DynStore;

        pub struct FailingCreateStore {
            pub inner: DynStore,
            pub fail_create: AtomicBool,
        }

        impl FailingCreateStore {
            pub fn arm(&self) {
                self.fail_create.store(true, Ordering::SeqCst);
            }
        }

        macro_rules! delegate {
            ($($name:ident($($arg:ident: $ty:ty),*) -> $ret:ty;)+) => {
                $(fn $name(&self, $($arg: $ty),*) -> Result<$ret, StoreError> {
                    self.inner.$name($($arg),*)
                })+
            };
        }

        impl JobStore for FailingCreateStore {
            delegate! {
                get_job_state(job_key: &str) -> Option<JobState>;
                upsert_job_state(state: &JobState) -> ();
                list_job_states() -> Vec<JobState>;
                delete_job_state(job_key: &str) -> ();
            }
        }

        #[allow(clippy::too_many_arguments)] // delegated complete_execution
        impl ExecutionStore for FailingCreateStore {
            fn create_execution(&self, execution: &Execution) -> Result<(), StoreError> {
                if self.fail_create.load(Ordering::SeqCst) {
                    return Err(StoreError::Database(
                        "injected create_execution failure".into(),
                    ));
                }
                self.inner.create_execution(execution)
            }
            delegate! {
                create_execution_and_advance_job_state(execution: &Execution, job_state: &JobState) -> ();
                get_execution(id: Uuid) -> Option<Execution>;
                claim_execution(id: Uuid, runner_id: &str, now: DateTime<Utc>) -> Execution;
                complete_execution(id: Uuid, runner_id: Option<&str>, state: ExecutionState, duration_ms: Option<i64>, error: Option<&str>, dead_reason: Option<&str>, now: DateTime<Utc>) -> bool;
                find_queued_executions(capabilities: &[String], limit: u32) -> Vec<Execution>;
                list_executions(filter: &ExecutionFilter) -> Vec<Execution>;
                list_claimed_older_than(cutoff: DateTime<Utc>, limit: u32) -> Vec<Execution>;
                find_execution_by_idempotency_key(job_key: &str, idempotency_key: &str, window_start: DateTime<Utc>) -> Option<Execution>;
                requeue_abandoned(runner_id: &str, now: DateTime<Utc>) -> Vec<Uuid>;
                requeue_if_claimed(id: Uuid, now: DateTime<Utc>) -> bool;
                cancel_execution(id: Uuid, now: DateTime<Utc>) -> ();
                count_by_state() -> HashMap<ExecutionState, u64>;
                count_executions_in_states(job_key: &str, states: &[ExecutionState]) -> u64;
                job_execution_metrics() -> Vec<JobExecutionMetrics>;
                prune_executions_older_than(cutoff: DateTime<Utc>, limit: u32) -> u64;
                prune_executions_keep_last(job_key: &str, keep_last: u32, limit: u32) -> u64;
            }
        }

        impl RunnerStore for FailingCreateStore {
            delegate! {
                upsert_runner(runner: &Runner) -> ();
                get_runner(runner_id: &str) -> Option<Runner>;
                list_runners() -> Vec<Runner>;
                remove_runner(runner_id: &str) -> ();
                update_poll(runner_id: &str, inflight: &[Uuid], now: DateTime<Utc>) -> ();
                runner_identity_bind(runner_id: &str, owner_id: &str, now: DateTime<Utc>) -> String;
                runner_identity_owner(runner_id: &str) -> Option<String>;
                runner_identity_release(runner_id: &str) -> ();
            }
        }

        impl DeadLetterStore for FailingCreateStore {
            delegate! {
                add_dead_letter(dl: &DeadLetter) -> ();
                complete_as_dead(execution_id: Uuid, runner_id: Option<&str>, duration_ms: Option<i64>, error: Option<&str>, dead_letter: &DeadLetter, now: DateTime<Utc>) -> bool;
                replay_dead_letter(dead_letter_id: Uuid, execution: &Execution) -> ();
                get_dead_letter(id: Uuid) -> Option<DeadLetter>;
                list_dead_letters(filter: &DeadLetterFilter) -> Vec<DeadLetter>;
                remove_dead_letter(id: Uuid) -> ();
                remove_dead_letters(ids: &[Uuid]) -> u64;
                clear_dead_letters(job_key: Option<&str>) -> u64;
                purge_expired(now: DateTime<Utc>) -> u64;
            }
        }

        impl AuthStore for FailingCreateStore {
            delegate! {
                create_client(client: &ApiClient) -> ();
                get_client(client_id: &str) -> Option<ApiClient>;
                list_clients() -> Vec<ApiClient>;
                delete_client(client_id: &str) -> ();
                create_api_key(key: &ApiKey) -> ();
                find_api_key_by_hash(key_hash: &str) -> Option<ApiKey>;
                revoke_api_key(key_id: &str, now: DateTime<Utc>) -> ();
                list_api_keys(client_id: &str) -> Vec<ApiKey>;
                get_credentials(username: &str) -> Option<PasswordCredential>;
                upsert_credentials(cred: &PasswordCredential) -> ();
                create_refresh_token(token: &RefreshToken) -> ();
                validate_refresh_token(token_hash: &str) -> Option<RefreshToken>;
                revoke_refresh_token(token_hash: &str, now: DateTime<Utc>) -> ();
                users_create(user: &User) -> ();
                users_get_by_id(user_id: &str) -> Option<User>;
                users_get_by_username(username: &str) -> Option<User>;
                users_list() -> Vec<User>;
                users_update(user: &User) -> ();
                users_delete(user_id: &str) -> ();
                users_set_last_login(user_id: &str, at: DateTime<Utc>) -> ();
                users_count_active_admins() -> u64;
                users_token_generation(user_id: &str) -> Option<i64>;
                users_bump_token_generation(user_id: &str) -> ();
                invitations_create(invite: &Invitation) -> ();
                invitations_get(invitation_id: &str) -> Option<Invitation>;
                invitations_get_by_token_hash(token_hash: &str) -> Option<Invitation>;
                invitations_list() -> Vec<Invitation>;
                invitations_mark_accepted(invitation_id: &str, at: DateTime<Utc>) -> ();
                invitations_revoke(invitation_id: &str, at: DateTime<Utc>) -> ();
                password_resets_create(reset: &PasswordReset) -> ();
                password_resets_get_by_token_hash(token_hash: &str) -> Option<PasswordReset>;
                password_resets_mark_used(reset_id: &str, at: DateTime<Utc>) -> ();
                totp_upsert(secret: &TotpSecret) -> ();
                totp_get(user_id: &str) -> Option<TotpSecret>;
                totp_set_enabled(user_id: &str, enabled: bool, confirmed_at: Option<DateTime<Utc>>) -> ();
                totp_delete(user_id: &str) -> ();
                recovery_codes_replace_all(user_id: &str, codes: &[RecoveryCode]) -> ();
                recovery_codes_find_unused(user_id: &str, code_hash: &str) -> Option<RecoveryCode>;
                recovery_codes_mark_used(code_id: &str, at: DateTime<Utc>) -> ();
                recovery_codes_count_unused(user_id: &str) -> u64;
                pat_create(pat: &PersonalAccessToken) -> ();
                pat_find_by_hash(token_hash: &str) -> Option<PersonalAccessToken>;
                pat_list(user_id: &str) -> Vec<PersonalAccessToken>;
                pat_revoke(token_id: &str, at: DateTime<Utc>) -> ();
                pat_touch_last_used(token_id: &str, at: DateTime<Utc>) -> ();
                oidc_link(identity: &OidcIdentity) -> ();
                oidc_get_by_subject(provider: &str, subject: &str) -> Option<OidcIdentity>;
                oidc_touch_last_login(provider: &str, subject: &str, at: DateTime<Utc>) -> ();
                oidc_pending_create(pending: &OidcPendingLogin) -> ();
                oidc_pending_take(state: &str) -> Option<OidcPendingLogin>;
                oidc_pending_purge_expired(now: DateTime<Utc>) -> u64;
                audit_log(event: &AuditEvent) -> ();
                audit_list(filter: &AuditFilter) -> Vec<AuditEvent>;
            }
        }

        impl JobDefinitionStore for FailingCreateStore {
            delegate! {
                create_job_definition(job: &JobDefinition) -> ();
                get_job_definition(job_key: &str) -> Option<JobDefinition>;
                list_job_definitions() -> Vec<JobDefinition>;
                delete_job_definition(job_key: &str) -> ();
            }
        }

        impl TriggerDefinitionStore for FailingCreateStore {
            delegate! {
                create_trigger(trigger: &TriggerDefinition) -> ();
                get_trigger(trigger_id: &str) -> Option<TriggerDefinition>;
                list_triggers(job_key: Option<&str>) -> Vec<TriggerDefinition>;
                delete_trigger(trigger_id: &str) -> ();
                update_trigger(trigger: &TriggerDefinition) -> bool;
            }
        }

        impl CalendarDefinitionStore for FailingCreateStore {
            delegate! {
                create_calendar(cal: &CalendarDefinition) -> ();
                get_calendar(calendar_id: &str) -> Option<CalendarDefinition>;
                list_calendars() -> Vec<CalendarDefinition>;
                delete_calendar(calendar_id: &str) -> ();
            }
        }

        impl DslAdoptionStore for FailingCreateStore {
            delegate! {
                insert_adoption(adoption: &DslAdoption) -> ();
                delete_adoption(resource_type: &str, resource_key: &str) -> bool;
                is_adopted(resource_type: &str, resource_key: &str) -> bool;
                list_adoptions(resource_type: &str) -> Vec<DslAdoption>;
            }
        }

        impl ExecutionLogStore for FailingCreateStore {
            delegate! {
                append_log(entry: &ExecutionLogEntry) -> ();
                append_logs_batch(entries: &[ExecutionLogEntry]) -> ();
                read_logs(execution_id: Uuid, limit: u32) -> Vec<ExecutionLogEntry>;
            }
        }

        impl AlertStore for FailingCreateStore {
            delegate! {
                record_alert_delivery(delivery: &AlertDelivery) -> ();
                list_alert_deliveries(filter: &AlertDeliveryFilter) -> Vec<AlertDelivery>;
                get_alert_delivery(delivery_id: &str) -> Option<AlertDelivery>;
                last_alert_fire_at(rule_name: &str, job_key: &str) -> Option<DateTime<Utc>>;
                upsert_alert_rule_override(ov: &AlertRuleOverride) -> ();
                get_alert_rule_override(rule_name: &str) -> Option<AlertRuleOverride>;
                list_alert_rule_overrides() -> Vec<AlertRuleOverride>;
                delete_alert_rule_override(rule_name: &str) -> bool;
                delete_expired_alert_rule_overrides(now: DateTime<Utc>) -> Vec<String>;
                prune_alert_rule_overrides(valid_rule_names: &[String]) -> Vec<String>;
            }
        }

        impl MaintenanceStore for FailingCreateStore {
            delegate! {
                get_maintenance() -> MaintenanceState;
                set_maintenance(state: &MaintenanceState) -> ();
            }
        }

        impl croniq_store::traits::Store for FailingCreateStore {}
    }

    fn make_failing_store() -> (Arc<failing_store::FailingCreateStore>, DynStore) {
        let failing = Arc::new(failing_store::FailingCreateStore {
            inner: make_store(),
            fail_create: std::sync::atomic::AtomicBool::new(false),
        });
        let store: DynStore = failing.clone();
        (failing, store)
    }

    #[tokio::test]
    async fn retry_persist_failure_skips_enqueue_and_dead_letters() {
        use croniq_store::models::DeadLetterFilter;

        let (failing, store) = make_failing_store();
        let runner = make_runner();
        let exec_id = seed_execution(&store, "test:job");
        failing.arm();

        let processor = CompletionProcessor::new(
            vec![make_job("test:job", 3)],
            Arc::clone(&store),
            Arc::clone(&runner),
        );

        let outcome = processor
            .process(event(exec_id, CompletionStatus::Failure, 1))
            .await;

        // Retries remained, but the retry row could not be persisted → the
        // chain terminates as dead-lettered instead of retrying.
        assert!(
            matches!(outcome, ProcessedOutcome::DeadLettered { ref reason } if reason.contains("could not be persisted")),
            "expected DeadLettered, got {outcome:?}"
        );

        // No ghost work item: a runner must never claim an execution_id
        // without a backing row.
        assert_eq!(runner.queue.read().await.len(), 0);

        // The original execution is terminal and the dead letter replayable.
        let exec = store.get_execution(exec_id).unwrap().unwrap();
        assert_eq!(exec.state, ExecutionState::Dead);
        let dls = store
            .list_dead_letters(&DeadLetterFilter::default())
            .unwrap();
        assert_eq!(dls.len(), 1);
        assert_eq!(dls[0].execution_id, exec_id);
        // Default 30d retention stamps an expiry (None is reserved for
        // retention 0 = keep forever).
        assert!(dls[0].expires_at.is_some());
    }

    #[tokio::test]
    async fn retry_persist_failure_with_dead_letter_disabled_drops() {
        let (failing, store) = make_failing_store();
        let runner = make_runner();
        let exec_id = seed_execution(&store, "test:job");
        failing.arm();

        let mut job = make_job("test:job", 3);
        job.dead_letter.enabled = false;

        let processor =
            CompletionProcessor::new(vec![job], Arc::clone(&store), Arc::clone(&runner));

        let outcome = processor
            .process(event(exec_id, CompletionStatus::Failure, 1))
            .await;

        assert!(
            matches!(outcome, ProcessedOutcome::Dropped { .. }),
            "expected Dropped, got {outcome:?}"
        );
        assert_eq!(runner.queue.read().await.len(), 0);

        // The failed attempt itself is still recorded.
        let exec = store.get_execution(exec_id).unwrap().unwrap();
        assert_eq!(exec.state, ExecutionState::Failed);
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
                dead_letter_retention: None,
                dead_letter_operator_hint: None,
                dead_letter_replay_max_age: None,
                tags: vec![],
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
    async fn ephemeral_completion_without_row_is_acknowledged() {
        // Issue #263: an ephemeral job persists no execution row, so the
        // store miss is expected. A completion whose id was recorded as a
        // tracked ephemeral dispatch must be acknowledged (no warn, no
        // NotFound) and the id consumed.
        let store = make_store();
        let runner = make_runner();
        let id = Uuid::new_v4();
        runner
            .record_ephemeral(&id.to_string(), Utc::now(), chrono::Duration::hours(1))
            .await;

        let processor = CompletionProcessor::new(vec![], Arc::clone(&store), Arc::clone(&runner));
        let outcome = processor
            .process(event(id, CompletionStatus::Success, 1))
            .await;

        assert_eq!(outcome, ProcessedOutcome::Ephemeral);
        // Consumed: a duplicate completion would now read as NotFound.
        assert!(
            !runner
                .ephemeral_inflight
                .read()
                .await
                .contains_key(&id.to_string())
        );
    }

    #[tokio::test]
    async fn untracked_missing_execution_still_not_found() {
        // A store miss for an id that was *not* a tracked ephemeral dispatch
        // remains NotFound — the #263 fix must not mask genuinely lost work.
        let store = make_store();
        let runner = make_runner();
        let processor = CompletionProcessor::new(vec![], Arc::clone(&store), runner);

        let outcome = processor
            .process(event(Uuid::new_v4(), CompletionStatus::Success, 1))
            .await;

        assert_eq!(outcome, ProcessedOutcome::NotFound);
    }

    #[tokio::test]
    async fn cancelled_status_marks_cancelled() {
        let store = make_store();
        let runner = make_runner();
        let exec_id = seed_execution(&store, "test:job");

        let processor =
            CompletionProcessor::new(vec![make_job("test:job", 3)], Arc::clone(&store), runner);

        let mut ev = event(exec_id, CompletionStatus::Cancelled, 1);
        ev.error = None;
        let outcome = processor.process(ev).await;

        assert_eq!(outcome, ProcessedOutcome::Completed);
        let exec = store.get_execution(exec_id).unwrap().unwrap();
        assert_eq!(exec.state, ExecutionState::Cancelled);
    }

    // ─── Late completions after a watchdog requeue (issue #374) ─────

    #[tokio::test]
    async fn late_success_after_requeue_is_ignored() {
        let store = make_store();
        let runner = make_runner();
        let exec_id = seed_execution(&store, "test:job");

        // Watchdog requeues the stale claim before the completion arrives.
        assert!(store.requeue_if_claimed(exec_id, Utc::now()).unwrap());

        let processor =
            CompletionProcessor::new(vec![make_job("test:job", 3)], Arc::clone(&store), runner);
        let outcome = processor
            .process(event(exec_id, CompletionStatus::Success, 1))
            .await;

        assert_eq!(outcome, ProcessedOutcome::Stale);
        let exec = store.get_execution(exec_id).unwrap().unwrap();
        assert_eq!(
            exec.state,
            ExecutionState::Queued,
            "re-run still owns the row"
        );
        assert!(exec.completed_at.is_none());
    }

    #[tokio::test]
    async fn late_failure_after_requeue_spawns_no_retry() {
        let store = make_store();
        let runner = make_runner();
        let exec_id = seed_execution(&store, "test:job");
        assert!(store.requeue_if_claimed(exec_id, Utc::now()).unwrap());

        let processor = CompletionProcessor::new(
            vec![make_job("test:job", 3)],
            Arc::clone(&store),
            Arc::clone(&runner),
        );
        let outcome = processor
            .process(event(exec_id, CompletionStatus::Failure, 1))
            .await;

        assert_eq!(outcome, ProcessedOutcome::Stale);
        // No retry enqueued and no extra execution row created — the
        // requeued original is the only pending work.
        assert_eq!(runner.queue.read().await.len(), 0);
        let exec = store.get_execution(exec_id).unwrap().unwrap();
        assert_eq!(exec.state, ExecutionState::Queued);
    }

    #[tokio::test]
    async fn late_exhausted_failure_after_requeue_creates_no_dead_letter() {
        use croniq_store::models::DeadLetterFilter;

        let store = make_store();
        let runner = make_runner();
        let exec_id = seed_execution(&store, "test:job");
        assert!(store.requeue_if_claimed(exec_id, Utc::now()).unwrap());

        // Single attempt → the late failure would dead-letter if not fenced.
        let processor =
            CompletionProcessor::new(vec![make_job("test:job", 1)], Arc::clone(&store), runner);
        let outcome = processor
            .process(event(exec_id, CompletionStatus::Failure, 1))
            .await;

        assert_eq!(outcome, ProcessedOutcome::Stale);
        let exec = store.get_execution(exec_id).unwrap().unwrap();
        assert_eq!(exec.state, ExecutionState::Queued);
        assert!(
            store
                .list_dead_letters(&DeadLetterFilter::default())
                .unwrap()
                .is_empty(),
            "a late completion must not dead-letter the requeued execution"
        );
    }

    #[tokio::test]
    async fn completion_from_wrong_runner_is_ignored() {
        let store = make_store();
        let runner = make_runner();
        let exec_id = seed_execution(&store, "test:job");

        // Requeue + re-claim by another runner: the row is `claimed` again,
        // so only the runner fence can reject the original runner's event.
        assert!(store.requeue_if_claimed(exec_id, Utc::now()).unwrap());
        store
            .claim_execution(exec_id, "runner-2", Utc::now())
            .unwrap();

        let processor =
            CompletionProcessor::new(vec![make_job("test:job", 3)], Arc::clone(&store), runner);
        // event() reports runner-1 — not the current owner.
        let outcome = processor
            .process(event(exec_id, CompletionStatus::Success, 1))
            .await;

        assert_eq!(outcome, ProcessedOutcome::Stale);
        let exec = store.get_execution(exec_id).unwrap().unwrap();
        assert_eq!(exec.state, ExecutionState::Claimed);
        assert_eq!(exec.runner_id.as_deref(), Some("runner-2"));
    }

    // ─── Alerts (issue #140) ────────────────────────────────────────

    // Unix-only: asserts a `Delivered` state through a Shell channel,
    // and shell alert delivery spawns `sh -c` (absent on stock Windows).
    #[cfg(unix)]
    #[tokio::test]
    async fn dead_letter_dispatches_configured_alert_rule() {
        use croniq_config::compile::{
            AlertsConfig, ChannelConfig, ChannelKind, RuleConfig, RuleTrigger,
        };
        use croniq_store::models::{AlertDeliveryFilter, AlertDeliveryState};

        let store = make_store();
        let runner = make_runner();
        let exec_id = seed_execution(&store, "billing:invoice");

        // One rule, one channel — `true` is a shell-noop that returns 0.
        let alerts = AlertsConfig {
            channels: [(
                "ops".to_string(),
                ChannelConfig {
                    name: "ops".into(),
                    kind: ChannelKind::Shell {
                        command: "true".into(),
                    },
                },
            )]
            .into_iter()
            .collect(),
            rules: vec![RuleConfig {
                name: "billing-fail".into(),
                trigger: RuleTrigger::JobFailed,
                job_key_glob: "billing:*".into(),
                min_attempts: 1,
                dead_letter_only: false,
                throttle: None,
                expected_within: None,
                channels: vec!["ops".into()],
            }],
        };

        let job = make_job("billing:invoice", 1); // single attempt → dead-letter
        let processor = CompletionProcessor::with_alerts(
            vec![job],
            Arc::clone(&store),
            runner,
            alerts,
            crate::alerts::empty_throttle_map(),
            crate::email::default_sender(),
        );

        let outcome = processor
            .process(event(exec_id, CompletionStatus::Failure, 1))
            .await;
        assert!(matches!(outcome, ProcessedOutcome::DeadLettered { .. }));

        let deliveries = store
            .list_alert_deliveries(&AlertDeliveryFilter::default())
            .unwrap();
        assert_eq!(deliveries.len(), 1, "exactly one rule fired");
        assert_eq!(deliveries[0].rule_name, "billing-fail");
        assert_eq!(deliveries[0].channel_name, "ops");
        assert_eq!(deliveries[0].job_key, "billing:invoice");
        assert_eq!(deliveries[0].state, AlertDeliveryState::Delivered);
    }

    #[tokio::test]
    async fn dead_letter_with_empty_alerts_records_no_deliveries() {
        // Mirrors the pre-#140 baseline: an install without an
        // `alerts {}` block and no `CRONIQ_ON_FAILURE_CMD` env var is
        // completely silent. The completion processor must not write
        // anything to alert_deliveries.
        use croniq_store::models::AlertDeliveryFilter;

        let store = make_store();
        let runner = make_runner();
        let exec_id = seed_execution(&store, "any:job");
        let job = make_job("any:job", 1);
        let processor = CompletionProcessor::new(vec![job], Arc::clone(&store), runner);

        let outcome = processor
            .process(event(exec_id, CompletionStatus::Failure, 1))
            .await;
        assert!(matches!(outcome, ProcessedOutcome::DeadLettered { .. }));

        let deliveries = store
            .list_alert_deliveries(&AlertDeliveryFilter::default())
            .unwrap();
        assert!(deliveries.is_empty(), "no alerts config = no deliveries");
    }
}

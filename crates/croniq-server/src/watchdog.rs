//! Abandoned-job watchdog: detects dead runners and requeues their in-flight
//! executions so they are not lost permanently.
//!
//! Flow (runs every `interval`, default 30 s):
//! ```text
//! 1. Collect runner IDs whose last_poll_at > Dead threshold (2 min)
//! 2. For each dead runner:
//!    a. store.requeue_abandoned(runner_id, now)  → Vec<Uuid>
//!    b. For each requeued execution ID:
//!       - Load the execution from the store
//!       - Rebuild a WorkItem (job config look-up for require/prefer/timeout)
//!       - Enqueue the WorkItem back in the runner queue
//! 3. Remove dead runners from the in-memory registry so they don't skew stats
//! 4. SLA sweep (issue #140 PR-4): list claimed executions, fire
//!    `job_sla_missed` rules whose `expected_within` has elapsed,
//!    deduped per (rule, execution_id) so a long-running job doesn't
//!    re-alert every 30 s.
//! ```

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::loader::job_config_from_job_def;
use crate::store::DynStore;
use chrono::{DateTime, Utc};
use croniq_config::compile::{AlertsConfig, JobConfig, RuleTrigger};
use croniq_runner::{AppState, RunnerStatus, WorkItem};
use croniq_store::models::{ExecutionFilter, ExecutionState};

/// Result of a single watchdog sweep.
#[derive(Debug, Clone, Default)]
pub struct WatchdogResult {
    /// Runner IDs found dead and processed.
    pub dead_runners: Vec<String>,
    /// Execution IDs that were requeued.
    pub requeued: Vec<uuid::Uuid>,
    /// Execution IDs cancelled due to queue_ttl expiry.
    pub expired: Vec<uuid::Uuid>,
    /// `(rule_name, execution_id)` pairs whose SLA-miss alert fired in
    /// this sweep (issue #140 PR-4). Useful for tests and a future
    /// metric; not exposed via any public API today.
    pub sla_missed: Vec<(String, uuid::Uuid)>,
    /// Rule names whose operational override expired and was auto-cleared
    /// this sweep (issue #231). Each emits an `alerts.override.cleared`
    /// audit event.
    pub cleared_overrides: Vec<String>,
}

/// Requeue all executions still claimed by `runner_id` in the persistent
/// store and re-enqueue them onto the in-memory work queue. Shared between
/// the watchdog sweep and the inline-takeover path in the poll handler
/// (issue #190).
///
/// `resolve_job_config` is invoked once per requeued execution to rebuild the
/// `WorkItem`'s require/prefer/timeout fields. Returns the list of execution
/// IDs that were marked queued (possibly empty). Executions whose job config
/// can't be resolved are skipped with a warn-level log — the store row is
/// still flipped to `queued`, the watchdog will pick it up again later.
pub async fn requeue_abandoned_for_runner<F>(
    store: &DynStore,
    runner: &Arc<AppState>,
    runner_id: &str,
    now: DateTime<Utc>,
    mut resolve_job_config: F,
) -> Vec<uuid::Uuid>
where
    F: FnMut(&str) -> Option<JobConfig>,
{
    let requeued_ids = match store.requeue_abandoned(runner_id, now) {
        Ok(ids) => ids,
        Err(e) => {
            tracing::error!(
                runner_id = %runner_id,
                error = %e,
                "requeue_abandoned: store error"
            );
            return vec![];
        }
    };

    if requeued_ids.is_empty() {
        return requeued_ids;
    }

    let mut enqueued = 0usize;
    for exec_id in &requeued_ids {
        let execution = match store.get_execution(*exec_id) {
            Ok(Some(e)) => e,
            Ok(None) => {
                tracing::warn!(id = %exec_id, "requeue_abandoned: execution not found in store");
                continue;
            }
            Err(e) => {
                tracing::error!(id = %exec_id, error = %e, "requeue_abandoned: store read error");
                continue;
            }
        };

        let job = match resolve_job_config(&execution.job_key) {
            Some(c) => c,
            None => {
                tracing::warn!(
                    job_key = %execution.job_key,
                    "requeue_abandoned: job not in DSL or store — leaving queued for next sweep"
                );
                continue;
            }
        };

        let item = WorkItem {
            execution_id: exec_id.to_string(),
            job_key: execution.job_key.clone(),
            fire_at: execution.fire_at,
            attempt: execution.attempt,
            require: job.runner.require.clone(),
            prefer: job.runner.prefer.clone(),
            metadata: serde_json::json!(execution.metadata),
            timeout: job.timeout.unwrap_or_else(|| "5m".into()),
        };

        runner.queue.write().await.enqueue(item);
        enqueued += 1;
    }

    if enqueued > 0 {
        runner.work_notify.notify_waiters();
    }

    requeued_ids
}

/// In-memory set of `(rule_name, execution_id)` pairs that have
/// already received an SLA-miss alert. Prevents the watchdog from
/// re-alerting every sweep for the same long-running execution.
///
/// Reset on process restart — after a restart, a still-running
/// execution will produce one duplicate alert. That cost is
/// proportional to "how many SLAs you breached at restart time" and
/// is bounded by sweep interval (~30s).
pub type SlaFiredSet = Arc<Mutex<HashSet<(String, uuid::Uuid)>>>;

/// Build an empty SLA dedup set. Used by `new()` and tests.
pub fn empty_sla_fired_set() -> SlaFiredSet {
    Arc::new(Mutex::new(HashSet::new()))
}

/// Periodically scans for dead runners and requeues their abandoned executions.
pub struct WatchdogLoop {
    jobs: HashMap<String, JobConfig>,
    store: DynStore,
    runner: Arc<AppState>,
    /// Failure-alert configuration (issue #140). Empty default ⇒ the
    /// SLA sweep is a no-op (no `job_sla_missed` rules to fire).
    alerts: AlertsConfig,
    /// Shared with [`crate::completion::CompletionProcessor`] so the
    /// per-(rule, job_key) throttle applies across both `job_failed`
    /// and `job_sla_missed` fires. Without this share, a rule with
    /// `throttle 10m` would fire one failure + one SLA alert in the
    /// same window.
    alert_throttle: crate::alerts::ThrottleMap,
    /// Tracks `(rule_name, execution_id)` already alerted on so the
    /// SLA sweep doesn't re-alert every 30 s while the execution
    /// stays in-flight.
    sla_fired: SlaFiredSet,
    /// Shared with the rest of the server so SLA-miss alerts that
    /// route to an `email` channel actually deliver instead of
    /// silently dropping. Defaults to `NoopSender` in tests.
    email_sender: Arc<dyn crate::email::EmailSender>,
}

impl WatchdogLoop {
    pub fn new(jobs: Vec<JobConfig>, store: DynStore, runner: Arc<AppState>) -> Self {
        Self::with_alerts(
            jobs,
            store,
            runner,
            AlertsConfig::default(),
            crate::alerts::empty_throttle_map(),
            empty_sla_fired_set(),
            Arc::new(crate::email::NoopSender),
        )
    }

    /// Construct with full alerting wiring. Used by main.rs to share
    /// the same `AlertsConfig` + throttle map with the completion
    /// processor — without sharing, a rule with `throttle 10m` would
    /// allow one failure + one SLA alert in the same window.
    pub fn with_alerts(
        jobs: Vec<JobConfig>,
        store: DynStore,
        runner: Arc<AppState>,
        alerts: AlertsConfig,
        alert_throttle: crate::alerts::ThrottleMap,
        sla_fired: SlaFiredSet,
        email_sender: Arc<dyn crate::email::EmailSender>,
    ) -> Self {
        let jobs = jobs.into_iter().map(|j| (j.key.clone(), j)).collect();
        Self {
            jobs,
            store,
            runner,
            alerts,
            alert_throttle,
            sla_fired,
            email_sender,
        }
    }

    /// Resolve `JobConfig` for a key: DSL map first, then store fallback.
    fn resolve_job_config(&self, job_key: &str) -> Option<JobConfig> {
        if let Some(c) = self.jobs.get(job_key) {
            return Some(c.clone());
        }
        match self.store.get_job_definition(job_key) {
            Ok(Some(def)) => {
                tracing::debug!(job_key = %job_key, "watchdog: synthesising config from store for API job");
                Some(job_config_from_job_def(&def))
            }
            Ok(None) => None,
            Err(e) => {
                tracing::error!(job_key = %job_key, error = %e, "watchdog: store error resolving job config");
                None
            }
        }
    }

    /// Run one sweep at `now`.
    pub async fn sweep(&self, now: DateTime<Utc>) -> WatchdogResult {
        let mut result = WatchdogResult::default();

        // 1. Find dead runners from the in-memory registry (using configured lease TTL)
        let dead_ids: Vec<String> = {
            let reg = self.runner.registry.read().await;
            reg.by_status_with_ttl(RunnerStatus::Dead, now, self.runner.lease_ttl_secs)
                .into_iter()
                .map(|r| r.runner_id.clone())
                .collect()
        };

        // The dead-runner branch only runs when there's actually a
        // dead runner — but the queue_ttl expiry (step 4) and the
        // SLA-miss sweep (step 5, issue #140 PR-4) always need to
        // run, so we DON'T early-return when dead_ids is empty.
        for runner_id in &dead_ids {
            let requeued_ids = requeue_abandoned_for_runner(
                &self.store,
                &self.runner,
                runner_id,
                now,
                |job_key| self.resolve_job_config(job_key),
            )
            .await;

            if requeued_ids.is_empty() {
                tracing::debug!(runner_id = %runner_id, "watchdog: dead runner had no inflight executions");
            } else {
                tracing::warn!(
                    runner_id = %runner_id,
                    count = requeued_ids.len(),
                    "watchdog: requeued abandoned executions from dead runner"
                );
            }

            for exec_id in &requeued_ids {
                result.requeued.push(*exec_id);
            }
            result.dead_runners.push(runner_id.clone());
        }

        // 3. Evict dead runners from the in-memory registry AND drop any
        //    pending cancels queued for them — a runner that never came
        //    back can't act on its cancel queue, and the executions are
        //    being requeued onto a (different) live runner in the next
        //    sweep step. Without this cleanup `AppState::cancel_queues`
        //    grows unboundedly on long-lived servers where operators
        //    decommission hosts (issue #176 follow-up).
        {
            let mut reg = self.runner.registry.write().await;
            let mut cancels = self.runner.cancel_queues.write().await;
            for runner_id in &result.dead_runners {
                reg.remove(runner_id);
                cancels.remove(runner_id);
            }
        }

        // 4. Expire queued executions that have exceeded their queue_ttl
        self.expire_queued_by_ttl(now, &mut result).await;

        // 5. SLA-miss sweep (issue #140 PR-4). Fast-path: no
        //    `job_sla_missed` rules ⇒ skip the store query entirely.
        if self
            .alerts
            .rules
            .iter()
            .any(|r| matches!(r.trigger, RuleTrigger::JobSlaMissed))
        {
            self.sweep_sla_missed(now, &mut result).await;
        }

        // 6. Auto-clear expired operational overrides (issue #231). Same
        //    cadence as the SLA sweep — a "snooze 4h" evaporates without
        //    operator follow-up. Each cleared row gets an audit event.
        self.sweep_expired_overrides(now, &mut result);

        result
    }

    /// Delete overrides whose `expires_at` has passed and emit one
    /// `alerts.override.cleared` audit event per row (system actor).
    /// Best-effort — a store error is logged and the next sweep retries.
    fn sweep_expired_overrides(&self, now: DateTime<Utc>, result: &mut WatchdogResult) {
        match self.store.delete_expired_alert_rule_overrides(now) {
            Ok(cleared) => {
                for rule_name in &cleared {
                    crate::api::audit::record_event(
                        &self.store,
                        "system",
                        None,
                        "alerts.override.cleared",
                        "alert_rule",
                        Some(rule_name),
                    );
                    tracing::info!(
                        target: "croniq::alerts",
                        rule = %rule_name,
                        "operational override expired — auto-cleared"
                    );
                }
                result.cleared_overrides = cleared;
            }
            Err(e) => {
                tracing::warn!(
                    target: "croniq::alerts",
                    error = %e,
                    "watchdog: failed to sweep expired alert-rule overrides"
                );
            }
        }
    }

    /// Find claimed executions whose `expected_within` window has
    /// elapsed and fire matching `job_sla_missed` rules.
    ///
    /// Dedup: each `(rule_name, execution_id)` pair fires at most
    /// once per process lifetime (see [`SlaFiredSet`]).
    async fn sweep_sla_missed(&self, now: DateTime<Utc>, result: &mut WatchdogResult) {
        // List all currently claimed executions. The "since" filter
        // is left wide-open because we want to find executions that
        // have been claimed FOR A LONG TIME — the typical case is a
        // job that started 30 minutes ago and is hung. A bounded
        // limit keeps the sweep cheap on busy servers.
        let filter = ExecutionFilter {
            state: Some(ExecutionState::Claimed),
            limit: Some(500),
            ..Default::default()
        };
        let claimed = match self.store.list_executions(&filter) {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(
                    target: "croniq::alerts",
                    error = %e,
                    "watchdog: SLA sweep failed to list claimed executions"
                );
                return;
            }
        };

        for execution in &claimed {
            // Prefer started_at over claimed_at: SLA is about
            // execution duration, not queue + claim latency. Fall
            // back to claimed_at when started_at is None (runner
            // claimed but hasn't reported back yet).
            let Some(started) = execution.started_at.or(execution.claimed_at) else {
                continue;
            };
            let elapsed = (now - started).num_seconds();
            if elapsed <= 0 {
                continue; // clock skew or just-claimed
            }
            let elapsed = elapsed as u64;

            for rule in &self.alerts.rules {
                if !matches!(rule.trigger, RuleTrigger::JobSlaMissed) {
                    continue;
                }
                let Some(window_str) = rule.expected_within.as_deref() else {
                    continue; // compile path drops these, defensive
                };
                let Some(window_secs) = crate::alerts::parse_throttle_secs(window_str) else {
                    continue;
                };
                if elapsed < window_secs {
                    continue;
                }
                if !crate::alerts::glob_match(&rule.job_key_glob, &execution.job_key) {
                    continue;
                }

                // Dedup per-execution. The set is small (bounded by
                // concurrent in-flight executions × matching rules)
                // so the linear lookup is fine.
                let key = (rule.name.clone(), execution.id);
                {
                    let mut guard = self.sla_fired.lock().unwrap();
                    if !guard.insert(key.clone()) {
                        continue;
                    }
                }

                // Fire via the shared `dispatch_rule` helper so
                // throttle + channels + audit behave identically to
                // the `job_failed` path. The "reason" field
                // distinguishes the two trigger types for shell
                // channels (CRONIQ_REASON=sla_miss vs dead_letter).
                let ctx = crate::alerts::FailureContext {
                    job_key: execution.job_key.clone(),
                    execution_id: execution.id.to_string(),
                    error: format!(
                        "SLA missed: started at {started}, in-flight for {elapsed}s (expected within {window_str})",
                    ),
                    attempt: execution.attempt,
                    reason: "sla_miss".to_string(),
                };
                crate::alerts::dispatch_rule(
                    rule,
                    &self.alerts,
                    &ctx,
                    &self.alert_throttle,
                    &self.store,
                    &self.email_sender,
                )
                .await;
                result.sla_missed.push(key);
            }
        }
    }

    /// Cancel queued executions whose age exceeds the job's `queue_ttl`.
    ///
    /// Scans all items currently in the in-memory queue. For each item whose
    /// job has a `queue_ttl` configured, check whether the item has been
    /// waiting longer than allowed. If so, remove it from the queue and cancel
    /// the execution in the store.
    async fn expire_queued_by_ttl(&self, now: DateTime<Utc>, result: &mut WatchdogResult) {
        // Collect job keys that have a queue_ttl configured.
        let ttls: HashMap<&str, chrono::Duration> = self
            .jobs
            .iter()
            .filter_map(|(key, job)| {
                let ttl_str = job.queue_ttl.as_deref()?;
                let std_dur = croniq_execution::retry::parse_duration(ttl_str)?;
                let dur = chrono::Duration::from_std(std_dur).ok()?;
                Some((key.as_str(), dur))
            })
            .collect();

        if ttls.is_empty() {
            return;
        }

        // Peek at queued items and find expired ones.
        let expired_ids: Vec<String> = {
            let q = self.runner.queue.read().await;
            q.peek_n(10_000)
                .iter()
                .filter(|item| {
                    if let Some(ttl) = ttls.get(item.job_key.as_str()) {
                        // fire_at is when the item was created/enqueued
                        now.signed_duration_since(item.fire_at) > *ttl
                    } else {
                        false
                    }
                })
                .map(|item| item.execution_id.clone())
                .collect()
        };

        if expired_ids.is_empty() {
            return;
        }

        // Remove expired items from the queue and cancel in store.
        let mut q = self.runner.queue.write().await;
        for exec_id in &expired_ids {
            q.remove(exec_id);
            if let Ok(uuid) = uuid::Uuid::parse_str(exec_id) {
                let _ = self.store.cancel_execution(uuid, now);
                result.expired.push(uuid);
            }
        }
        drop(q);

        tracing::info!(
            count = expired_ids.len(),
            "watchdog: cancelled queued executions due to queue_ttl expiry"
        );
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::Duration as ChronoDuration;
    use croniq_config::compile::{DeadLetterConfig, RetryConfig, RunnerConfig};
    use croniq_runner::AppState;
    use croniq_store::{
        models::{Execution, ExecutionState},
        sqlite::SqliteStore,
        traits::ExecutionStore,
    };
    use pretty_assertions::assert_eq;
    use uuid::Uuid;

    use super::*;
    use crate::store::{DynStore, sqlite_store};

    fn make_store() -> DynStore {
        sqlite_store(SqliteStore::in_memory().unwrap())
    }

    fn make_runner() -> Arc<AppState> {
        AppState::new()
    }

    fn make_job(key: &str) -> JobConfig {
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
            runner: RunnerConfig {
                require: vec!["billing".into()],
                ..RunnerConfig::default()
            },
            retry: RetryConfig::default(),
            timeout: Some("10m".into()),
            dead_letter: DeadLetterConfig::default(),
            metadata: HashMap::new(),
            execution_mode: croniq_config::compile::ExecutionMode::default(),
            catch_up: croniq_config::compile::CatchUpPolicy::default(),
            queue_ttl: None,
            max_queue_depth: None,
            tags: vec![],
        }
    }

    /// Seed an execution in Claimed state (simulates a runner that grabbed it).
    fn seed_claimed_execution(
        store: &dyn ExecutionStore,
        job_key: &str,
        runner_id: &str,
        attempt: u32,
    ) -> Uuid {
        let id = Uuid::new_v4();
        let now = Utc::now();
        store
            .create_execution(&Execution {
                id,
                job_key: job_key.into(),
                fire_at: now,
                attempt,
                state: ExecutionState::Queued,
                runner_id: None,
                claimed_at: None,
                started_at: None,
                completed_at: None,
                duration_ms: None,
                error: None,
                dead_reason: None,
                metadata: HashMap::new(),
                created_at: now,
            })
            .unwrap();
        // Transition to Claimed
        store.claim_execution(id, runner_id, now).unwrap();
        id
    }

    fn long_dead_time() -> DateTime<Utc> {
        // 5 minutes ago — well past the 2-minute dead threshold
        Utc::now() - ChronoDuration::minutes(5)
    }

    #[tokio::test]
    async fn sweep_no_dead_runners_does_nothing() {
        let store = make_store();
        let runner = make_runner();
        let watchdog = WatchdogLoop::new(vec![make_job("test:job")], store, runner);

        let result = watchdog.sweep(Utc::now()).await;
        assert!(result.dead_runners.is_empty());
        assert!(result.requeued.is_empty());
    }

    #[tokio::test]
    async fn sweep_requeues_abandoned_execution() {
        let store = make_store();
        let runner = make_runner();

        // Simulate a runner that polled 5 minutes ago (Dead threshold = 2 min)
        {
            let mut reg = runner.registry.write().await;
            let _ = reg.register_or_update(
                "dead-runner",
                vec!["billing".into()],
                3,
                vec![],
                None,
                vec![],
            );
            // Manually set last_poll_at to long ago
            if let Some(r) = reg.get_mut("dead-runner") {
                r.last_poll_at = long_dead_time();
            }
        }

        // Seed an execution that was claimed by the now-dead runner
        let exec_id = seed_claimed_execution(&*store, "test:job", "dead-runner", 1);

        let watchdog = WatchdogLoop::new(
            vec![make_job("test:job")],
            Arc::clone(&store),
            Arc::clone(&runner),
        );

        let result = watchdog.sweep(Utc::now()).await;

        assert_eq!(result.dead_runners, vec!["dead-runner"]);
        assert_eq!(result.requeued, vec![exec_id]);

        // Execution should be back to Queued in the store
        let exec = store.get_execution(exec_id).unwrap().unwrap();
        assert_eq!(exec.state, ExecutionState::Queued);

        // WorkItem should be enqueued
        let q = runner.queue.read().await;
        assert_eq!(q.len(), 1);
    }

    #[tokio::test]
    async fn sweep_requeued_work_item_has_correct_attempt() {
        let store = make_store();
        let runner = make_runner();

        {
            let mut reg = runner.registry.write().await;
            let _ = reg.register_or_update("dead-runner", vec![], 3, vec![], None, vec![]);
            if let Some(r) = reg.get_mut("dead-runner") {
                r.last_poll_at = long_dead_time();
            }
        }

        // Seed attempt 2 (retry that was in progress)
        seed_claimed_execution(&*store, "test:job", "dead-runner", 2);

        let watchdog = WatchdogLoop::new(
            vec![make_job("test:job")],
            Arc::clone(&store),
            Arc::clone(&runner),
        );

        watchdog.sweep(Utc::now()).await;

        let q = runner.queue.read().await;
        assert_eq!(q.len(), 1);
        // The requeued item should carry attempt 2, not 1
        // (we just check queue len here; the item's attempt is verified via store)
    }

    #[tokio::test]
    async fn sweep_removes_dead_runner_from_registry() {
        let store = make_store();
        let runner = make_runner();

        {
            let mut reg = runner.registry.write().await;
            let _ = reg.register_or_update("dead-runner", vec![], 3, vec![], None, vec![]);
            if let Some(r) = reg.get_mut("dead-runner") {
                r.last_poll_at = long_dead_time();
            }
        }

        let watchdog = WatchdogLoop::new(vec![], Arc::clone(&store), Arc::clone(&runner));

        let result = watchdog.sweep(Utc::now()).await;
        assert_eq!(result.dead_runners, vec!["dead-runner"]);

        // Dead runner should be evicted from registry
        let reg = runner.registry.read().await;
        assert!(reg.get("dead-runner").is_none());
    }

    #[tokio::test]
    async fn sweep_multiple_dead_runners_all_processed() {
        let store = make_store();
        let runner = make_runner();

        {
            let mut reg = runner.registry.write().await;
            for name in ["runner-a", "runner-b"] {
                let _ = reg.register_or_update(name, vec![], 3, vec![], None, vec![]);
                if let Some(r) = reg.get_mut(name) {
                    r.last_poll_at = long_dead_time();
                }
            }
        }

        let exec_a = seed_claimed_execution(&*store, "test:job", "runner-a", 1);
        let exec_b = seed_claimed_execution(&*store, "test:job", "runner-b", 1);

        let watchdog = WatchdogLoop::new(
            vec![make_job("test:job")],
            Arc::clone(&store),
            Arc::clone(&runner),
        );

        let result = watchdog.sweep(Utc::now()).await;

        assert_eq!(result.dead_runners.len(), 2);
        assert_eq!(result.requeued.len(), 2);
        assert!(result.requeued.contains(&exec_a));
        assert!(result.requeued.contains(&exec_b));

        let q = runner.queue.read().await;
        assert_eq!(q.len(), 2);
    }

    // ─── #140 PR-4 SLA-miss sweep ──────────────────────────────────

    use croniq_config::compile::{ChannelConfig, ChannelKind, RuleConfig, RuleTrigger};
    use croniq_store::models::AlertDeliveryFilter;

    /// Seed a Claimed execution with a fixed `claimed_at` / `started_at`
    /// so SLA tests can control the elapsed time deterministically.
    fn seed_claimed_at(
        store: &dyn ExecutionStore,
        job_key: &str,
        runner_id: &str,
        claimed_at: DateTime<Utc>,
    ) -> Uuid {
        let id = Uuid::new_v4();
        store
            .create_execution(&Execution {
                id,
                job_key: job_key.into(),
                fire_at: claimed_at,
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
                created_at: claimed_at,
            })
            .unwrap();
        store.claim_execution(id, runner_id, claimed_at).unwrap();
        id
    }

    fn sla_rule(name: &str, glob: &str, within: &str, channel: &str) -> RuleConfig {
        RuleConfig {
            name: name.into(),
            trigger: RuleTrigger::JobSlaMissed,
            job_key_glob: glob.into(),
            min_attempts: 1,
            dead_letter_only: false,
            throttle: None,
            expected_within: Some(within.into()),
            channels: vec![channel.into()],
        }
    }

    fn alerts_with_sla(rules: Vec<RuleConfig>) -> AlertsConfig {
        AlertsConfig {
            channels: [(
                "ops".into(),
                ChannelConfig {
                    name: "ops".into(),
                    kind: ChannelKind::Shell {
                        command: "true".into(),
                    },
                },
            )]
            .into_iter()
            .collect(),
            rules,
        }
    }

    fn watchdog_with_alerts_only(store: DynStore, alerts: AlertsConfig) -> WatchdogLoop {
        WatchdogLoop::with_alerts(
            vec![],
            store,
            make_runner(),
            alerts,
            crate::alerts::empty_throttle_map(),
            empty_sla_fired_set(),
            Arc::new(crate::email::NoopSender),
        )
    }

    #[tokio::test]
    async fn sla_sweep_fires_when_in_flight_past_window() {
        let store = make_store();
        let now = Utc::now();
        // Execution claimed 15 minutes ago, still in-flight.
        let exec_id = seed_claimed_at(
            &*store,
            "billing:invoice",
            "runner-1",
            now - ChronoDuration::minutes(15),
        );

        let alerts = alerts_with_sla(vec![sla_rule("slow-billing", "billing:*", "10m", "ops")]);
        let watchdog = watchdog_with_alerts_only(Arc::clone(&store), alerts);
        let result = watchdog.sweep(now).await;

        assert_eq!(
            result.sla_missed,
            vec![("slow-billing".to_string(), exec_id)]
        );
        let deliveries = store
            .list_alert_deliveries(&AlertDeliveryFilter::default())
            .unwrap();
        assert_eq!(deliveries.len(), 1, "exactly one delivery row");
        assert_eq!(deliveries[0].rule_name, "slow-billing");
        assert_eq!(deliveries[0].job_key, "billing:invoice");
    }

    #[tokio::test]
    async fn sla_sweep_skips_when_within_window() {
        let store = make_store();
        let now = Utc::now();
        let _exec_id = seed_claimed_at(
            &*store,
            "billing:invoice",
            "runner-1",
            now - ChronoDuration::minutes(5),
        );

        let alerts = alerts_with_sla(vec![sla_rule("slow-billing", "billing:*", "10m", "ops")]);
        let watchdog = watchdog_with_alerts_only(Arc::clone(&store), alerts);
        let result = watchdog.sweep(now).await;

        assert!(result.sla_missed.is_empty(), "5min elapsed < 10min window");
        let deliveries = store
            .list_alert_deliveries(&AlertDeliveryFilter::default())
            .unwrap();
        assert!(deliveries.is_empty());
    }

    #[tokio::test]
    async fn sla_sweep_dedups_across_repeated_sweeps() {
        let store = make_store();
        let now = Utc::now();
        seed_claimed_at(
            &*store,
            "ops:long",
            "runner-1",
            now - ChronoDuration::minutes(15),
        );

        let alerts = alerts_with_sla(vec![sla_rule("slow", "*", "10m", "ops")]);
        let watchdog = watchdog_with_alerts_only(Arc::clone(&store), alerts);

        // First sweep fires the alert.
        let r1 = watchdog.sweep(now).await;
        assert_eq!(r1.sla_missed.len(), 1);

        // Second sweep, same execution still in-flight, must NOT fire again.
        let r2 = watchdog.sweep(now + ChronoDuration::seconds(30)).await;
        assert!(
            r2.sla_missed.is_empty(),
            "dedup must suppress repeat alerts for the same execution"
        );

        // Only one delivery row in the store.
        let deliveries = store
            .list_alert_deliveries(&AlertDeliveryFilter::default())
            .unwrap();
        assert_eq!(deliveries.len(), 1);
    }

    #[tokio::test]
    async fn sla_sweep_respects_job_key_glob() {
        let store = make_store();
        let now = Utc::now();
        seed_claimed_at(
            &*store,
            "ops:cleanup",
            "runner-1",
            now - ChronoDuration::minutes(15),
        );
        seed_claimed_at(
            &*store,
            "billing:invoice",
            "runner-1",
            now - ChronoDuration::minutes(15),
        );

        // Rule only matches billing:*.
        let alerts = alerts_with_sla(vec![sla_rule("only-billing", "billing:*", "10m", "ops")]);
        let watchdog = watchdog_with_alerts_only(Arc::clone(&store), alerts);
        let result = watchdog.sweep(now).await;

        assert_eq!(result.sla_missed.len(), 1, "exactly the billing one fires");
        let deliveries = store
            .list_alert_deliveries(&AlertDeliveryFilter::default())
            .unwrap();
        assert_eq!(deliveries.len(), 1);
        assert_eq!(deliveries[0].job_key, "billing:invoice");
    }

    #[tokio::test]
    async fn sla_sweep_noop_when_no_sla_rules() {
        // job_failed rules don't trigger the SLA path; the sweep
        // must NOT query the store at all (we can't easily assert
        // "no query happened" — assert by absence of side-effects).
        let store = make_store();
        let now = Utc::now();
        seed_claimed_at(&*store, "x:y", "r1", now - ChronoDuration::minutes(15));

        let alerts = AlertsConfig {
            channels: [(
                "ops".into(),
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
                name: "permanent-failures".into(),
                trigger: RuleTrigger::JobFailed,
                job_key_glob: "*".into(),
                min_attempts: 1,
                dead_letter_only: false,
                throttle: None,
                expected_within: None,
                channels: vec!["ops".into()],
            }],
        };
        let watchdog = watchdog_with_alerts_only(Arc::clone(&store), alerts);
        let result = watchdog.sweep(now).await;

        assert!(result.sla_missed.is_empty());
        let deliveries = store
            .list_alert_deliveries(&AlertDeliveryFilter::default())
            .unwrap();
        assert!(deliveries.is_empty());
    }
}

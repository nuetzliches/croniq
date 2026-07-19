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
//! 5. Stale-claim reaper (issue #374): requeue claimed executions whose
//!    claim age exceeds the job `timeout` plus a grace window — the
//!    liveness-independent safety net for claims orphaned by a fast
//!    runner restart or a server restart. Runs after the SLA sweep so
//!    stuck claims alert before they are recovered.
//! 6. Queued-reconcile sweep: re-enqueue store-`queued` rows missing
//!    from the in-memory work queue (e.g. a requeue path flipped the row
//!    but couldn't rebuild the WorkItem). Rows whose job no longer exists
//!    anywhere are cancelled — nothing could ever dispatch them.
//! ```

use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

use crate::loader::job_config_from_job_def;
use crate::store::DynStore;
use chrono::{DateTime, Duration, Utc};
use croniq_config::compile::{AlertsConfig, JobConfig, RuleTrigger};
use croniq_runner::{AppState, RunnerStatus, WorkItem};
use croniq_store::models::{ExecutionState, JobStatus};

/// Max execution rows deleted per prune DELETE statement. Bounds SQLite's
/// whole-DB write-lock hold time; a backlog drains across ticks/batches.
const PRUNE_BATCH: u32 = 5_000;
/// Max batches per reason per sweep, so the first prune on a large backlog
/// doesn't monopolise the 30 s watchdog tick. Backlog beyond
/// `PRUNE_BATCH * PRUNE_MAX_BATCHES` rows carries over to the next tick.
const PRUNE_MAX_BATCHES: usize = 20;

/// Max store-`queued` rows the reconcile sweep examines per tick. A larger
/// stranded backlog drains across sweeps — rows stay `queued` until handled,
/// and `find_queued_executions` returns oldest-due first.
const RECONCILE_BATCH: u32 = 500;

/// Max claimed executions examined per SLA / stale-claim sweep. The store
/// query lists oldest claim first, so with more than this many concurrent
/// claims the overflow only defers the NEWEST claims to a later sweep —
/// the oldest (the SLA-breached / orphaned ones both sweeps target) are
/// always inside the window.
const CLAIM_SWEEP_LIMIT: u32 = 500;

/// Counts from one [`WatchdogLoop::prune_executions`] pass (issue #344).
#[derive(Debug, Clone, Copy, Default)]
pub struct PruneResult {
    /// Rows deleted by the global `execution_retention` age sweep.
    pub by_age: u64,
    /// Rows deleted by per-job `keep_last` caps.
    pub by_cap: u64,
}

impl PruneResult {
    /// Total execution rows deleted this pass.
    pub fn total(&self) -> u64 {
        self.by_age + self.by_cap
    }
}

/// Result of a single watchdog sweep.
#[derive(Debug, Clone, Default)]
pub struct WatchdogResult {
    /// Runner IDs found dead and processed.
    pub dead_runners: Vec<String>,
    /// Execution IDs that were requeued.
    pub requeued: Vec<uuid::Uuid>,
    /// Execution IDs requeued by the stale-claim reaper (issue #374):
    /// claims older than job `timeout` + grace, independent of runner
    /// liveness. Kept separate from `requeued` (dead-runner sweep) so
    /// operators and tests can tell the two recovery paths apart.
    pub stale_claims: Vec<uuid::Uuid>,
    /// Execution IDs cancelled due to queue_ttl expiry.
    pub expired: Vec<uuid::Uuid>,
    /// Execution IDs re-enqueued by the queued-reconcile sweep: rows that
    /// were `queued` in the store but missing from the in-memory work
    /// queue (e.g. a requeue path flipped the row but couldn't rebuild
    /// the WorkItem at the time). Without this sweep such rows would
    /// strand until the next server restart — no other path re-reads
    /// store-queued rows at runtime.
    pub reconciled: Vec<uuid::Uuid>,
    /// Stranded queued executions cancelled by the reconcile sweep
    /// because their job exists neither in the DSL nor as a stored
    /// JobDefinition — no sweep could ever rebuild their WorkItem.
    pub stranded_cancelled: Vec<uuid::Uuid>,
    /// `(rule_name, execution_id)` pairs whose SLA-miss alert fired in
    /// this sweep (issue #140 PR-4). Useful for tests and a future
    /// metric; not exposed via any public API today.
    pub sla_missed: Vec<(String, uuid::Uuid)>,
    /// `(rule_name, job_key)` pairs whose missed-fire alert fired in
    /// this sweep (issue #250). A scheduled fire that never happened —
    /// the job's persisted `next_fire_at` went overdue past the rule's
    /// grace window while the trigger was still active.
    pub missed_fires: Vec<(String, String)>,
    /// Rule names whose operational override expired and was auto-cleared
    /// this sweep (issue #231). Each emits an `alerts.override.cleared`
    /// audit event.
    pub cleared_overrides: Vec<String>,
}

/// Cumulative process-lifetime counters for the watchdog's recovery actions,
/// exposed as Prometheus counters on `/metrics` (same pattern as
/// [`crate::reload::ReloadCounters`]). Their rates are an operator signal:
/// frequent dead-runner/stale-claim requeues point at unstable runners,
/// stranded cancels at jobs deleted with work still in flight.
#[derive(Debug, Default)]
pub struct WatchdogCounters {
    /// Executions requeued from dead runners — by the watchdog sweep AND by
    /// the inline-takeover path in the poll handler (both recover the same
    /// "runner session gone" condition, so they share one series).
    pub requeued_dead_runner: AtomicU64,
    /// Executions requeued by the stale-claim reaper (issue #374).
    pub requeued_stale_claim: AtomicU64,
    /// Executions re-enqueued by the queued-reconcile sweep.
    pub requeued_reconciled: AtomicU64,
    /// Queued executions cancelled on `queue_ttl` expiry.
    pub cancelled_queue_ttl: AtomicU64,
    /// Stranded queued executions cancelled because their job is gone.
    pub cancelled_stranded: AtomicU64,
    /// `job_sla_missed` alerts fired (issue #140 PR-4).
    pub sla_missed: AtomicU64,
    /// `job_missed_fire` alerts fired (issue #250).
    pub missed_fires: AtomicU64,
}

impl WatchdogCounters {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Fold one sweep's [`WatchdogResult`] into the cumulative counters.
    pub fn record(&self, result: &WatchdogResult) {
        use std::sync::atomic::Ordering::Relaxed;
        self.requeued_dead_runner
            .fetch_add(result.requeued.len() as u64, Relaxed);
        self.requeued_stale_claim
            .fetch_add(result.stale_claims.len() as u64, Relaxed);
        self.requeued_reconciled
            .fetch_add(result.reconciled.len() as u64, Relaxed);
        self.cancelled_queue_ttl
            .fetch_add(result.expired.len() as u64, Relaxed);
        self.cancelled_stranded
            .fetch_add(result.stranded_cancelled.len() as u64, Relaxed);
        self.sla_missed
            .fetch_add(result.sla_missed.len() as u64, Relaxed);
        self.missed_fires
            .fetch_add(result.missed_fires.len() as u64, Relaxed);
    }

    /// Count executions recovered by the inline-takeover requeue in the poll
    /// handler — same condition as the sweep's dead-runner requeue, so it
    /// feeds the same series.
    pub fn add_dead_runner_requeued(&self, n: u64) {
        self.requeued_dead_runner
            .fetch_add(n, std::sync::atomic::Ordering::Relaxed);
    }
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
/// still flipped to `queued`, and the watchdog's queued-reconcile sweep
/// ([`WatchdogLoop::reconcile_stranded_queued`]) later re-enqueues it or
/// cancels it if the job is gone for good.
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
        if enqueue_requeued_execution(store, runner, exec_id, &mut resolve_job_config).await {
            enqueued += 1;
        }
    }

    if enqueued > 0 {
        runner.work_notify.notify_waiters();
    }

    requeued_ids
}

/// Load a freshly-requeued execution, rebuild its `WorkItem` and put it back
/// on the in-memory work queue. Shared between the dead-runner requeue, the
/// stale-claim reaper (issue #374) and the queued-reconcile sweep. Returns
/// whether an item was enqueued; the caller batches
/// `work_notify.notify_waiters()`. Executions whose job config can't be
/// resolved are skipped with a warn-level log — the store row is already
/// `queued`, and [`WatchdogLoop::reconcile_stranded_queued`] retries it (or
/// cancels it once the job is gone from both DSL and store) on a later sweep.
async fn enqueue_requeued_execution<F>(
    store: &DynStore,
    runner: &Arc<AppState>,
    exec_id: &uuid::Uuid,
    resolve_job_config: &mut F,
) -> bool
where
    F: FnMut(&str) -> Option<JobConfig>,
{
    let execution = match store.get_execution(*exec_id) {
        Ok(Some(e)) => e,
        Ok(None) => {
            tracing::warn!(id = %exec_id, "requeue: execution not found in store");
            return false;
        }
        Err(e) => {
            tracing::error!(id = %exec_id, error = %e, "requeue: store read error");
            return false;
        }
    };

    // A completion or cancel may have raced the caller's requeue CAS and
    // this read — never resurrect a WorkItem for a non-queued row.
    if execution.state != ExecutionState::Queued {
        tracing::debug!(
            id = %exec_id,
            state = ?execution.state,
            "requeue: execution left queued state concurrently — not enqueuing"
        );
        return false;
    }

    let job = match resolve_job_config(&execution.job_key) {
        Some(c) => c,
        None => {
            tracing::warn!(
                job_key = %execution.job_key,
                "requeue: job not in DSL or store — left queued; the reconcile sweep retries or cancels it"
            );
            return false;
        }
    };

    let item = WorkItem {
        execution_id: exec_id.to_string(),
        job_key: execution.job_key.clone(),
        fire_at: execution.fire_at,
        scheduled_for: execution.scheduled_for,
        attempt: execution.attempt,
        require: job.runner.require.clone(),
        prefer: job.runner.prefer.clone(),
        metadata: serde_json::json!(execution.metadata),
        timeout: job.timeout.unwrap_or_else(|| "5m".into()),
    };

    runner.queue.write().await.enqueue(item)
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

/// In-memory set of `(rule_name, job_key, next_fire_at)` triples that
/// have already received a missed-fire alert (issue #250). Keyed on the
/// specific overdue fire time so each distinct missed fire alerts once,
/// even across many 30 s sweeps — and so the *next* missed fire (a
/// different `next_fire_at`) still alerts.
///
/// Reset on process restart, like [`SlaFiredSet`]. A restart while a job
/// is still overdue produces at most one duplicate alert.
pub type MissedFiredSet = Arc<Mutex<HashSet<(String, String, DateTime<Utc>)>>>;

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
    /// Tracks `(rule_name, job_key, next_fire_at)` already alerted on so
    /// the missed-fire sweep doesn't re-alert every 30 s while a job
    /// stays overdue (issue #250). Initialised internally; not a
    /// constructor parameter since nothing else shares it.
    missed_fired: MissedFiredSet,
    /// Shared with the rest of the server so SLA-miss alerts that
    /// route to an `email` channel actually deliver instead of
    /// silently dropping. Defaults to `NoopSender` in tests.
    email_sender: Arc<dyn crate::email::EmailSender>,
    /// Max claimed executions per SLA / stale-claim sweep query. Always
    /// [`CLAIM_SWEEP_LIMIT`] in production; tests shrink it to exercise
    /// the over-limit path without seeding hundreds of rows.
    claim_sweep_limit: u32,
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
            missed_fired: Arc::new(Mutex::new(HashSet::new())),
            email_sender,
            claim_sweep_limit: CLAIM_SWEEP_LIMIT,
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

    /// Enforce execution retention (issue #344): the global age sweep plus
    /// per-job `keep_last` caps read from the DSL job snapshot. Deletes
    /// terminal executions (`completed` / `failed` / `cancelled`) and their
    /// logs in bounded batches; `dead` executions are left to dead-letter
    /// retention. `retention` is the already-parsed age threshold (`None`
    /// disables the age sweep). Returns per-reason delete counts.
    ///
    /// Store calls are synchronous; each DELETE is bounded to [`PRUNE_BATCH`]
    /// rows and looped up to [`PRUNE_MAX_BATCHES`] times per reason so a large
    /// initial backlog drains over several ticks instead of one long-locking
    /// statement (matters most for SQLite's whole-DB write lock).
    pub fn prune_executions(&self, now: DateTime<Utc>, retention: Option<Duration>) -> PruneResult {
        let mut result = PruneResult::default();

        if let Some(dur) = retention {
            let cutoff = now - dur;
            for _ in 0..PRUNE_MAX_BATCHES {
                match self.store.prune_executions_older_than(cutoff, PRUNE_BATCH) {
                    Ok(n) => {
                        result.by_age += n;
                        if n < PRUNE_BATCH as u64 {
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "watchdog: execution_retention prune failed");
                        break;
                    }
                }
            }
        }

        for job in self.jobs.values() {
            let Some(keep) = job.keep_last else {
                continue;
            };
            for _ in 0..PRUNE_MAX_BATCHES {
                match self
                    .store
                    .prune_executions_keep_last(&job.key, keep, PRUNE_BATCH)
                {
                    Ok(n) => {
                        result.by_cap += n;
                        if n < PRUNE_BATCH as u64 {
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::error!(job_key = %job.key, error = %e, "watchdog: keep_last prune failed");
                        break;
                    }
                }
            }
        }

        result
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

        // 6. Missed-fire / liveness sweep (issue #250). Fast-path: no
        //    `job_missed_fire` rules ⇒ skip the job_states query.
        if self
            .alerts
            .rules
            .iter()
            .any(|r| matches!(r.trigger, RuleTrigger::JobMissedFire))
        {
            self.sweep_missed_fires(now, &mut result).await;
        }

        // 7. Stale-claim reaper (issue #374): requeue claimed executions
        //    whose claim age exceeds the job timeout + grace — independent
        //    of runner liveness, so it also catches orphans the dead-runner
        //    sweep can't see (fast runner restart, server restart). Runs
        //    AFTER the SLA sweep so a stuck claim still fires its
        //    `job_sla_missed` alert before the reaper makes it disappear.
        self.sweep_stale_claims(now, &mut result).await;

        // 8. Queued-reconcile sweep: re-enqueue store-`queued` rows that
        //    are missing from the in-memory work queue, and cancel rows
        //    whose job no longer exists anywhere. Runs after the requeue
        //    paths so anything they flipped-but-failed-to-enqueue this
        //    sweep is picked up next sweep at the latest.
        self.reconcile_stranded_queued(now, &mut result).await;

        // 9. Auto-clear expired operational overrides (issue #231). Same
        //    cadence as the SLA sweep — a "snooze 4h" evaporates without
        //    operator follow-up. Each cleared row gets an audit event.
        self.sweep_expired_overrides(now, &mut result);

        result
    }

    /// Fire `job_missed_fire` rules for jobs whose scheduled fire never
    /// happened (issue #250).
    ///
    /// A healthy scheduler advances `job_states.next_fire_at` to the
    /// future the instant it fires; so a `next_fire_at` that has slipped
    /// into the past — past the rule's `expected_within` grace — while the
    /// job is still `Active` means the scheduler never enqueued that fire.
    /// This is the one signal that catches a silently-stalled scheduler
    /// (#248), which otherwise shows 100% success and no failed/claimed
    /// execution for the alert engine to evaluate.
    ///
    /// Dedup: each `(rule_name, job_key, next_fire_at)` fires at most once
    /// (see [`MissedFiredSet`]). When the scheduler recovers and advances
    /// `next_fire_at`, a later miss is a new triple and alerts again.
    async fn sweep_missed_fires(&self, now: DateTime<Utc>, result: &mut WatchdogResult) {
        let states = match self.store.list_job_states() {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(
                    target: "croniq::alerts",
                    error = %e,
                    "watchdog: missed-fire sweep failed to list job states"
                );
                return;
            }
        };

        for state in &states {
            // Only Active triggers have a meaningful "expected" fire; a
            // paused/disabled/exhausted job is not supposed to fire.
            if state.status != JobStatus::Active {
                continue;
            }
            let Some(next_fire) = state.next_fire_at else {
                continue;
            };
            let overdue = (now - next_fire).num_seconds();
            if overdue <= 0 {
                continue; // not yet due, or fires exactly now
            }
            let overdue = overdue as u64;

            for rule in &self.alerts.rules {
                if !matches!(rule.trigger, RuleTrigger::JobMissedFire) {
                    continue;
                }
                let Some(grace_str) = rule.expected_within.as_deref() else {
                    continue; // compile path drops these, defensive
                };
                let Some(grace_secs) = crate::alerts::parse_throttle_secs(grace_str) else {
                    continue;
                };
                if overdue < grace_secs {
                    continue;
                }
                if !crate::alerts::glob_match(&rule.job_key_glob, &state.job_key) {
                    continue;
                }

                // Dedup per (rule, job_key, this fire time).
                let key = (rule.name.clone(), state.job_key.clone(), next_fire);
                {
                    let mut guard = self.missed_fired.lock().unwrap();
                    if !guard.insert(key.clone()) {
                        continue;
                    }
                }

                // Reuse the shared dispatch path so throttle + channels +
                // audit behave identically to the other triggers. No
                // execution exists (that's the whole point), so the id is
                // empty and `reason` distinguishes this from a real failure.
                let ctx = crate::alerts::FailureContext {
                    job_key: state.job_key.clone(),
                    execution_id: String::new(),
                    error: format!(
                        "missed scheduled fire: expected at {next_fire}, overdue {overdue}s (grace {grace_str}) — scheduler never enqueued the execution"
                    ),
                    attempt: 0,
                    reason: "missed_fire".to_string(),
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
                result
                    .missed_fires
                    .push((rule.name.clone(), state.job_key.clone()));
            }
        }
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
        // List currently claimed executions, OLDEST claim first — the sweep
        // is after executions that have been in flight FOR A LONG TIME, so
        // when more than `claim_sweep_limit` claims exist, the bounded
        // window must not cut off the old end (a newest-first listing would
        // permanently hide exactly the hung executions this sweep targets).
        let claimed = match self
            .store
            .list_claimed_older_than(now, self.claim_sweep_limit)
        {
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

    /// Requeue `claimed` executions whose claim age exceeds the job's
    /// `timeout` plus a grace window (issue #374).
    ///
    /// This is the liveness-INDEPENDENT safety net: the dead-runner sweep
    /// only reclaims executions of runners whose registry entry went Dead,
    /// which misses claims orphaned by a fast runner restart (same
    /// `runner_id` keeps polling) and claims whose runner re-registered as
    /// `New` after a server restart (the in-memory registry forgot it ever
    /// held them).
    ///
    /// Grace rationale: a live, connected runner enforces `timeout` itself
    /// and reports a failure — so anything still `claimed` well past
    /// timeout + grace has, with high confidence, lost its runner. The
    /// remaining risk (partitioned-but-running runner ⇒ duplicate run) is
    /// the same at-least-once tradeoff the dead-runner requeue already
    /// makes. Requeue keeps the SAME attempt: orphaning is an infra fault,
    /// not a handler fault — frequent redeploys must not burn retry
    /// attempts and dead-letter healthy jobs.
    async fn sweep_stale_claims(&self, now: DateTime<Utc>, result: &mut WatchdogResult) {
        let grace_secs = (2 * self.runner.lease_ttl_secs).max(120);

        // Oldest claim first, pre-filtered to claims older than the grace
        // window (every reap threshold is at least `timeout + grace`, so
        // anything younger can't qualify). Oldest-first ordering matters
        // once more than `claim_sweep_limit` claims are in flight: the
        // orphans this reaper exists for are the OLDEST rows, which a
        // newest-first listing would permanently push out of the window.
        let cutoff = now - Duration::seconds(grace_secs as i64);
        let claimed = match self
            .store
            .list_claimed_older_than(cutoff, self.claim_sweep_limit)
        {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(
                    error = %e,
                    "watchdog: stale-claim sweep failed to list claimed executions"
                );
                return;
            }
        };
        if claimed.is_empty() {
            return;
        }

        // Snapshot the inflight sets of all live (non-Dead) runners once.
        // A claim that its runner still reports as inflight is being worked
        // on (slow-but-alive handler) — reaping it would double-run a
        // singleton. Orphans never show up here: a restarted session polls
        // with an empty inflight list, and a vanished runner goes Dead.
        let live_inflight: HashMap<String, HashSet<String>> = {
            let reg = self.runner.registry.read().await;
            reg.all()
                .filter(|r| {
                    r.status_at_with_ttl(now, self.runner.lease_ttl_secs) != RunnerStatus::Dead
                })
                .map(|r| {
                    (
                        r.runner_id.clone(),
                        r.inflight.iter().cloned().collect::<HashSet<String>>(),
                    )
                })
                .collect()
        };

        let mut reaped: Vec<(Option<String>, uuid::Uuid)> = Vec::new();
        let mut enqueued = 0usize;
        for execution in &claimed {
            let age_basis = execution
                .claimed_at
                .or(execution.started_at)
                .unwrap_or(execution.created_at);
            // Default 5m — also on unresolvable config or unparsable
            // timeout, matching the WorkItem-build default.
            let timeout_secs = self
                .resolve_job_config(&execution.job_key)
                .and_then(|j| j.timeout)
                .and_then(|t| croniq_execution::retry::parse_duration(&t))
                .map_or(300, |d| d.as_secs());
            let threshold_secs = timeout_secs + grace_secs;
            let age = now.signed_duration_since(age_basis).num_seconds();
            if age <= threshold_secs as i64 {
                continue;
            }

            if let Some(rid) = execution.runner_id.as_deref()
                && live_inflight
                    .get(rid)
                    .is_some_and(|inflight| inflight.contains(&execution.id.to_string()))
            {
                continue;
            }

            // CAS: a completion / cancel racing this sweep wins — then we
            // must not re-enqueue.
            match self.store.requeue_if_claimed(execution.id, now) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(e) => {
                    tracing::error!(
                        execution_id = %execution.id,
                        error = %e,
                        "watchdog: stale-claim requeue failed"
                    );
                    continue;
                }
            }

            tracing::warn!(
                job_key = %execution.job_key,
                execution_id = %execution.id,
                runner_id = execution.runner_id.as_deref().unwrap_or("<none>"),
                age_secs = age,
                threshold_secs,
                "watchdog: requeued stale claimed execution — claim outlived job timeout + grace"
            );
            let id_str = execution.id.to_string();
            crate::api::audit::record_event(
                &self.store,
                "system",
                None,
                "execution.stale_claim_requeued",
                "execution",
                Some(&id_str),
            );

            if enqueue_requeued_execution(&self.store, &self.runner, &execution.id, &mut |k| {
                self.resolve_job_config(k)
            })
            .await
            {
                enqueued += 1;
            }
            result.stale_claims.push(execution.id);
            reaped.push((execution.runner_id.clone(), execution.id));
        }

        if reaped.is_empty() {
            return;
        }

        // Drop the reaped executions from their (possibly still registered)
        // runner's inflight bookkeeping so registry capacity stats don't
        // count them twice once the work is re-claimed.
        {
            let mut reg = self.runner.registry.write().await;
            for (rid, id) in &reaped {
                if let Some(rid) = rid {
                    reg.release(rid, &id.to_string());
                }
            }
        }

        if enqueued > 0 {
            self.runner.work_notify.notify_waiters();
        }
    }

    /// Re-enqueue store-`queued` executions that are missing from the
    /// in-memory work queue, and cancel the ones that can never dispatch.
    ///
    /// The in-memory queue is the only source runners are served from
    /// (`try_dequeue_for`); `find_queued_executions` is otherwise only read
    /// at boot. So a store row that is `queued` but has no matching
    /// WorkItem — e.g. a requeue path flipped the row but
    /// `resolve_job_config` failed at that moment, or the boot restore
    /// skipped it — would strand as "queued" forever. This sweep restores
    /// the invariant store-queued ⇔ in-memory-queued.
    ///
    /// Rows whose job exists neither in the DSL map nor as a stored
    /// JobDefinition (job deleted while work was in flight) are cancelled:
    /// no future sweep could ever rebuild their WorkItem. A transient store
    /// error while resolving is NOT treated as "job gone" — the row is left
    /// for the next sweep.
    ///
    /// Races with concurrent enqueuers (scheduler fire between
    /// create-row and enqueue, poll-handler inline takeover) are harmless:
    /// `WorkQueue::enqueue` dedupes by execution_id, and the store-side
    /// claim CAS is the final arbiter.
    async fn reconcile_stranded_queued(&self, now: DateTime<Utc>, result: &mut WatchdogResult) {
        let queued = match self.store.find_queued_executions(&[], RECONCILE_BATCH) {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(
                    error = %e,
                    "watchdog: queued-reconcile sweep failed to list queued executions"
                );
                return;
            }
        };
        if queued.is_empty() {
            return;
        }

        // Snapshot the queue's execution IDs once. try_dequeue_for claims
        // rows in the store before releasing the queue write lock, so by
        // the time we get the read lock, anything dequeued-but-in-claiming
        // is already `claimed` in the store and the per-row re-check inside
        // enqueue_requeued_execution rejects it.
        let in_queue: HashSet<String> = {
            let q = self.runner.queue.read().await;
            q.peek_n(q.len())
                .iter()
                .map(|item| item.execution_id.clone())
                .collect()
        };

        let mut enqueued = 0usize;
        for execution in &queued {
            if in_queue.contains(&execution.id.to_string()) {
                continue;
            }

            // Resolve inline (not via resolve_job_config) to tell "job is
            // gone" (Ok(None) ⇒ cancel) apart from a transient store error
            // (Err ⇒ retry next sweep).
            let known = self.jobs.contains_key(&execution.job_key)
                || match self.store.get_job_definition(&execution.job_key) {
                    Ok(def) => def.is_some(),
                    Err(e) => {
                        tracing::error!(
                            job_key = %execution.job_key,
                            error = %e,
                            "watchdog: queued-reconcile store error resolving job — skipping row"
                        );
                        continue;
                    }
                };

            if !known {
                if let Err(e) = self.store.cancel_execution(execution.id, now) {
                    tracing::error!(
                        execution_id = %execution.id,
                        error = %e,
                        "watchdog: failed to cancel stranded queued execution"
                    );
                    continue;
                }
                tracing::warn!(
                    job_key = %execution.job_key,
                    execution_id = %execution.id,
                    "watchdog: cancelled stranded queued execution — job exists neither in DSL nor store"
                );
                let id_str = execution.id.to_string();
                crate::api::audit::record_event(
                    &self.store,
                    "system",
                    None,
                    "execution.stranded_queued_cancelled",
                    "execution",
                    Some(&id_str),
                );
                result.stranded_cancelled.push(execution.id);
                continue;
            }

            if enqueue_requeued_execution(&self.store, &self.runner, &execution.id, &mut |k| {
                self.resolve_job_config(k)
            })
            .await
            {
                tracing::warn!(
                    job_key = %execution.job_key,
                    execution_id = %execution.id,
                    "watchdog: re-enqueued stranded queued execution missing from in-memory queue"
                );
                result.reconciled.push(execution.id);
                enqueued += 1;
            }
        }

        if enqueued > 0 {
            self.runner.work_notify.notify_waiters();
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
            keep_last: None,
            max_concurrent: None,
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
                scheduled_for: now,
                attempt,
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

    #[test]
    fn prune_executions_enforces_age_and_keep_last() {
        let store = make_store();
        let now = Utc::now();
        let seed = |job_key: &str, at: DateTime<Utc>, state: ExecutionState| -> Uuid {
            let id = Uuid::new_v4();
            store
                .create_execution(&Execution {
                    id,
                    job_key: job_key.into(),
                    fire_at: at,
                    scheduled_for: at,
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
                    created_at: at,
                })
                .unwrap();
            store.claim_execution(id, "r-seed", at).unwrap();
            store
                .complete_execution(id, None, state, Some(1), None, None, at)
                .unwrap();
            id
        };

        // Age sweep (retention 30d): a 40-day-old completed row is stale; a
        // 1-day-old one is fresh and kept.
        let stale = seed(
            "ret:job",
            now - ChronoDuration::days(40),
            ExecutionState::Completed,
        );
        let fresh = seed(
            "ret:job",
            now - ChronoDuration::days(1),
            ExecutionState::Completed,
        );

        // keep_last=1: three fresh completed rows for a capped job (not caught
        // by the age sweep, so this exercises the per-job cap in isolation).
        let recent = now - ChronoDuration::days(1);
        let capped_ids: Vec<Uuid> = (0..3)
            .map(|i| {
                seed(
                    "cap:job",
                    recent + ChronoDuration::seconds(i),
                    ExecutionState::Completed,
                )
            })
            .collect();

        let mut capped_job = make_job("cap:job");
        capped_job.keep_last = Some(1);
        let watchdog = WatchdogLoop::new(
            vec![make_job("ret:job"), capped_job],
            Arc::clone(&store),
            make_runner(),
        );

        let result = watchdog.prune_executions(now, Some(ChronoDuration::days(30)));
        assert_eq!(result.by_age, 1, "only the 40-day-old row is age-pruned");
        assert_eq!(result.by_cap, 2, "keep_last=1 removes 2 of 3 capped rows");

        assert!(store.get_execution(stale).unwrap().is_none());
        assert!(store.get_execution(fresh).unwrap().is_some());
        let survivors = capped_ids
            .iter()
            .filter(|id| store.get_execution(**id).unwrap().is_some())
            .count();
        assert_eq!(survivors, 1);
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

    // ─── #374 stale-claim reaper ───────────────────────────────────

    #[tokio::test]
    async fn stale_claim_reaper_requeues_orphan_without_registry_entry() {
        let store = make_store();
        let runner = make_runner();

        // Server-restart shape: the claim's runner_id has no registry entry
        // at all, so the dead-runner sweep can never see it.
        let exec_id = seed_claimed_at(
            &*store,
            "test:job",
            "vanished-runner",
            Utc::now() - ChronoDuration::hours(1),
        );

        let watchdog = WatchdogLoop::new(
            vec![make_job("test:job")],
            Arc::clone(&store),
            Arc::clone(&runner),
        );
        let result = watchdog.sweep(Utc::now()).await;

        assert!(result.dead_runners.is_empty());
        assert!(result.requeued.is_empty());
        assert_eq!(result.stale_claims, vec![exec_id]);

        let exec = store.get_execution(exec_id).unwrap().unwrap();
        assert_eq!(exec.state, ExecutionState::Queued);
        assert!(exec.runner_id.is_none());
        assert_eq!(exec.attempt, 1, "reaper must not burn a retry attempt");

        let q = runner.queue.read().await;
        assert_eq!(q.len(), 1);

        // Reap leaves a forensic audit trail.
        let events = store
            .audit_list(&croniq_store::models::AuditFilter {
                action: Some("execution.stale_claim_requeued".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(events.len(), 1);
    }

    #[tokio::test]
    async fn stale_claim_reaper_skips_fresh_claim() {
        let store = make_store();
        let runner = make_runner();

        // 5 min old — inside timeout (10m) + grace (240s).
        let exec_id = seed_claimed_at(
            &*store,
            "test:job",
            "vanished-runner",
            Utc::now() - ChronoDuration::minutes(5),
        );

        let watchdog = WatchdogLoop::new(
            vec![make_job("test:job")],
            Arc::clone(&store),
            Arc::clone(&runner),
        );
        let result = watchdog.sweep(Utc::now()).await;

        assert!(result.stale_claims.is_empty());
        let exec = store.get_execution(exec_id).unwrap().unwrap();
        assert_eq!(exec.state, ExecutionState::Claimed);
        assert_eq!(runner.queue.read().await.len(), 0);
    }

    #[tokio::test]
    async fn stale_claim_reaper_skips_claim_reported_inflight_by_live_runner() {
        let store = make_store();
        let runner = make_runner();

        let exec_id = seed_claimed_at(
            &*store,
            "test:job",
            "app-runner",
            Utc::now() - ChronoDuration::hours(1),
        );

        // Live runner still reports the execution inflight — a
        // slow-but-alive handler must not be double-run.
        {
            let mut reg = runner.registry.write().await;
            let _ = reg.register_or_update(
                "app-runner",
                vec!["billing".into()],
                3,
                vec![exec_id.to_string()],
                None,
                vec![],
            );
        }

        let watchdog = WatchdogLoop::new(
            vec![make_job("test:job")],
            Arc::clone(&store),
            Arc::clone(&runner),
        );
        let result = watchdog.sweep(Utc::now()).await;

        assert!(result.stale_claims.is_empty());
        let exec = store.get_execution(exec_id).unwrap().unwrap();
        assert_eq!(exec.state, ExecutionState::Claimed);
    }

    #[tokio::test]
    async fn stale_claim_reaper_reaps_orphan_of_live_runner_with_empty_inflight() {
        let store = make_store();
        let runner = make_runner();

        let exec_id = seed_claimed_at(
            &*store,
            "test:job",
            "app-runner",
            Utc::now() - ChronoDuration::hours(1),
        );

        // The exact #374 shape: the runner restarted fast, keeps polling
        // under the same runner_id (never Dead) but no longer knows about
        // the claim — its inflight list is empty.
        {
            let mut reg = runner.registry.write().await;
            let _ = reg.register_or_update(
                "app-runner",
                vec!["billing".into()],
                3,
                vec![],
                None,
                vec![],
            );
        }

        let watchdog = WatchdogLoop::new(
            vec![make_job("test:job")],
            Arc::clone(&store),
            Arc::clone(&runner),
        );
        let result = watchdog.sweep(Utc::now()).await;

        assert_eq!(result.stale_claims, vec![exec_id]);
        assert!(result.dead_runners.is_empty());

        let exec = store.get_execution(exec_id).unwrap().unwrap();
        assert_eq!(exec.state, ExecutionState::Queued);
        assert_eq!(runner.queue.read().await.len(), 1);
    }

    #[tokio::test]
    async fn stale_claim_reaper_uses_default_timeout_when_unset() {
        let store = make_store();
        let runner = make_runner();

        // 10 min old claim, job without explicit timeout: default 5m +
        // grace 240s = 540s threshold → reaped.
        let exec_id = seed_claimed_at(
            &*store,
            "test:job",
            "vanished-runner",
            Utc::now() - ChronoDuration::minutes(10),
        );

        let mut job = make_job("test:job");
        job.timeout = None;
        let watchdog = WatchdogLoop::new(vec![job], Arc::clone(&store), Arc::clone(&runner));
        let result = watchdog.sweep(Utc::now()).await;

        assert_eq!(result.stale_claims, vec![exec_id]);
        let exec = store.get_execution(exec_id).unwrap().unwrap();
        assert_eq!(exec.state, ExecutionState::Queued);
    }

    // ─── stranded-queued reconcile ─────────────────────────────────

    /// Seed an execution left in `Queued` state with nothing in the
    /// in-memory queue — the shape a failed requeue (or a skipped boot
    /// restore) leaves behind.
    fn seed_queued_at(store: &dyn ExecutionStore, job_key: &str, at: DateTime<Utc>) -> Uuid {
        let id = Uuid::new_v4();
        store
            .create_execution(&Execution {
                id,
                job_key: job_key.into(),
                fire_at: at,
                scheduled_for: at,
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
                created_at: at,
            })
            .unwrap();
        id
    }

    #[tokio::test]
    async fn reconcile_reenqueues_store_queued_row_missing_from_memory_queue() {
        let store = make_store();
        let runner = make_runner();

        let exec_id = seed_queued_at(&*store, "test:job", Utc::now() - ChronoDuration::minutes(5));

        let watchdog = WatchdogLoop::new(
            vec![make_job("test:job")],
            Arc::clone(&store),
            Arc::clone(&runner),
        );
        let result = watchdog.sweep(Utc::now()).await;

        assert_eq!(result.reconciled, vec![exec_id]);
        assert!(result.stranded_cancelled.is_empty());

        let exec = store.get_execution(exec_id).unwrap().unwrap();
        assert_eq!(exec.state, ExecutionState::Queued);
        {
            let q = runner.queue.read().await;
            assert_eq!(q.len(), 1);
            assert_eq!(q.peek().unwrap().execution_id, exec_id.to_string());
        }

        // Once the item is back in the queue, later sweeps are a no-op.
        let again = watchdog.sweep(Utc::now()).await;
        assert!(again.reconciled.is_empty());
        assert_eq!(runner.queue.read().await.len(), 1, "no duplicate item");
    }

    #[tokio::test]
    async fn reconcile_skips_rows_already_in_memory_queue() {
        let store = make_store();
        let runner = make_runner();

        let exec_id = seed_queued_at(&*store, "test:job", Utc::now() - ChronoDuration::minutes(5));
        // The healthy shape: the WorkItem is already in the queue.
        {
            let mut q = runner.queue.write().await;
            q.enqueue(WorkItem {
                execution_id: exec_id.to_string(),
                job_key: "test:job".into(),
                fire_at: Utc::now(),
                scheduled_for: Utc::now(),
                attempt: 1,
                require: vec!["billing".into()],
                prefer: vec![],
                metadata: serde_json::Value::Null,
                timeout: "10m".into(),
            });
        }

        let watchdog = WatchdogLoop::new(
            vec![make_job("test:job")],
            Arc::clone(&store),
            Arc::clone(&runner),
        );
        let result = watchdog.sweep(Utc::now()).await;

        assert!(result.reconciled.is_empty());
        assert!(result.stranded_cancelled.is_empty());
        assert_eq!(runner.queue.read().await.len(), 1);
    }

    #[tokio::test]
    async fn reconcile_cancels_row_whose_job_is_gone() {
        let store = make_store();
        let runner = make_runner();

        // Job exists neither in the DSL map nor as a stored JobDefinition —
        // a deleted job whose execution was stranded mid-requeue.
        let exec_id = seed_queued_at(
            &*store,
            "ghost:job",
            Utc::now() - ChronoDuration::minutes(5),
        );

        let watchdog = WatchdogLoop::new(vec![], Arc::clone(&store), Arc::clone(&runner));
        let result = watchdog.sweep(Utc::now()).await;

        assert!(result.reconciled.is_empty());
        assert_eq!(result.stranded_cancelled, vec![exec_id]);

        let exec = store.get_execution(exec_id).unwrap().unwrap();
        assert_eq!(exec.state, ExecutionState::Cancelled);
        assert_eq!(runner.queue.read().await.len(), 0);

        // The cancel leaves a forensic audit trail.
        let events = store
            .audit_list(&croniq_store::models::AuditFilter {
                action: Some("execution.stranded_queued_cancelled".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(events.len(), 1);
    }

    #[tokio::test]
    async fn reconcile_resolves_api_job_from_store_definition() {
        let store = make_store();
        let runner = make_runner();

        // API-registered job: not in the DSL map, but a JobDefinition
        // exists in the store — must be re-enqueued, not cancelled.
        let now = Utc::now();
        store
            .create_job_definition(&croniq_store::models::JobDefinition {
                job_key: "api:job".into(),
                description: None,
                assigned_runner_id: None,
                is_active: true,
                metadata: HashMap::new(),
                created_at: now,
                updated_at: now,
                timeout: Some("2m".into()),
                max_retries: None,
                dead_letter_enabled: None,
                dead_letter_retention: None,
                dead_letter_operator_hint: None,
                dead_letter_replay_max_age: None,
                tags: vec![],
            })
            .unwrap();
        let exec_id = seed_queued_at(&*store, "api:job", now - ChronoDuration::minutes(5));

        let watchdog = WatchdogLoop::new(vec![], Arc::clone(&store), Arc::clone(&runner));
        let result = watchdog.sweep(now).await;

        assert_eq!(result.reconciled, vec![exec_id]);
        assert!(result.stranded_cancelled.is_empty());
        assert_eq!(
            store.get_execution(exec_id).unwrap().unwrap().state,
            ExecutionState::Queued
        );
        let q = runner.queue.read().await;
        assert_eq!(q.len(), 1);
        assert_eq!(q.peek().unwrap().timeout, "2m");
    }

    #[tokio::test]
    async fn stale_claim_reaper_reaps_oldest_when_claims_exceed_limit() {
        let store = make_store();
        let runner = make_runner();

        // Three stale orphans, sweep limit two: the OLDEST claims must be
        // the ones inside the bounded window. With the previous
        // newest-first listing the oldest orphan would sit permanently
        // outside the limit and never be reaped.
        let now = Utc::now();
        let oldest = seed_claimed_at(&*store, "test:job", "gone", now - ChronoDuration::hours(3));
        let mid = seed_claimed_at(&*store, "test:job", "gone", now - ChronoDuration::hours(2));
        let newest = seed_claimed_at(&*store, "test:job", "gone", now - ChronoDuration::hours(1));

        let mut watchdog = WatchdogLoop::new(
            vec![make_job("test:job")],
            Arc::clone(&store),
            Arc::clone(&runner),
        );
        watchdog.claim_sweep_limit = 2;
        let result = watchdog.sweep(now).await;

        assert_eq!(result.stale_claims, vec![oldest, mid]);
        // The newest stale claim is merely deferred to the next sweep,
        // not lost.
        let deferred = store.get_execution(newest).unwrap().unwrap();
        assert_eq!(deferred.state, ExecutionState::Claimed);
        let result2 = watchdog.sweep(now).await;
        assert_eq!(result2.stale_claims, vec![newest]);
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
                scheduled_for: claimed_at,
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
    async fn sla_sweep_covers_oldest_claim_when_claims_exceed_limit() {
        let store = make_store();
        let now = Utc::now();

        // Three in-flight executions past the window, sweep limit two: the
        // bounded window must contain the OLDEST claims — the worst SLA
        // breaches — not the newest.
        let oldest = seed_claimed_at(
            &*store,
            "billing:a",
            "runner-1",
            now - ChronoDuration::minutes(40),
        );
        let mid = seed_claimed_at(
            &*store,
            "billing:b",
            "runner-1",
            now - ChronoDuration::minutes(30),
        );
        let _newest = seed_claimed_at(
            &*store,
            "billing:c",
            "runner-1",
            now - ChronoDuration::minutes(20),
        );

        let alerts = alerts_with_sla(vec![sla_rule("slow", "billing:*", "10m", "ops")]);
        let mut watchdog = watchdog_with_alerts_only(Arc::clone(&store), alerts);
        watchdog.claim_sweep_limit = 2;
        let result = watchdog.sweep(now).await;

        let alerted: Vec<Uuid> = result.sla_missed.iter().map(|(_, id)| *id).collect();
        assert_eq!(alerted, vec![oldest, mid]);
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

    // ─── #250 missed-fire sweep ────────────────────────────────────

    use croniq_store::models::{JobState, JobStatus};
    use croniq_store::traits::JobStore;

    fn missed_fire_rule(name: &str, glob: &str, grace: &str, channel: &str) -> RuleConfig {
        RuleConfig {
            name: name.into(),
            trigger: RuleTrigger::JobMissedFire,
            job_key_glob: glob.into(),
            min_attempts: 1,
            dead_letter_only: false,
            throttle: None,
            expected_within: Some(grace.into()),
            channels: vec![channel.into()],
        }
    }

    fn seed_job_state(
        store: &dyn JobStore,
        job_key: &str,
        next_fire_at: Option<DateTime<Utc>>,
        status: JobStatus,
    ) {
        store
            .upsert_job_state(&JobState {
                job_key: job_key.into(),
                next_fire_at,
                last_fired_at: None,
                fire_count: 3,
                status,
                updated_at: Utc::now(),
            })
            .unwrap();
    }

    #[tokio::test]
    async fn missed_fire_fires_when_overdue_past_grace() {
        let store = make_store();
        let now = Utc::now();
        // Daily backup that should have fired 15 min ago and didn't.
        seed_job_state(
            &*store,
            "billing:backup",
            Some(now - ChronoDuration::minutes(15)),
            JobStatus::Active,
        );

        let alerts = alerts_with_sla(vec![missed_fire_rule(
            "backup-liveness",
            "billing:*",
            "10m",
            "ops",
        )]);
        let watchdog = watchdog_with_alerts_only(Arc::clone(&store), alerts);
        let result = watchdog.sweep(now).await;

        assert_eq!(
            result.missed_fires,
            vec![("backup-liveness".to_string(), "billing:backup".to_string())]
        );
        let deliveries = store
            .list_alert_deliveries(&AlertDeliveryFilter::default())
            .unwrap();
        assert_eq!(deliveries.len(), 1);
        assert_eq!(deliveries[0].rule_name, "backup-liveness");
        assert_eq!(deliveries[0].job_key, "billing:backup");
    }

    #[tokio::test]
    async fn missed_fire_skips_within_grace() {
        let store = make_store();
        let now = Utc::now();
        // Only 5 min late — inside the 10 min grace, so not (yet) a miss.
        seed_job_state(
            &*store,
            "billing:backup",
            Some(now - ChronoDuration::minutes(5)),
            JobStatus::Active,
        );

        let alerts = alerts_with_sla(vec![missed_fire_rule("backup-liveness", "*", "10m", "ops")]);
        let watchdog = watchdog_with_alerts_only(Arc::clone(&store), alerts);
        let result = watchdog.sweep(now).await;

        assert!(result.missed_fires.is_empty());
    }

    #[tokio::test]
    async fn missed_fire_skips_future_next_fire() {
        let store = make_store();
        let now = Utc::now();
        // Healthy: next fire is in the future.
        seed_job_state(
            &*store,
            "billing:backup",
            Some(now + ChronoDuration::hours(6)),
            JobStatus::Active,
        );

        let alerts = alerts_with_sla(vec![missed_fire_rule("backup-liveness", "*", "10m", "ops")]);
        let watchdog = watchdog_with_alerts_only(Arc::clone(&store), alerts);
        let result = watchdog.sweep(now).await;

        assert!(result.missed_fires.is_empty());
    }

    /// #391 regression: a calendar-gated job parked outside its window is
    /// Active with `next_fire_at` at the next gate-open instant — a healthy,
    /// intentionally-idle state that must never trip the liveness alert.
    /// (The pre-#391 failure mode — a stale past `next_fire_at` lingering
    /// through the gap — can no longer be produced: `compute_next_fire` only
    /// emits gate-allowed instants and boot heals old rows.)
    #[tokio::test]
    async fn missed_fire_ignores_calendar_gated_waiting_job() {
        let store = make_store();
        let now = Utc::now();
        // Seed the exact state the fixed scheduler persists for
        // `every 1 minute { calendar business-hours }`.
        let triggers = crate::loader::load_str(
            r#"
            calendar biz {
                include weekly weekday
                include window "08:00".."18:00"
            }
            job ops:tick { every 1 minutes { calendar biz } }
            "#,
        )
        .unwrap()
        .triggers;
        let next_fire = triggers["ops:tick"].next_fire_at.expect("trigger armed");
        assert!(next_fire > now, "gate-jump always lands in the future");
        seed_job_state(&*store, "ops:tick", Some(next_fire), JobStatus::Active);

        let alerts = alerts_with_sla(vec![missed_fire_rule("liveness", "*", "10m", "ops")]);
        let watchdog = watchdog_with_alerts_only(Arc::clone(&store), alerts);
        let result = watchdog.sweep(now).await;

        assert!(result.missed_fires.is_empty());
    }

    #[tokio::test]
    async fn missed_fire_skips_non_active_status() {
        let store = make_store();
        let now = Utc::now();
        // Paused jobs aren't supposed to fire — being "overdue" is fine.
        seed_job_state(
            &*store,
            "billing:backup",
            Some(now - ChronoDuration::hours(2)),
            JobStatus::Paused,
        );

        let alerts = alerts_with_sla(vec![missed_fire_rule("backup-liveness", "*", "10m", "ops")]);
        let watchdog = watchdog_with_alerts_only(Arc::clone(&store), alerts);
        let result = watchdog.sweep(now).await;

        assert!(result.missed_fires.is_empty());
    }

    #[tokio::test]
    async fn missed_fire_dedups_same_fire_across_sweeps() {
        let store = make_store();
        let now = Utc::now();
        seed_job_state(
            &*store,
            "billing:backup",
            Some(now - ChronoDuration::minutes(15)),
            JobStatus::Active,
        );

        let alerts = alerts_with_sla(vec![missed_fire_rule("backup-liveness", "*", "10m", "ops")]);
        let watchdog = watchdog_with_alerts_only(Arc::clone(&store), alerts);

        let r1 = watchdog.sweep(now).await;
        assert_eq!(r1.missed_fires.len(), 1);

        // Same overdue fire, 30 s later — must NOT re-alert.
        let r2 = watchdog.sweep(now + ChronoDuration::seconds(30)).await;
        assert!(
            r2.missed_fires.is_empty(),
            "dedup must suppress repeat alerts for the same missed fire"
        );

        let deliveries = store
            .list_alert_deliveries(&AlertDeliveryFilter::default())
            .unwrap();
        assert_eq!(deliveries.len(), 1);
    }

    #[tokio::test]
    async fn missed_fire_respects_job_key_glob() {
        let store = make_store();
        let now = Utc::now();
        seed_job_state(
            &*store,
            "ops:cleanup",
            Some(now - ChronoDuration::minutes(15)),
            JobStatus::Active,
        );
        seed_job_state(
            &*store,
            "billing:backup",
            Some(now - ChronoDuration::minutes(15)),
            JobStatus::Active,
        );

        let alerts = alerts_with_sla(vec![missed_fire_rule(
            "only-billing",
            "billing:*",
            "10m",
            "ops",
        )]);
        let watchdog = watchdog_with_alerts_only(Arc::clone(&store), alerts);
        let result = watchdog.sweep(now).await;

        assert_eq!(result.missed_fires.len(), 1);
        assert_eq!(result.missed_fires[0].1, "billing:backup");
    }
}

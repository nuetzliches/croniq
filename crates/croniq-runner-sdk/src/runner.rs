//! CroniqRunner: the main orchestrator for job execution runners.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use tokio::sync::RwLock;

use crate::client::{AckRequest, CroniqClient, PollRequest, RegisterJobRequest, RenewRequest};
use crate::handler::{ExecutionContext, HandlerError, HandlerRegistry};

/// A job schedule to register on the server at startup.
struct JobSchedule {
    job_key: String,
    schedule: String,
}

/// Outcome of feeding a poll result through the conflict-streak
/// tracker. Returned by [`update_conflict_streak`] so the run-loop can
/// either retry (with backoff) or exit fatally.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PollLoopAction {
    /// Continue polling after the configured retry delay.
    Continue,
    /// Stop the loop and propagate the conflict as a fatal error.
    BailOut,
}

/// Update the consecutive-409 counter based on the latest poll result
/// and decide whether to keep polling. Extracted for unit-testing —
/// the run-loop in [`CroniqRunner::start`] calls this on every poll
/// outcome.
///
/// * Successful polls reset the counter (the conflict resolved itself).
/// * Non-409 transient errors reset the counter too (a 5xx is unrelated
///   to instance ownership; counting it would bail prematurely).
/// * 409 conflicts increment and trip [`PollLoopAction::BailOut`] at the
///   configured threshold.
pub(crate) fn update_conflict_streak(
    result: &Result<crate::client::PollResponse, crate::client::ClientError>,
    consecutive: &mut u32,
    max_consecutive: u32,
) -> PollLoopAction {
    match result {
        Ok(_) => {
            *consecutive = 0;
            PollLoopAction::Continue
        }
        Err(crate::client::ClientError::PollInstanceConflict { .. }) => {
            *consecutive = consecutive.saturating_add(1);
            if *consecutive >= max_consecutive {
                PollLoopAction::BailOut
            } else {
                PollLoopAction::Continue
            }
        }
        Err(_) => {
            *consecutive = 0;
            PollLoopAction::Continue
        }
    }
}

/// Builder for constructing a CroniqRunner.
pub struct RunnerBuilder {
    server_url: String,
    runner_id: String,
    api_key: Option<String>,
    capabilities: Vec<String>,
    max_inflight: u32,
    tags: Vec<String>,
    poll_retry_delay: Duration,
    capacity_backoff: Duration,
    max_consecutive_poll_conflicts: u32,
}

impl RunnerBuilder {
    pub fn api_key(mut self, key: &str) -> Self {
        self.api_key = Some(key.to_string());
        self
    }

    pub fn capabilities(mut self, caps: Vec<String>) -> Self {
        self.capabilities = caps;
        self
    }

    pub fn max_inflight(mut self, n: u32) -> Self {
        self.max_inflight = n;
        self
    }

    /// Free-form tags self-declared by the runner. Filter-only — does not
    /// influence routing (capabilities do that). Convention: `key=value`
    /// strings (`env=prod`, `team=ops`) but plain labels are equally valid.
    pub fn tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    /// How long to wait after a transient poll error (non-409 HTTP failure,
    /// timeout, network error) before retrying. Default: 5 seconds. The
    /// runner waits this long between retries, then resumes polling.
    pub fn poll_retry_delay(mut self, delay: Duration) -> Self {
        self.poll_retry_delay = delay;
        self
    }

    /// Idle delay between polls when the runner is at `max_inflight`
    /// capacity. Default: 5 seconds.
    ///
    /// Even at capacity the SDK keeps polling so the server can deliver
    /// admin-issued cancels via `PollResponse.cancel` (issue #176). The
    /// at-capacity branch returns immediately on the server side
    /// (capacity=0), so this is what paces the loop and prevents a
    /// stampede. A long-poll cadence (~5 s, well under the 35 s the
    /// server long-polls on the normal path) trades a small extra
    /// load on the server for sub-5 s cancel-delivery latency on a
    /// single-slot runner.
    pub fn capacity_backoff(mut self, delay: Duration) -> Self {
        self.capacity_backoff = delay;
        self
    }

    /// Maximum number of consecutive `409 Conflict` responses from the
    /// poll endpoint before the runner gives up and exits with a fatal
    /// [`ClientError::PollInstanceConflict`]. Default: 3.
    ///
    /// A 409 means another process is already registered with the same
    /// `runner_id` (instance guard); retrying forever just masks an
    /// operator misconfiguration. The counter resets on any successful
    /// poll or a non-409 transient error.
    pub fn max_consecutive_poll_conflicts(mut self, n: u32) -> Self {
        self.max_consecutive_poll_conflicts = n;
        self
    }

    pub fn build(self) -> CroniqRunner {
        let mut client = CroniqClient::new(&self.server_url);
        if let Some(key) = &self.api_key {
            client = client.with_api_key(key);
        }

        CroniqRunner {
            client: Arc::new(client),
            runner_id: self.runner_id,
            capabilities: self.capabilities,
            max_inflight: self.max_inflight,
            tags: self.tags,
            instance_id: uuid::Uuid::new_v4().to_string(),
            poll_retry_delay: self.poll_retry_delay,
            capacity_backoff: self.capacity_backoff,
            max_consecutive_poll_conflicts: self.max_consecutive_poll_conflicts,
            handlers: Arc::new(RwLock::new(HandlerRegistry::new())),
            schedules: Arc::new(RwLock::new(Vec::new())),
            inflight: Arc::new(RwLock::new(Vec::new())),
            draining: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
}

/// The main runner orchestrator.
pub struct CroniqRunner {
    client: Arc<CroniqClient>,
    runner_id: String,
    capabilities: Vec<String>,
    max_inflight: u32,
    tags: Vec<String>,
    instance_id: String,
    poll_retry_delay: Duration,
    capacity_backoff: Duration,
    max_consecutive_poll_conflicts: u32,
    handlers: Arc<RwLock<HandlerRegistry>>,
    schedules: Arc<RwLock<Vec<JobSchedule>>>,
    inflight: Arc<RwLock<Vec<String>>>,
    draining: Arc<std::sync::atomic::AtomicBool>,
}

impl CroniqRunner {
    pub fn builder(server_url: &str, runner_id: &str) -> RunnerBuilder {
        RunnerBuilder {
            server_url: server_url.to_string(),
            runner_id: runner_id.to_string(),
            api_key: None,
            capabilities: Vec::new(),
            max_inflight: 5,
            tags: Vec::new(),
            poll_retry_delay: Duration::from_secs(5),
            capacity_backoff: Duration::from_secs(5),
            max_consecutive_poll_conflicts: 3,
        }
    }

    /// Register a handler for a specific job key (job must already exist on server).
    pub async fn register<F, Fut>(&self, job_key: &str, handler: F)
    where
        F: Fn(ExecutionContext) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<(), HandlerError>> + Send + 'static,
    {
        self.handlers.write().await.register(job_key, handler);
    }

    /// Register a handler AND its schedule on the server.
    ///
    /// The server will create the job + trigger if they don't exist.
    /// If the job is already managed by the Croniqfile, the schedule is skipped
    /// (DSL has precedence).
    pub async fn register_with_schedule<F, Fut>(&self, job_key: &str, schedule: &str, handler: F)
    where
        F: Fn(ExecutionContext) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<(), HandlerError>> + Send + 'static,
    {
        self.handlers.write().await.register(job_key, handler);
        self.schedules.write().await.push(JobSchedule {
            job_key: job_key.to_string(),
            schedule: schedule.to_string(),
        });
    }

    /// Register a catch-all handler invoked when no specific handler matches the job key.
    pub async fn set_default_handler<F, Fut>(&self, handler: F)
    where
        F: Fn(ExecutionContext) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<(), HandlerError>> + Send + 'static,
    {
        self.handlers.write().await.set_default(handler);
    }

    /// Signal graceful shutdown: stop accepting new work, wait for inflight.
    pub fn drain(&self) {
        self.draining
            .store(true, std::sync::atomic::Ordering::Relaxed);
        tracing::info!(runner_id = %self.runner_id, "draining — no new work will be accepted");
    }

    /// Start the runner: auto-register jobs, then poll loop + lease renewal.
    pub async fn start(&self) -> Result<(), crate::client::ClientError> {
        tracing::info!(
            runner_id = %self.runner_id,
            capabilities = ?self.capabilities,
            max_inflight = self.max_inflight,
            "runner starting"
        );

        // Auto-register jobs with schedules on the server
        let schedules = self.schedules.read().await;
        for sched in schedules.iter() {
            tracing::info!(job_key = %sched.job_key, schedule = %sched.schedule, "registering job on server");
            match self
                .client
                .register_job(&RegisterJobRequest {
                    job_key: sched.job_key.clone(),
                    schedule: sched.schedule.clone(),
                    timezone: None,
                    timeout: None,
                    runner_id: Some(self.runner_id.clone()),
                    capabilities: self.capabilities.clone(),
                    description: None,
                })
                .await
            {
                Ok(()) => tracing::info!(job_key = %sched.job_key, "job registered"),
                Err(e) => {
                    tracing::warn!(job_key = %sched.job_key, error = %e, "failed to register job — will still poll")
                }
            }
        }
        drop(schedules);

        // Tracks consecutive `409 Conflict` responses on poll. Reset on
        // any successful poll or non-409 transient error. When this hits
        // `max_consecutive_poll_conflicts` we bail out of the loop with
        // a fatal error so the host process can exit non-zero — see
        // #134 sub-item 1.
        let mut consecutive_conflicts: u32 = 0;

        loop {
            if self.draining.load(std::sync::atomic::Ordering::Relaxed) {
                let inflight = self.inflight.read().await;
                if inflight.is_empty() {
                    tracing::info!("drain complete — all inflight work finished");
                    return Ok(());
                }
                tracing::debug!(inflight = inflight.len(), "draining — waiting for inflight");
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }

            let inflight = self.inflight.read().await.clone();
            let capacity = (self.max_inflight as usize).saturating_sub(inflight.len());
            let at_capacity = capacity == 0;

            // Control-slot polling (issue #176): at capacity we still poll
            // so the server can deliver cancels for in-flight executions
            // via `PollResponse.cancel`. The server's poll handler returns
            // immediately when `inflight.len() == max_inflight` (no
            // long-poll), so `capacity_backoff` is what paces the loop and
            // prevents a stampede. Work is never dequeued in this state
            // because the server sees zero capacity from the request.
            let poll_req = PollRequest {
                runner_id: self.runner_id.clone(),
                capabilities: self.capabilities.clone(),
                max_inflight: self.max_inflight,
                inflight,
                instance_id: Some(self.instance_id.clone()),
                tags: self.tags.clone(),
            };

            let poll_result = self.client.poll(&poll_req).await;
            let action = update_conflict_streak(
                &poll_result,
                &mut consecutive_conflicts,
                self.max_consecutive_poll_conflicts,
            );
            match poll_result {
                Ok(resp) => {
                    // Note: `resp.cancel` is not acted upon by the Rust SDK
                    // today — handler abort on server-requested cancel is
                    // tracked separately (conformance case 04 documents the
                    // gap). The poll itself still happens at capacity to
                    // keep the wire-protocol behaviour identical to the
                    // other SDKs and to make adding cancel handling later
                    // a localised change.
                    if at_capacity {
                        // Server returned immediately (capacity=0 branch).
                        // Pace the loop to avoid hammering the server.
                        tokio::time::sleep(self.capacity_backoff).await;
                        continue;
                    }
                    for assignment in resp.work {
                        let exec_id = assignment.execution_id.clone();
                        let job_key = assignment.job_key.clone();
                        self.inflight.write().await.push(exec_id.clone());

                        let client = Arc::clone(&self.client);
                        // Shared between the ExecutionContext (where the handler
                        // may lazy-init a streaming log writer) and the post-
                        // handler drain step. See `crate::log_writer` and #115.
                        let log_writer_slot = Arc::new(OnceLock::new());
                        let ctx = ExecutionContext {
                            client: Arc::clone(&client),
                            log_writer_slot: Arc::clone(&log_writer_slot),
                            execution_id: assignment.execution_id,
                            job_key: assignment.job_key,
                            attempt: assignment.attempt,
                            metadata: assignment.metadata,
                            timeout: assignment.timeout,
                            runner_id: self.runner_id.clone(),
                            runner_tags: self.tags.clone(),
                        };
                        let handlers = Arc::clone(&self.handlers);
                        let runner_id = self.runner_id.clone();
                        let inflight = Arc::clone(&self.inflight);

                        tokio::spawn(async move {
                            let attempt = ctx.attempt;

                            // Find handler
                            let handler = {
                                let reg = handlers.read().await;
                                reg.get(&job_key).cloned()
                            };

                            let (status, error, duration_ms) = if let Some(handler) = handler {
                                // Spawn lease renewal
                                let renew_client = Arc::clone(&client);
                                let renew_runner_id = runner_id.clone();
                                let renew_exec_id = exec_id.clone();
                                let renew_handle = tokio::spawn(async move {
                                    loop {
                                        tokio::time::sleep(Duration::from_secs(15)).await;
                                        let _ = renew_client
                                            .renew(&RenewRequest {
                                                runner_id: renew_runner_id.clone(),
                                                execution_id: renew_exec_id.clone(),
                                            })
                                            .await;
                                    }
                                });

                                let start = std::time::Instant::now();
                                let result = handler(ctx).await;
                                renew_handle.abort();

                                let duration_ms = start.elapsed().as_millis() as i64;
                                match result {
                                    Ok(()) => ("success".to_string(), None, duration_ms),
                                    Err(e) => {
                                        ("failure".to_string(), Some(e.to_string()), duration_ms)
                                    }
                                }
                            } else {
                                (
                                    "failure".to_string(),
                                    Some(format!("no handler registered for {job_key}")),
                                    0,
                                )
                            };

                            // Drain any streaming log writer the handler used
                            // before we ack — guarantees logs are server-side
                            // by the time the execution is marked complete.
                            // Capped at 5 s so an unreachable server doesn't
                            // wedge the dispatch loop (#115).
                            if let Some(inner) = log_writer_slot.get() {
                                inner.shutdown_and_drain().await;
                            }

                            // Ack
                            let ack = AckRequest {
                                runner_id,
                                execution_id: exec_id.clone(),
                                status,
                                error,
                                duration_ms: Some(duration_ms),
                                attempt,
                            };
                            if let Err(e) = client.ack(&ack).await {
                                tracing::error!(execution_id = %exec_id, error = %e, "failed to ack");
                            }

                            // Remove from inflight
                            inflight.write().await.retain(|id| id != &exec_id);
                        });
                    }
                }
                Err(e) if action == PollLoopAction::BailOut => {
                    tracing::error!(
                        runner_id = %self.runner_id,
                        instance_id = %self.instance_id,
                        consecutive = consecutive_conflicts,
                        max = self.max_consecutive_poll_conflicts,
                        "fatal: server returned 409 Conflict on poll repeatedly — \
                         another runner is registered with this runner_id. \
                         Stop the duplicate process or rotate runner_id."
                    );
                    return Err(e);
                }
                Err(crate::client::ClientError::PollInstanceConflict { .. }) => {
                    tracing::warn!(
                        runner_id = %self.runner_id,
                        consecutive = consecutive_conflicts,
                        max = self.max_consecutive_poll_conflicts,
                        delay_ms = self.poll_retry_delay.as_millis() as u64,
                        "poll returned 409 Conflict — another runner instance may be active; \
                         will retry"
                    );
                    tokio::time::sleep(self.poll_retry_delay).await;
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        delay_ms = self.poll_retry_delay.as_millis() as u64,
                        "poll failed — retrying"
                    );
                    tokio::time::sleep(self.poll_retry_delay).await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{ClientError, PollResponse};

    fn ok() -> Result<PollResponse, ClientError> {
        Ok(PollResponse {
            work: vec![],
            cancel: vec![],
        })
    }
    fn conflict() -> Result<PollResponse, ClientError> {
        Err(ClientError::PollInstanceConflict {
            body: "runner instance conflict".into(),
        })
    }
    fn server_error() -> Result<PollResponse, ClientError> {
        Err(ClientError::Server {
            status: 503,
            body: "service unavailable".into(),
        })
    }

    #[test]
    fn successful_poll_resets_streak() {
        let mut streak = 2;
        let action = update_conflict_streak(&ok(), &mut streak, 3);
        assert_eq!(action, PollLoopAction::Continue);
        assert_eq!(streak, 0);
    }

    #[test]
    fn non_conflict_error_resets_streak() {
        // A 5xx in the middle of a 409 streak must NOT extend the streak —
        // it has nothing to do with instance ownership.
        let mut streak = 2;
        let action = update_conflict_streak(&server_error(), &mut streak, 3);
        assert_eq!(action, PollLoopAction::Continue);
        assert_eq!(streak, 0);
    }

    #[test]
    fn conflict_increments_and_bails_at_threshold() {
        let mut streak = 0;
        for expected_streak in 1..3 {
            let action = update_conflict_streak(&conflict(), &mut streak, 3);
            assert_eq!(action, PollLoopAction::Continue);
            assert_eq!(streak, expected_streak);
        }
        // 3rd conflict → bail
        let action = update_conflict_streak(&conflict(), &mut streak, 3);
        assert_eq!(action, PollLoopAction::BailOut);
        assert_eq!(streak, 3);
    }

    #[test]
    fn conflict_then_success_then_conflicts_bails_correctly() {
        // Real-world flow: brief conflict, recovery, then a *new* conflict
        // streak. The recovery must reset the counter so the second
        // streak gets its full N attempts before bailing.
        let mut streak = 0;
        assert_eq!(
            update_conflict_streak(&conflict(), &mut streak, 3),
            PollLoopAction::Continue
        );
        assert_eq!(
            update_conflict_streak(&conflict(), &mut streak, 3),
            PollLoopAction::Continue
        );
        // Recovery resets.
        assert_eq!(
            update_conflict_streak(&ok(), &mut streak, 3),
            PollLoopAction::Continue
        );
        assert_eq!(streak, 0);
        // Fresh streak starts at 1.
        assert_eq!(
            update_conflict_streak(&conflict(), &mut streak, 3),
            PollLoopAction::Continue
        );
        assert_eq!(streak, 1);
    }

    #[test]
    fn max_one_bails_on_first_conflict() {
        // Aggressive operator setting: refuse to tolerate any conflict at
        // all. First 409 → fatal.
        let mut streak = 0;
        let action = update_conflict_streak(&conflict(), &mut streak, 1);
        assert_eq!(action, PollLoopAction::BailOut);
        assert_eq!(streak, 1);
    }
}

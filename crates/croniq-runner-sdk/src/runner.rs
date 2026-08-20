//! CroniqRunner: the main orchestrator for job execution runners.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use tokio::sync::RwLock;
use tokio::task::AbortHandle;

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
/// * A 403 ([`ClientError::WorkOwnershipDenied`]) bails on the *first*
///   occurrence regardless of the threshold: the credential is bound to
///   another `runner_id` and no amount of retrying can change that
///   (issue #437). The streak counter is left untouched — it belongs to
///   the 409 story and the loop exits immediately anyway.
///
/// [`ClientError::WorkOwnershipDenied`]: crate::client::ClientError::WorkOwnershipDenied
pub(crate) fn update_conflict_streak(
    result: &Result<crate::client::PollResponse, crate::client::ClientError>,
    consecutive: &mut u32,
    max_consecutive: u32,
) -> PollLoopAction {
    // The 403 bails immediately and leaves the counter alone: it belongs to
    // the 409 story, and the loop exits regardless.
    if matches!(
        result,
        Err(crate::client::ClientError::WorkOwnershipDenied { .. })
    ) {
        return PollLoopAction::BailOut;
    }
    let counts = matches!(
        result,
        Err(crate::client::ClientError::PollInstanceConflict { .. })
    );
    update_streak(counts, consecutive, max_consecutive)
}

/// The budget rule both streak trackers apply: count the outcome the caller
/// cares about, reset on anything else, and bail once the run reaches the
/// threshold.
///
/// Shared so the two cannot drift apart. They already differ on which error
/// they count and that is the whole of the intended difference — the
/// saturating increment, the `>=` comparison and the reset-on-anything-else
/// are one rule, and the SDK conformance suite holds all six language
/// bindings to it.
fn update_streak(counts: bool, consecutive: &mut u32, max_consecutive: u32) -> PollLoopAction {
    if !counts {
        *consecutive = 0;
        return PollLoopAction::Continue;
    }
    *consecutive = consecutive.saturating_add(1);
    if *consecutive >= max_consecutive {
        PollLoopAction::BailOut
    } else {
        PollLoopAction::Continue
    }
}

/// Update the consecutive-401 counter and decide whether to keep polling.
///
/// A `401` says the credential was rejected. The SDK reads its API key once,
/// at construction, and never re-reads it, so every later request presents
/// the same rejected key — retrying cannot clear it (issue #473). Before this
/// existed a `401` landed in the generic transient bucket and the runner
/// retried on the poll interval forever: the process stayed up, looked
/// healthy, did nothing, and never exited non-zero, so no restart policy
/// fired — and restarting is precisely what would have fixed it.
///
/// Unlike a `403` it is not fatal on the first occurrence. Key rotation hands
/// over by installing the new key and giving the old one an expiry (issue
/// #471); dropping dead on a single `401` would turn a narrow race around
/// that handover into an outage. So:
///
/// * Successful polls reset the counter — the credential works.
/// * Other errors reset it too: a 5xx or a timeout says nothing about whether
///   the key is valid, and counting them would bail on a merely unwell server.
/// * `401`s increment and trip [`PollLoopAction::BailOut`] at the threshold.
pub(crate) fn update_auth_streak(
    result: &Result<crate::client::PollResponse, crate::client::ClientError>,
    consecutive: &mut u32,
    max_consecutive: u32,
) -> PollLoopAction {
    let counts = matches!(result, Err(crate::client::ClientError::Unauthorized { .. }));
    update_streak(counts, consecutive, max_consecutive)
}

/// Map the result of awaiting a handler task to an ack `(status, error,
/// duration_ms)` triple. A handler that ran is `success`/`failure` as before;
/// a task aborted by a server cancel (or one that panicked) is reported as
/// `failure` so the server records a terminal outcome (issue #176).
fn classify_join(
    join: Result<Result<(), HandlerError>, tokio::task::JoinError>,
    duration_ms: i64,
) -> (String, Option<String>, i64) {
    match join {
        Ok(Ok(())) => ("success".to_string(), None, duration_ms),
        Ok(Err(e)) => ("failure".to_string(), Some(e.to_string()), duration_ms),
        Err(je) if je.is_cancelled() => (
            "failure".to_string(),
            Some("execution cancelled by server".to_string()),
            duration_ms,
        ),
        Err(je) => (
            "failure".to_string(),
            Some(format!("handler task failed: {je}")),
            duration_ms,
        ),
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
    max_consecutive_auth_failures: u32,
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
    /// capacity. Default: **500 ms**, matching the .NET / Go / Python /
    /// TypeScript / Java SDK defaults.
    ///
    /// Even at capacity the SDK keeps polling so the server can deliver
    /// admin-issued cancels via `PollResponse.cancel` (issue #176). The
    /// at-capacity branch returns immediately on the server side
    /// (capacity=0), so this is what paces the loop and prevents a
    /// stampede.
    ///
    /// On a `PollResponse.cancel` the SDK aborts the matching in-flight
    /// handler future and acks the execution as `failure`, matching the
    /// other SDKs and conformance cases 04 / 04a (issue #176).
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

    /// Maximum number of consecutive `401 Unauthorized` responses before the
    /// runner gives up and exits with a fatal [`ClientError::Unauthorized`].
    /// Default: 3.
    ///
    /// The API key is read once, at construction, and never re-read, so a
    /// rejected credential cannot fix itself — retrying only produces an
    /// idle-looking process that never exits and never gets restarted. The
    /// budget exists so a narrow race around a key rotation handover (issue
    /// #471) does not kill an otherwise healthy runner. The counter resets on
    /// any successful poll, and on any other error: a 5xx says nothing about
    /// whether the credential is valid.
    pub fn max_consecutive_auth_failures(mut self, n: u32) -> Self {
        self.max_consecutive_auth_failures = n;
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
            max_consecutive_auth_failures: self.max_consecutive_auth_failures,
            handlers: Arc::new(RwLock::new(HandlerRegistry::new())),
            schedules: Arc::new(RwLock::new(Vec::new())),
            inflight: Arc::new(RwLock::new(Vec::new())),
            aborts: Arc::new(RwLock::new(HashMap::new())),
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
    max_consecutive_auth_failures: u32,
    handlers: Arc<RwLock<HandlerRegistry>>,
    schedules: Arc<RwLock<Vec<JobSchedule>>>,
    inflight: Arc<RwLock<Vec<String>>>,
    /// Abort handles for the in-flight handler futures, keyed by execution
    /// id. A server-issued cancel (`PollResponse.cancel`) looks the id up
    /// here and aborts just that handler; the dispatch task then acks it.
    aborts: Arc<RwLock<HashMap<String, AbortHandle>>>,
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
            capacity_backoff: Duration::from_millis(500),
            max_consecutive_poll_conflicts: 3,
            max_consecutive_auth_failures: 3,
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

    /// Abort the in-flight handler futures for the given execution ids.
    /// Unknown ids (already finished, or never claimed here) are ignored, so
    /// this is safe to call with a stale cancel list. The aborted handler's
    /// dispatch task acks the execution as a failure and frees its slot.
    async fn abort_cancelled(aborts: &RwLock<HashMap<String, AbortHandle>>, cancel: &[String]) {
        if cancel.is_empty() {
            return;
        }
        let guard = aborts.read().await;
        for exec_id in cancel {
            if let Some(handle) = guard.get(exec_id) {
                tracing::info!(execution_id = %exec_id, "server requested cancel — aborting handler");
                handle.abort();
            }
        }
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
        // Same shape for 401s, tracked separately: the two failures are
        // independent, and a run of conflicts must not spend the auth budget.
        let mut consecutive_auth_failures: u32 = 0;

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
            let auth_action = update_auth_streak(
                &poll_result,
                &mut consecutive_auth_failures,
                self.max_consecutive_auth_failures,
            );
            match poll_result {
                Ok(resp) => {
                    // Honour server-issued cancels (issue #176): abort the
                    // matching in-flight handler futures. Each dispatch task
                    // then acks its execution as a failure and releases the
                    // inflight slot. Done before the at-capacity early-return
                    // so a max_inflight=1 runner still acts on cancels.
                    Self::abort_cancelled(&self.aborts, &resp.cancel).await;

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
                        // Parse the logical fire time; a missing or malformed
                        // value yields None rather than falling back to
                        // fire_at — see ExecutionContext::scheduled_for.
                        let scheduled_for = assignment.scheduled_for.as_deref().and_then(|s| {
                            match chrono::DateTime::parse_from_rfc3339(s) {
                                Ok(dt) => Some(dt.with_timezone(&chrono::Utc)),
                                Err(e) => {
                                    tracing::warn!(value = %s, error = %e, "failed to parse scheduled_for");
                                    None
                                }
                            }
                        });
                        let ctx = ExecutionContext {
                            client: Arc::clone(&client),
                            log_writer_slot: Arc::clone(&log_writer_slot),
                            execution_id: assignment.execution_id,
                            job_key: assignment.job_key,
                            scheduled_for,
                            attempt: assignment.attempt,
                            metadata: assignment.metadata,
                            timeout: assignment.timeout,
                            runner_id: self.runner_id.clone(),
                            runner_tags: self.tags.clone(),
                        };
                        let handlers = Arc::clone(&self.handlers);
                        let runner_id = self.runner_id.clone();
                        let inflight = Arc::clone(&self.inflight);
                        let aborts = Arc::clone(&self.aborts);

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
                                        // The result used to be discarded outright
                                        // (`let _ = …`), which hid a misconfigured
                                        // credential completely — see #437.
                                        let renewed = renew_client
                                            .renew(&RenewRequest {
                                                runner_id: renew_runner_id.clone(),
                                                execution_id: renew_exec_id.clone(),
                                            })
                                            .await;
                                        match renewed {
                                            Ok(()) => {}
                                            Err(
                                                crate::client::ClientError::WorkOwnershipDenied {
                                                    ..
                                                },
                                            ) => {
                                                tracing::error!(
                                                    runner_id = %renew_runner_id,
                                                    execution_id = %renew_exec_id,
                                                    "lease renew refused with 403 Forbidden — this \
                                                     runner's credential does not own runner_id. \
                                                     The lease will expire and the execution be \
                                                     reclaimed. Give the runner its own runner_id, \
                                                     or release the existing binding with \
                                                     DELETE /v1/runners/{{id}}."
                                                );
                                            }
                                            // Since #447 renew is a real per-execution
                                            // lease: 404 (no longer leased here) and 409
                                            // (already terminal) are the normal outcome
                                            // of a renew racing our own completion.
                                            Err(crate::client::ClientError::Server {
                                                status: status @ (404 | 409),
                                                ..
                                            }) => {
                                                tracing::debug!(
                                                    execution_id = %renew_exec_id,
                                                    status,
                                                    "lease renew raced execution completion"
                                                );
                                            }
                                            Err(e) => {
                                                tracing::warn!(
                                                    execution_id = %renew_exec_id,
                                                    error = %e,
                                                    "lease renew failed — will retry"
                                                );
                                            }
                                        }
                                    }
                                });

                                let start = std::time::Instant::now();

                                // Run the handler on its own task so a server
                                // cancel can abort *only* the handler future,
                                // leaving this task free to ack + release the
                                // inflight slot (issue #176). The abort handle
                                // is registered under the execution id so the
                                // poll loop can reach it.
                                let handler_task = tokio::spawn(async move { handler(ctx).await });
                                aborts
                                    .write()
                                    .await
                                    .insert(exec_id.clone(), handler_task.abort_handle());

                                let join = handler_task.await;
                                renew_handle.abort();
                                aborts.write().await.remove(&exec_id);

                                let duration_ms = start.elapsed().as_millis() as i64;
                                classify_join(join, duration_ms)
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
                            let ack_runner_id = runner_id.clone();
                            let ack = AckRequest {
                                runner_id,
                                execution_id: exec_id.clone(),
                                status,
                                error,
                                duration_ms: Some(duration_ms),
                                attempt,
                            };
                            match client.ack(&ack).await {
                                Ok(()) => {}
                                Err(e @ crate::client::ClientError::WorkOwnershipDenied { .. }) => {
                                    tracing::error!(
                                        execution_id = %exec_id,
                                        runner_id = %ack_runner_id,
                                        error = %e,
                                        "ack refused with 403 Forbidden — this runner's credential \
                                         does not own runner_id, so the execution stays claimed \
                                         until its lease expires. Give the runner its own \
                                         runner_id, or release the existing binding with \
                                         DELETE /v1/runners/{{id}}."
                                    );
                                }
                                Err(e) => {
                                    tracing::error!(execution_id = %exec_id, error = %e, "failed to ack");
                                }
                            }

                            // Remove from inflight
                            inflight.write().await.retain(|id| id != &exec_id);
                        });
                    }
                }
                Err(e @ crate::client::ClientError::WorkOwnershipDenied { .. }) => {
                    // Threshold of 1: a 403 is permanent, so retrying only
                    // hides an operator misconfiguration (issue #437).
                    tracing::error!(
                        runner_id = %self.runner_id,
                        instance_id = %self.instance_id,
                        "fatal: server returned 403 Forbidden on poll — this runner's credential \
                         does not own runner_id. Give the runner its own runner_id, or release \
                         the existing binding with DELETE /v1/runners/{{id}}."
                    );
                    return Err(e);
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
                Err(e) if auth_action == PollLoopAction::BailOut => {
                    tracing::error!(
                        runner_id = %self.runner_id,
                        consecutive = consecutive_auth_failures,
                        max = self.max_consecutive_auth_failures,
                        "fatal: server returned 401 Unauthorized on poll repeatedly — the API \
                         key was rejected. It may have been revoked, or its rotation grace \
                         window may have elapsed. Restart the runner with the current key."
                    );
                    return Err(e);
                }
                Err(crate::client::ClientError::Unauthorized { .. }) => {
                    tracing::warn!(
                        runner_id = %self.runner_id,
                        consecutive = consecutive_auth_failures,
                        max = self.max_consecutive_auth_failures,
                        delay_ms = self.poll_retry_delay.as_millis() as u64,
                        "poll returned 401 Unauthorized — the API key was rejected; will retry"
                    );
                    tokio::time::sleep(self.poll_retry_delay).await;
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
    fn ownership_denied() -> Result<PollResponse, ClientError> {
        Err(ClientError::WorkOwnershipDenied {
            endpoint: "/v1/work/poll",
            body: "runner_id is owned by another credential".into(),
        })
    }
    fn unauthorized() -> Result<PollResponse, ClientError> {
        Err(ClientError::Unauthorized {
            endpoint: "/v1/work/poll",
            body: "unauthorized".into(),
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

    #[test]
    fn ownership_denied_bails_on_the_first_occurrence() {
        // Threshold of 1 regardless of max_consecutive_poll_conflicts: a
        // 403 is permanent, so a second attempt can only fail the same way.
        let mut streak = 0;
        assert_eq!(
            update_conflict_streak(&ownership_denied(), &mut streak, 100),
            PollLoopAction::BailOut
        );
    }

    #[test]
    fn ownership_denied_does_not_disturb_the_conflict_streak() {
        // The 409 counter tells operators how long a duplicate deployment
        // has been fenced out; a 403 is a different failure and must not
        // inflate it.
        let mut streak = 2;
        assert_eq!(
            update_conflict_streak(&ownership_denied(), &mut streak, 3),
            PollLoopAction::BailOut
        );
        assert_eq!(streak, 2);
    }

    #[tokio::test]
    async fn abort_cancelled_aborts_only_the_matching_handler() {
        let aborts: RwLock<HashMap<String, AbortHandle>> = RwLock::new(HashMap::new());

        let task = tokio::spawn(async {
            tokio::time::sleep(Duration::from_secs(3600)).await;
        });
        aborts
            .write()
            .await
            .insert("exec-1".to_string(), task.abort_handle());

        // A cancel for a different id must leave the handler running.
        CroniqRunner::abort_cancelled(&aborts, &["other".to_string()]).await;
        assert!(!task.is_finished(), "non-matching cancel must not abort");

        // A cancel for the matching id aborts the handler future.
        CroniqRunner::abort_cancelled(&aborts, &["exec-1".to_string()]).await;
        assert!(task.await.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn classify_join_reports_server_cancel_as_failure() {
        // An aborted handler task surfaces as a cancelled JoinError, which
        // must be acked as "failure" (conformance cases 04 / 04a).
        let task: tokio::task::JoinHandle<Result<(), HandlerError>> = tokio::spawn(async {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            Ok(())
        });
        task.abort();
        let (status, error, duration_ms) = classify_join(task.await, 42);
        assert_eq!(status, "failure");
        assert!(error.unwrap().contains("cancelled"));
        assert_eq!(duration_ms, 42);
    }

    #[test]
    fn classify_join_reports_success_and_handler_error() {
        let (status, error, duration_ms) = classify_join(Ok(Ok(())), 7);
        assert_eq!(status, "success");
        assert!(error.is_none());
        assert_eq!(duration_ms, 7);

        let (status, error, _) = classify_join(Ok(Err(HandlerError::msg("boom"))), 7);
        assert_eq!(status, "failure");
        assert_eq!(error.as_deref(), Some("boom"));
    }

    // ─── 401 budget (issue #473) ─────────────────────────────────────────

    #[test]
    fn a_single_unauthorized_is_survivable() {
        // Key rotation hands over through an expiry window; bailing on the
        // first 401 would turn a narrow race around that into an outage.
        let mut streak = 0;
        assert_eq!(
            update_auth_streak(&unauthorized(), &mut streak, 3),
            PollLoopAction::Continue
        );
        assert_eq!(streak, 1);
    }

    #[test]
    fn consecutive_unauthorized_bails_at_the_threshold() {
        let mut streak = 0;
        assert_eq!(
            update_auth_streak(&unauthorized(), &mut streak, 2),
            PollLoopAction::Continue
        );
        assert_eq!(
            update_auth_streak(&unauthorized(), &mut streak, 2),
            PollLoopAction::BailOut,
            "a streak of 401s is a credential that is gone, not a blip"
        );
        assert_eq!(streak, 2);
    }

    #[test]
    fn a_successful_poll_clears_the_auth_streak() {
        let mut streak = 2;
        assert_eq!(
            update_auth_streak(&ok(), &mut streak, 3),
            PollLoopAction::Continue
        );
        assert_eq!(streak, 0);
    }

    #[test]
    fn an_unrelated_error_clears_the_auth_streak() {
        // A 503 says nothing about whether the credential is valid. Counting
        // it would make an unwell server look like a revoked key.
        let mut streak = 2;
        assert_eq!(
            update_auth_streak(&server_error(), &mut streak, 3),
            PollLoopAction::Continue
        );
        assert_eq!(streak, 0);
    }

    #[test]
    fn the_two_budgets_do_not_share_a_counter() {
        // A run of 409s must not spend the auth budget, or a duplicate
        // deployment would be reported as an authentication failure.
        let mut auth = 0;
        let mut conflicts = 0;
        for _ in 0..5 {
            update_auth_streak(&conflict(), &mut auth, 2);
            update_conflict_streak(&unauthorized(), &mut conflicts, 2);
        }
        assert_eq!(auth, 0, "conflicts must not count against the auth budget");
        assert_eq!(
            conflicts, 0,
            "401s must not count against the conflict budget"
        );
    }
}

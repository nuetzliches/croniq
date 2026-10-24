//! CroniqRunner: the main orchestrator for job execution runners.

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use tokio::sync::{Notify, RwLock};
use tokio::task::AbortHandle;

use crate::client::{
    AckRequest, ClientError, CroniqClient, PollRequest, RegisterJobRequest, RenewRequest,
};
use crate::handler::{ExecutionContext, HandlerError, HandlerRegistry};
use crate::identifiers::{self, RejectedField};

/// Default interval between lease renewals of an in-flight execution.
pub const DEFAULT_RENEW_INTERVAL: Duration = Duration::from_secs(15);

/// Default bound on how long [`CroniqRunner::drain`] waits for in-flight
/// handlers before aborting them. Matches the Go SDK's `DefaultDrainTimeout`.
pub const DEFAULT_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);

/// Delays before the second and third ack attempt (review 2026-10-24, K-1).
///
/// Three attempts in total. Short on purpose: the handler has already run, the
/// slot stays taken while the ack is retried, and the server's stale-claim
/// reaper requeues an unreported claim after 60 s
/// (`UNREPORTED_CLAIM_GRACE_SECS`). The point is to ride out a blip, not an
/// outage.
const ACK_RETRY_DELAYS: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(2)];

/// Ceiling on the exponential backoff after transient poll failures. Never
/// below the configured `poll_retry_delay`.
const MAX_POLL_BACKOFF: Duration = Duration::from_secs(60);

/// Why a handler future was aborted, recorded before the abort so the ack can
/// say which of the three it was. One per in-flight execution.
const ABORT_NONE: u8 = 0;
const ABORT_SERVER_CANCEL: u8 = 1;
const ABORT_LEASE_LOST: u8 = 2;
const ABORT_DRAIN: u8 = 3;

/// The handle that aborts one in-flight handler, plus the reason slot the
/// aborting party fills in first. The first reason wins: a lease lost during
/// a drain stays a lost lease.
#[derive(Clone)]
struct Abort {
    handle: AbortHandle,
    reason: Arc<AtomicU8>,
}

impl Abort {
    fn abort(&self, reason: u8) {
        let _ =
            self.reason
                .compare_exchange(ABORT_NONE, reason, Ordering::AcqRel, Ordering::Acquire);
        self.handle.abort();
    }
}

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
pub(crate) fn update_conflict_streak(
    result: &Result<crate::client::PollResponse, ClientError>,
    consecutive: &mut u32,
    max_consecutive: u32,
) -> PollLoopAction {
    // The 403 bails immediately and leaves the counter alone: it belongs to
    // the 409 story, and the loop exits regardless.
    if matches!(result, Err(ClientError::WorkOwnershipDenied { .. })) {
        return PollLoopAction::BailOut;
    }
    let counts = matches!(result, Err(ClientError::PollInstanceConflict { .. }));
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
    result: &Result<crate::client::PollResponse, ClientError>,
    consecutive: &mut u32,
    max_consecutive: u32,
) -> PollLoopAction {
    let counts = matches!(result, Err(ClientError::Unauthorized { .. }));
    update_streak(counts, consecutive, max_consecutive)
}

/// Scale `d` by a random factor in 0.8–1.2 (review 2026-10-24, R-8 / K-9).
///
/// After a server restart every runner of a fleet fails its poll in the same
/// second; with a fixed delay they all come back in the same second too, every
/// time. ±20 % spreads them out and stays inside what conformance case 12
/// pins. The randomness comes from a v4 UUID, already a dependency.
pub(crate) fn jitter(d: Duration) -> Duration {
    let r = (uuid::Uuid::new_v4().as_u128() % 10_001) as f64 / 10_000.0;
    d.mul_f64(0.8 + 0.4 * r)
}

/// The wait after the `failures`-th transient poll failure in a row: the
/// configured delay doubled per further failure, capped at
/// [`MAX_POLL_BACKOFF`] (or the configured delay, if larger), jittered.
///
/// A fixed 5 s retry kept a whole fleet hammering a server that was down for
/// minutes (review 2026-10-24, R-8).
pub(crate) fn poll_backoff(base: Duration, failures: u32) -> Duration {
    let doublings = failures.saturating_sub(1).min(16);
    let raw = base.saturating_mul(1u32 << doublings);
    jitter(raw.min(MAX_POLL_BACKOFF.max(base)))
}

/// Map the result of awaiting a handler task to an ack `(status, error,
/// duration_ms)` triple. A handler that ran is `success`/`failure` as before;
/// a task aborted by a server cancel, a lost lease or a drain timeout (or one
/// that panicked) is reported as `failure` so the server records a terminal
/// outcome (issue #176), with the reason in the error.
fn classify_join(
    join: Result<Result<(), HandlerError>, tokio::task::JoinError>,
    abort_reason: u8,
    duration_ms: i64,
) -> (String, Option<String>, i64) {
    match join {
        Ok(Ok(())) => ("success".to_string(), None, duration_ms),
        Ok(Err(e)) => ("failure".to_string(), Some(e.to_string()), duration_ms),
        Err(je) if je.is_cancelled() => {
            let why = match abort_reason {
                ABORT_LEASE_LOST => {
                    "lease lost — the server no longer holds this execution for this runner, \
                     so the handler was aborted"
                }
                ABORT_DRAIN => {
                    "runner shutting down — the handler was still running when the drain \
                     timeout elapsed"
                }
                _ => "execution cancelled by server",
            };
            ("failure".to_string(), Some(why.to_string()), duration_ms)
        }
        Err(je) => (
            "failure".to_string(),
            Some(format!("handler task failed: {je}")),
            duration_ms,
        ),
    }
}

/// Send `ack`, retrying a transport failure or a 5xx after each of `delays`
/// (review 2026-10-24, K-1).
///
/// Every SDK used to log a failed ack and forget it: the execution left the
/// in-flight set, the next poll reported it missing, and the stale-claim
/// reaper requeued a job that had already succeeded — a full second run for a
/// millisecond network blip. The ack is idempotent server-side (CAS-fenced),
/// so repeating it is safe; a 4xx is never repeated, because the server has
/// answered.
pub(crate) async fn ack_with_retry(
    client: &CroniqClient,
    ack: &AckRequest,
    delays: &[Duration],
) -> Result<(), ClientError> {
    let mut retries = delays.iter();
    loop {
        match client.ack(ack).await {
            Ok(()) => return Ok(()),
            Err(e) if e.is_retryable_ack_failure() => match retries.next() {
                Some(delay) => {
                    tracing::warn!(
                        execution_id = %ack.execution_id,
                        error = %crate::error_chain::chain(&e),
                        retry_in_ms = delay.as_millis() as u64,
                        "ack failed — retrying"
                    );
                    tokio::time::sleep(*delay).await;
                }
                None => return Err(e),
            },
            Err(e) => return Err(e),
        }
    }
}

/// Resolve once the process is asked to stop: SIGTERM or SIGINT on unix,
/// Ctrl-C elsewhere. For [`CroniqRunner::run_until`].
///
/// `docker stop`, systemd and Kubernetes all send SIGTERM; a runner that only
/// listened for Ctrl-C died mid-handler on every deploy (review 2026-10-24,
/// R-4).
pub async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match (
            signal(SignalKind::terminate()),
            signal(SignalKind::interrupt()),
        ) {
            (Ok(mut term), Ok(mut int)) => {
                tokio::select! {
                    _ = term.recv() => {}
                    _ = int.recv() => {}
                }
                return;
            }
            _ => tracing::warn!("could not install SIGTERM/SIGINT handlers — Ctrl-C only"),
        }
    }
    if let Err(e) = tokio::signal::ctrl_c().await {
        tracing::warn!(error = %e, "could not listen for Ctrl-C — shutdown signal disabled");
        std::future::pending::<()>().await;
    }
}

/// Builder for constructing a CroniqRunner.
pub struct RunnerBuilder {
    server_url: String,
    runner_id: String,
    api_key: Option<String>,
    bearer_token: Option<String>,
    allow_insecure_http: bool,
    capabilities: Vec<String>,
    max_inflight: u32,
    tags: Vec<String>,
    poll_retry_delay: Duration,
    capacity_backoff: Duration,
    max_consecutive_poll_conflicts: u32,
    max_consecutive_auth_failures: u32,
    request_timeout: Duration,
    renew_interval: Duration,
    drain_timeout: Duration,
}

impl RunnerBuilder {
    pub fn api_key(mut self, key: &str) -> Self {
        self.api_key = Some(key.to_string());
        self
    }

    /// Authenticate with `Authorization: Bearer <token>` instead of an API
    /// key — a user's JWT, for instance. Ignored when an
    /// [`api_key`](Self::api_key) is also set, as in the Go SDK and
    /// [`TriggerClient`](crate::TriggerClient).
    pub fn bearer_token(mut self, token: &str) -> Self {
        self.bearer_token = Some(token.to_string());
        self
    }

    /// Accept a cleartext `http://` server URL on a non-loopback host.
    ///
    /// Without it such a URL is refused and [`CroniqRunner::start`] returns
    /// [`ClientError::Config`] at once: the credential would travel in the
    /// clear on every request (issue #440; review 2026-10-24, K-3). `https://`
    /// and `http://` to `localhost`, `127.0.0.0/8` or `::1` need no opt-in.
    pub fn allow_insecure_http(mut self) -> Self {
        self.allow_insecure_http = true;
        self
    }

    pub fn capabilities(mut self, caps: Vec<String>) -> Self {
        self.capabilities = caps;
        self
    }

    /// Maximum number of executions run at once. Default: 5. Must be at least
    /// 1: a runner with no slots polls forever and never takes work, so `0`
    /// makes [`CroniqRunner::start`] return [`ClientError::Config`].
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

    /// Base delay after a failed poll. Default: 5 seconds.
    ///
    /// After a 401 or a 409 the runner waits this long (±20 %) before the next
    /// poll. After other failures — a 5xx, a timeout, a refused connection —
    /// the wait doubles with each failure in a row, up to 60 s (or this delay,
    /// if larger), also ±20 %, and drops back once a poll succeeds.
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
    /// stampede. It is an upper bound: the wait ends as soon as an in-flight
    /// handler finishes, so a freed slot is refilled at once (issue #845).
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

    /// Bound on each ack, lease renewal, log-event push and job registration.
    /// Default: 30 s ([`crate::client::DEFAULT_REQUEST_TIMEOUT`]). The long
    /// poll keeps its own 35 s bound.
    ///
    /// An unbounded ack that hung on a half-open connection kept the
    /// execution in the runner's in-flight set forever, so every poll
    /// renewed its lease and the claim was never reaped (issue #792).
    pub fn request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Interval between lease renewals of each in-flight execution. Default:
    /// [`DEFAULT_RENEW_INTERVAL`] (15 s), well inside the server's stale-claim
    /// grace. Configurable so conformance case 05 can be driven.
    pub fn renew_interval(mut self, interval: Duration) -> Self {
        self.renew_interval = interval;
        self
    }

    /// How long [`CroniqRunner::drain`] lets in-flight handlers run before
    /// aborting them. Default: [`DEFAULT_DRAIN_TIMEOUT`] (30 s).
    ///
    /// Aborted handlers are acked as `failure` ("runner shutting down"), and
    /// the runner then waits up to `request_timeout` for those acks. Keep the
    /// sum below the orchestrator's stop grace (Docker: 10 s by default), or
    /// the process is killed before the acks leave.
    pub fn drain_timeout(mut self, timeout: Duration) -> Self {
        self.drain_timeout = timeout;
        self
    }

    pub fn build(self) -> CroniqRunner {
        let mut client =
            CroniqClient::new(&self.server_url).with_request_timeout(self.request_timeout);
        if let Some(key) = &self.api_key {
            client = client.with_api_key(key);
        } else if let Some(token) = &self.bearer_token {
            client = client.with_bearer(token);
        }

        // Refused configurations surface from `start()` rather than from here:
        // `build()` returning a `Result` would break every caller for a check
        // they can only act on at startup anyway.
        let config_error =
            crate::security::validate_base_url(&self.server_url, self.allow_insecure_http)
                .err()
                .or_else(|| {
                    (self.max_inflight == 0).then(|| {
                        "croniq: max_inflight must be at least 1 — a runner with no slots polls \
                         forever and never takes work"
                            .to_string()
                    })
                });

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
            request_timeout: self.request_timeout,
            renew_interval: self.renew_interval,
            drain_timeout: self.drain_timeout,
            config_error,
            handlers: Arc::new(RwLock::new(HandlerRegistry::new())),
            schedules: Arc::new(RwLock::new(Vec::new())),
            inflight: Arc::new(RwLock::new(Vec::new())),
            aborts: Arc::new(RwLock::new(HashMap::new())),
            slot_freed: Arc::new(Notify::new()),
            draining: Arc::new(AtomicBool::new(false)),
            drain_requested: Arc::new(Notify::new()),
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
    request_timeout: Duration,
    renew_interval: Duration,
    drain_timeout: Duration,
    /// Set by [`RunnerBuilder::build`] for a configuration the runner refuses
    /// to start with; returned by [`CroniqRunner::start`].
    config_error: Option<String>,
    handlers: Arc<RwLock<HandlerRegistry>>,
    schedules: Arc<RwLock<Vec<JobSchedule>>>,
    inflight: Arc<RwLock<Vec<String>>>,
    /// Abort handles for the in-flight handler futures, keyed by execution
    /// id. A server-issued cancel (`PollResponse.cancel`), a lost lease or a
    /// drain timeout looks the id up here and aborts just that handler; the
    /// dispatch task then acks it.
    aborts: Arc<RwLock<HashMap<String, Abort>>>,
    /// Woken by a dispatch task once it has released its in-flight slot, so
    /// the at-capacity backoff ends as soon as there is room (issue #845).
    slot_freed: Arc<Notify>,
    draining: Arc<AtomicBool>,
    /// Woken by [`CroniqRunner::drain`], so a long poll or a retry wait in
    /// progress ends at once instead of holding the shutdown for up to 35 s.
    drain_requested: Arc<Notify>,
}

impl CroniqRunner {
    pub fn builder(server_url: &str, runner_id: &str) -> RunnerBuilder {
        RunnerBuilder {
            server_url: server_url.to_string(),
            runner_id: runner_id.to_string(),
            api_key: None,
            bearer_token: None,
            allow_insecure_http: false,
            capabilities: Vec::new(),
            max_inflight: 5,
            tags: Vec::new(),
            poll_retry_delay: Duration::from_secs(5),
            capacity_backoff: Duration::from_millis(500),
            max_consecutive_poll_conflicts: 3,
            max_consecutive_auth_failures: 3,
            request_timeout: crate::client::DEFAULT_REQUEST_TIMEOUT,
            renew_interval: DEFAULT_RENEW_INTERVAL,
            drain_timeout: DEFAULT_DRAIN_TIMEOUT,
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

    /// Signal graceful shutdown: stop polling, let in-flight handlers finish.
    ///
    /// Takes effect in a running [`start`](Self::start), which then returns
    /// `Ok(())` once every in-flight execution is acked. Handlers still running
    /// after `drain_timeout` are aborted and acked as `failure`. Calling this
    /// from a `select!` arm that has dropped the `start()` future does
    /// nothing — use [`run_until`](Self::run_until).
    pub fn drain(&self) {
        self.draining.store(true, Ordering::Relaxed);
        self.drain_requested.notify_waiters();
        tracing::info!(runner_id = %self.runner_id, "draining — no new work will be accepted");
    }

    /// Run until `shutdown` resolves, then drain (review 2026-10-24, R-4).
    ///
    /// ```rust,no_run
    /// # use croniq_runner_sdk::CroniqRunner;
    /// # async fn demo(runner: CroniqRunner) {
    /// runner
    ///     .run_until(croniq_runner_sdk::runner::shutdown_signal())
    ///     .await
    ///     .unwrap();
    /// # }
    /// ```
    pub async fn run_until<F>(&self, shutdown: F) -> Result<(), ClientError>
    where
        F: Future<Output = ()>,
    {
        let run = self.start();
        tokio::pin!(run);
        tokio::select! {
            result = &mut run => return result,
            () = shutdown => {}
        }
        tracing::info!(runner_id = %self.runner_id, "shutdown requested");
        self.drain();
        run.await
    }

    /// Abort the in-flight handler futures for the given execution ids.
    /// Unknown ids (already finished, or never claimed here) are ignored, so
    /// this is safe to call with a stale cancel list. The aborted handler's
    /// dispatch task acks the execution as a failure and frees its slot.
    async fn abort_cancelled(aborts: &RwLock<HashMap<String, Abort>>, cancel: &[String]) {
        if cancel.is_empty() {
            return;
        }
        let guard = aborts.read().await;
        for exec_id in cancel {
            if let Some(abort) = guard.get(exec_id) {
                tracing::info!(execution_id = %exec_id, "server requested cancel — aborting handler");
                abort.abort(ABORT_SERVER_CANCEL);
            }
        }
    }

    /// The drain phase of [`start`](Self::start): wait for in-flight work up
    /// to `drain_timeout`, abort what is left, then wait up to
    /// `request_timeout` for the failure acks of the aborted handlers.
    ///
    /// Before review 2026-10-24 (K-3) the wait had no bound, so one hung
    /// handler blocked a shutdown forever — or until the orchestrator's
    /// SIGKILL, which loses every ack.
    async fn finish_drain(&self) -> Result<(), ClientError> {
        if self.wait_for_inflight(self.drain_timeout).await {
            tracing::info!("drain complete — all inflight work finished");
            return Ok(());
        }
        let remaining = self.aborts.read().await.len();
        tracing::warn!(
            remaining,
            drain_timeout_ms = self.drain_timeout.as_millis() as u64,
            "drain timeout elapsed — aborting the remaining handlers"
        );
        for abort in self.aborts.read().await.values() {
            abort.abort(ABORT_DRAIN);
        }
        if self.wait_for_inflight(self.request_timeout).await {
            tracing::info!("drain complete — remaining handlers aborted and acked");
        } else {
            let remaining = self.inflight.read().await.len();
            tracing::warn!(
                remaining,
                "drain gave up waiting for the final acks — the stale-claim reaper will \
                 recover those executions"
            );
        }
        Ok(())
    }

    /// Wait until the in-flight set is empty, for at most `limit`. Returns
    /// whether it emptied.
    async fn wait_for_inflight(&self, limit: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + limit;
        loop {
            // Registered before the check, so a slot freed in between wakes it.
            let freed = self.slot_freed.notified();
            tokio::pin!(freed);
            freed.as_mut().enable();
            let left = self.inflight.read().await.len();
            if left == 0 {
                return true;
            }
            tracing::debug!(inflight = left, "draining — waiting for inflight");
            tokio::select! {
                () = &mut freed => {}
                () = tokio::time::sleep_until(deadline) => {
                    return self.inflight.read().await.is_empty();
                }
            }
        }
    }

    /// Start the runner: auto-register jobs, then poll loop + lease renewal.
    ///
    /// Returns `Ok(())` after a [`drain`](Self::drain), an error for a fatal
    /// poll outcome, and [`ClientError::Config`] at once for a configuration
    /// the runner refuses (see [`RunnerBuilder::allow_insecure_http`],
    /// [`RunnerBuilder::max_inflight`]).
    pub async fn start(&self) -> Result<(), ClientError> {
        if let Some(e) = &self.config_error {
            tracing::error!(error = %e, "refusing to start the runner");
            return Err(ClientError::Config(e.clone()));
        }

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
        // Transient poll failures in a row, for the exponential backoff.
        let mut consecutive_transient: u32 = 0;

        loop {
            // Registered before the flag check, so a `drain()` that lands
            // after it still wakes the waits below.
            let drain_requested = self.drain_requested.notified();
            tokio::pin!(drain_requested);
            drain_requested.as_mut().enable();

            if self.draining.load(Ordering::Relaxed) {
                return self.finish_drain().await;
            }

            // Registered before the snapshot: a handler that finishes after it
            // wakes this future, and one that finished before it is already
            // missing from the snapshot — so the backoff below cannot sleep
            // through a slot that freed in between.
            let slot_freed = self.slot_freed.notified();
            tokio::pin!(slot_freed);
            slot_freed.as_mut().enable();

            let inflight = self.inflight.read().await.clone();
            let capacity = (self.max_inflight as usize).saturating_sub(inflight.len());
            let at_capacity = capacity == 0;

            // Control-slot polling (issue #176): at capacity we still poll
            // so the server can deliver cancels for in-flight executions
            // via `PollResponse.cancel`. The server's poll handler returns
            // immediately when `inflight.len() == max_inflight` (no
            // long-poll), so `capacity_backoff` is what paces the loop and
            // prevents a stampede. `at_capacity` is derived from the same
            // snapshot the request carries, so the decision and the server's
            // view agree (the .NET/Go/Java SDKs once read them separately and
            // raced — issue #817).
            let poll_req = PollRequest {
                runner_id: self.runner_id.clone(),
                capabilities: self.capabilities.clone(),
                max_inflight: self.max_inflight,
                inflight,
                instance_id: Some(self.instance_id.clone()),
                tags: self.tags.clone(),
            };

            // A drain ends a long poll in progress, as the other SDKs' context
            // cancellation does: the shutdown should not wait out the server's
            // 30 s window for work it is about to refuse.
            let poll_result = tokio::select! {
                result = self.client.poll(&poll_req) => result,
                () = &mut drain_requested => continue,
            };
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
            let retry_delay = match poll_result {
                Ok(resp) => {
                    consecutive_transient = 0;
                    // Honour server-issued cancels (issue #176): abort the
                    // matching in-flight handler futures. Each dispatch task
                    // then acks its execution as a failure and releases the
                    // inflight slot. Done on every poll, at capacity too, so
                    // a max_inflight=1 runner still acts on cancels.
                    Self::abort_cancelled(&self.aborts, &resp.cancel).await;

                    // Delivered work is dispatched on an at-capacity poll
                    // too. By the time the response arrives the server has
                    // committed the claim, so dropping an assignment strands
                    // it as `claimed` until timeout + grace; for a singleton
                    // job that blocks every later run (issue #817). Briefly
                    // running over `max_inflight` is the lesser evil.
                    for assignment in resp.work {
                        self.dispatch(assignment).await;
                    }

                    if at_capacity {
                        // Server returned immediately (capacity=0 branch).
                        // Pace the loop to avoid hammering the server — but
                        // only until a handler finishes. A fixed sleep made
                        // the backoff, not the work, the pace of dispatch for
                        // short handlers (issue #845).
                        tokio::select! {
                            () = &mut slot_freed => {}
                            () = &mut drain_requested => {}
                            () = tokio::time::sleep(self.capacity_backoff) => {}
                        }
                    }
                    continue;
                }
                Err(e @ ClientError::WorkOwnershipDenied { .. }) => {
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
                Err(ClientError::Unauthorized { .. }) => {
                    consecutive_transient = 0;
                    let delay = jitter(self.poll_retry_delay);
                    tracing::warn!(
                        runner_id = %self.runner_id,
                        consecutive = consecutive_auth_failures,
                        max = self.max_consecutive_auth_failures,
                        delay_ms = delay.as_millis() as u64,
                        "poll returned 401 Unauthorized — the API key was rejected; will retry"
                    );
                    delay
                }
                Err(ClientError::PollInstanceConflict { .. }) => {
                    consecutive_transient = 0;
                    let delay = jitter(self.poll_retry_delay);
                    tracing::warn!(
                        runner_id = %self.runner_id,
                        consecutive = consecutive_conflicts,
                        max = self.max_consecutive_poll_conflicts,
                        delay_ms = delay.as_millis() as u64,
                        "poll returned 409 Conflict — another runner instance may be active; \
                         will retry"
                    );
                    delay
                }
                Err(e) => {
                    consecutive_transient = consecutive_transient.saturating_add(1);
                    let delay = poll_backoff(self.poll_retry_delay, consecutive_transient);
                    // The whole chain, not just the top line. reqwest's
                    // `Display` is "error sending request for url (…)" and
                    // says nothing about whether the connection was refused,
                    // reset, closed mid-response or timed out — which is the
                    // only part an operator can act on (#648).
                    tracing::warn!(
                        error = %crate::error_chain::chain(&e),
                        consecutive = consecutive_transient,
                        delay_ms = delay.as_millis() as u64,
                        "poll failed — retrying"
                    );
                    delay
                }
            };
            tokio::select! {
                () = &mut drain_requested => {}
                () = tokio::time::sleep(retry_delay) => {}
            }
        }
    }

    /// Take one work assignment: refuse it if its identifiers are unsafe,
    /// otherwise run it on its own task with lease renewal, and ack it.
    async fn dispatch(&self, assignment: crate::client::WorkAssignment) {
        // Ingest guard for hostile identifiers (issue #441; review 2026-10-24,
        // R-7 / K-2), conformance cases 13 and 14.
        match identifiers::reject_reason(&assignment.execution_id, &assignment.job_key) {
            Some(RejectedField::ExecutionId) => {
                // Nothing safe to address the server with: drop, no ack. The
                // server-side lease expires on its own (case 14).
                tracing::warn!(
                    execution_id = %identifiers::preview_for_log(&assignment.execution_id),
                    "dropping a work assignment with an unsafe execution_id — not acked"
                );
                return;
            }
            Some(RejectedField::JobKey) => {
                // The id is fine, so say why: a failure ack dead-letters the
                // execution with an explanation instead of a silent requeue
                // loop (case 13). No handler — not even a catch-all — runs.
                let error = identifiers::rejection_ack_error(&assignment.job_key);
                tracing::warn!(
                    execution_id = %assignment.execution_id,
                    error = %error,
                    "refusing a work assignment with an unsafe job_key — acking it as failed"
                );
                let client = Arc::clone(&self.client);
                let ack = AckRequest {
                    runner_id: self.runner_id.clone(),
                    execution_id: assignment.execution_id,
                    status: "failure".to_string(),
                    error: Some(error),
                    duration_ms: Some(0),
                    attempt: assignment.attempt,
                };
                tokio::spawn(async move {
                    if let Err(e) = ack_with_retry(&client, &ack, &ACK_RETRY_DELAYS).await {
                        tracing::error!(execution_id = %ack.execution_id, error = %e, "failed to ack");
                    }
                });
                return;
            }
            None => {}
        }

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
                    // `?`: a server-supplied value, escaped by Debug.
                    tracing::warn!(value = ?s, error = %e, "failed to parse scheduled_for");
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
        let slot_freed = Arc::clone(&self.slot_freed);
        let renew_interval = self.renew_interval;

        tokio::spawn(async move {
            let attempt = ctx.attempt;

            // Find handler
            let handler = {
                let reg = handlers.read().await;
                reg.get(&job_key).cloned()
            };

            let (status, error, duration_ms) = if let Some(handler) = handler {
                let start = std::time::Instant::now();

                // Run the handler on its own task so a server cancel, a lost
                // lease or a drain timeout can abort *only* the handler
                // future, leaving this task free to ack + release the
                // inflight slot (issue #176). The abort handle is registered
                // under the execution id so the poll loop can reach it.
                let handler_task = tokio::spawn(async move { handler(ctx).await });
                let abort = Abort {
                    handle: handler_task.abort_handle(),
                    reason: Arc::new(AtomicU8::new(ABORT_NONE)),
                };
                aborts.write().await.insert(exec_id.clone(), abort.clone());

                let renew_handle = tokio::spawn(renew_lease(
                    Arc::clone(&client),
                    runner_id.clone(),
                    exec_id.clone(),
                    renew_interval,
                    abort.clone(),
                ));

                let join = handler_task.await;
                renew_handle.abort();
                aborts.write().await.remove(&exec_id);

                let duration_ms = start.elapsed().as_millis() as i64;
                classify_join(join, abort.reason.load(Ordering::Acquire), duration_ms)
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
            match ack_with_retry(&client, &ack, &ACK_RETRY_DELAYS).await {
                Ok(()) => {}
                Err(e @ ClientError::WorkOwnershipDenied { .. }) => {
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
                    tracing::error!(
                        execution_id = %exec_id,
                        error = %crate::error_chain::chain(&e),
                        "failed to ack — the server's stale-claim reaper will requeue the \
                         execution"
                    );
                }
            }

            // Remove from inflight
            inflight.write().await.retain(|id| id != &exec_id);
            slot_freed.notify_waiters();
        });
    }
}

/// Renew the lease on one in-flight execution every `interval` until aborted.
///
/// * `403` — the credential does not own the `runner_id` (ADR-0008). That is
///   permanent, so renewing stops after one error line instead of repeating it
///   every interval for the rest of the handler (review 2026-10-24, R-8).
/// * `404` / `409` — the server no longer holds this execution for this runner:
///   the claim was reaped after a partition longer than `timeout + grace`,
///   released at a server restart, or the execution reached a terminal state.
///   It may already be running elsewhere, so this handler is aborted and acked
///   as failure (review 2026-10-24, R-3). It used to be logged at debug as a
///   race with our own completion, but the dispatch task stops this loop
///   before it acks, so that race no longer reaches here.
/// * anything else — transient; retried at the next interval.
async fn renew_lease(
    client: Arc<CroniqClient>,
    runner_id: String,
    execution_id: String,
    interval: Duration,
    abort: Abort,
) {
    loop {
        tokio::time::sleep(interval).await;
        // The result used to be discarded outright (`let _ = …`), which hid a
        // misconfigured credential completely — see #437.
        let renewed = client
            .renew(&RenewRequest {
                runner_id: runner_id.clone(),
                execution_id: execution_id.clone(),
            })
            .await;
        match renewed {
            Ok(()) => {}
            Err(ClientError::WorkOwnershipDenied { .. }) => {
                tracing::error!(
                    runner_id = %runner_id,
                    execution_id = %execution_id,
                    "lease renew refused with 403 Forbidden — this runner's credential does not \
                     own runner_id, so renewing stops. The lease will expire and the execution \
                     be reclaimed. Give the runner its own runner_id, or release the existing \
                     binding with DELETE /v1/runners/{{id}}."
                );
                return;
            }
            Err(ClientError::Server {
                status: status @ (404 | 409),
                ..
            }) => {
                tracing::warn!(
                    execution_id = %execution_id,
                    status,
                    "lease lost — the server no longer holds this execution for this runner; \
                     aborting the handler so it does not run twice"
                );
                abort.abort(ABORT_LEASE_LOST);
                return;
            }
            Err(e) => {
                tracing::warn!(
                    execution_id = %execution_id,
                    error = %e,
                    "lease renew failed — will retry"
                );
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
    fn a_401_resets_the_conflict_streak() {
        // The specific instance of the rule above that four of the six SDKs
        // got wrong: their 401 branch returned before reaching the reset, so
        // 409, 409, 401, 409 counted as three consecutive conflicts and killed
        // the runner with the wrong error (issue #508). Rust reaches it
        // through `update_conflict_streak`'s catch-all `Err(_)` arm; this test
        // is here so a future refactor of that match cannot quietly join them.
        let mut streak = 2;
        let action = update_conflict_streak(&unauthorized(), &mut streak, 3);
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
        let aborts: RwLock<HashMap<String, Abort>> = RwLock::new(HashMap::new());

        let task = tokio::spawn(async {
            tokio::time::sleep(Duration::from_secs(3600)).await;
        });
        let reason = Arc::new(AtomicU8::new(ABORT_NONE));
        aborts.write().await.insert(
            "exec-1".to_string(),
            Abort {
                handle: task.abort_handle(),
                reason: Arc::clone(&reason),
            },
        );

        // A cancel for a different id must leave the handler running.
        CroniqRunner::abort_cancelled(&aborts, &["other".to_string()]).await;
        assert!(!task.is_finished(), "non-matching cancel must not abort");

        // A cancel for the matching id aborts the handler future.
        CroniqRunner::abort_cancelled(&aborts, &["exec-1".to_string()]).await;
        assert!(task.await.unwrap_err().is_cancelled());
        assert_eq!(reason.load(Ordering::Acquire), ABORT_SERVER_CANCEL);
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
        let (status, error, duration_ms) = classify_join(task.await, ABORT_NONE, 42);
        assert_eq!(status, "failure");
        assert!(error.unwrap().contains("cancelled"));
        assert_eq!(duration_ms, 42);
    }

    #[test]
    fn classify_join_reports_success_and_handler_error() {
        let (status, error, duration_ms) = classify_join(Ok(Ok(())), ABORT_NONE, 7);
        assert_eq!(status, "success");
        assert!(error.is_none());
        assert_eq!(duration_ms, 7);

        let (status, error, _) = classify_join(Ok(Err(HandlerError::msg("boom"))), ABORT_NONE, 7);
        assert_eq!(status, "failure");
        assert_eq!(error.as_deref(), Some("boom"));
    }

    // ─── Backoff with jitter (review 2026-10-24, R-8 / K-9) ─────────────

    #[test]
    fn jitter_stays_within_twenty_percent_and_varies() {
        let base = Duration::from_millis(1000);
        let samples: Vec<Duration> = (0..200).map(|_| jitter(base)).collect();
        for s in &samples {
            assert!(
                *s >= Duration::from_millis(800) && *s <= Duration::from_millis(1200),
                "{s:?}"
            );
        }
        let distinct: std::collections::HashSet<_> = samples.iter().collect();
        assert!(distinct.len() > 10, "a fleet must not retry in lockstep");
    }

    #[test]
    fn poll_backoff_doubles_and_is_capped() {
        let base = Duration::from_millis(400);
        let within = |d: Duration, nominal_ms: u64| {
            d >= Duration::from_millis(nominal_ms * 8 / 10)
                && d <= Duration::from_millis(nominal_ms * 12 / 10)
        };
        assert!(within(poll_backoff(base, 1), 400));
        assert!(within(poll_backoff(base, 2), 800));
        assert!(within(poll_backoff(base, 3), 1600));
        assert!(within(poll_backoff(base, 40), 60_000), "capped at 60 s");
        // A configured delay above the cap is honoured, not cut down.
        assert!(within(poll_backoff(Duration::from_secs(90), 5), 90_000));
    }

    // ─── Ack retry (review 2026-10-24, K-1) ─────────────────────────────

    /// A server that answers `/v1/work/ack` with the given statuses in turn
    /// (the last one repeating) and counts the calls.
    async fn ack_server(statuses: Vec<u16>) -> (String, Arc<std::sync::atomic::AtomicUsize>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                let n = seen.fetch_add(1, Ordering::SeqCst);
                let status = statuses[n.min(statuses.len() - 1)];
                let mut buf = [0u8; 4096];
                let _ = sock.read(&mut buf).await;
                let resp = format!(
                    "HTTP/1.1 {status} X\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
                );
                let _ = sock.write_all(resp.as_bytes()).await;
            }
        });
        (base, calls)
    }

    fn ack_req() -> AckRequest {
        AckRequest {
            runner_id: "r1".into(),
            execution_id: "e1".into(),
            status: "success".into(),
            error: None,
            duration_ms: Some(1),
            attempt: 1,
        }
    }

    const SHORT: [Duration; 2] = [Duration::from_millis(10), Duration::from_millis(10)];

    #[tokio::test]
    async fn a_5xx_ack_is_retried_until_it_succeeds() {
        let (base, calls) = ack_server(vec![503, 200]).await;
        let client = CroniqClient::new(&base);
        ack_with_retry(&client, &ack_req(), &SHORT).await.unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "503 then 200: exactly two acks"
        );
    }

    #[tokio::test]
    async fn ack_retries_stop_after_three_attempts() {
        let (base, calls) = ack_server(vec![500]).await;
        let client = CroniqClient::new(&base);
        let err = ack_with_retry(&client, &ack_req(), &SHORT)
            .await
            .unwrap_err();
        assert!(matches!(err, ClientError::Server { status: 500, .. }));
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn a_4xx_ack_is_never_retried() {
        for status in [400, 403, 404, 409] {
            let (base, calls) = ack_server(vec![status, 200]).await;
            let client = CroniqClient::new(&base);
            assert!(ack_with_retry(&client, &ack_req(), &SHORT).await.is_err());
            assert_eq!(calls.load(Ordering::SeqCst), 1, "status {status}");
        }
    }

    #[test]
    fn ack_retry_delays_are_short_and_three_attempts_in_total() {
        // The agreed cross-SDK semantics: 3 attempts, short delays.
        assert_eq!(ACK_RETRY_DELAYS.len(), 2);
        assert!(ACK_RETRY_DELAYS.iter().sum::<Duration>() <= Duration::from_secs(5));
    }

    // ─── Abort reasons (review 2026-10-24, R-3 / R-4) ────────────────────

    #[tokio::test]
    async fn an_aborted_handler_reports_why() {
        for (reason, needle) in [
            (ABORT_SERVER_CANCEL, "cancelled by server"),
            (ABORT_LEASE_LOST, "lease lost"),
            (ABORT_DRAIN, "shutting down"),
        ] {
            let task: tokio::task::JoinHandle<Result<(), HandlerError>> =
                tokio::spawn(std::future::pending());
            task.abort();
            let (status, error, _) = classify_join(task.await, reason, 1);
            assert_eq!(status, "failure");
            assert!(error.as_deref().unwrap().contains(needle), "{error:?}");
        }
    }

    #[tokio::test]
    async fn the_first_abort_reason_wins() {
        let task = tokio::spawn(std::future::pending::<()>());
        let abort = Abort {
            handle: task.abort_handle(),
            reason: Arc::new(AtomicU8::new(ABORT_NONE)),
        };
        abort.abort(ABORT_LEASE_LOST);
        abort.abort(ABORT_DRAIN);
        assert_eq!(abort.reason.load(Ordering::Acquire), ABORT_LEASE_LOST);
    }

    // ─── Refused configurations (review 2026-10-24, K-3 / R-12) ──────────

    #[tokio::test]
    async fn cleartext_to_a_remote_host_refuses_to_start() {
        let runner = CroniqRunner::builder("http://croniq-server:4000", "r").build();
        let err = runner.start().await.unwrap_err();
        assert!(
            matches!(&err, ClientError::Config(m) if m.contains("cleartext")),
            "{err}"
        );
        // The opt-in and https are accepted (not started: nothing listens).
        assert!(
            CroniqRunner::builder("http://croniq-server:4000", "r")
                .allow_insecure_http()
                .build()
                .config_error
                .is_none()
        );
        assert!(
            CroniqRunner::builder("https://croniq.example", "r")
                .build()
                .config_error
                .is_none()
        );
    }

    #[tokio::test]
    async fn zero_max_inflight_refuses_to_start() {
        let runner = CroniqRunner::builder("http://localhost:4000", "r")
            .max_inflight(0)
            .build();
        let err = runner.start().await.unwrap_err();
        assert!(
            matches!(&err, ClientError::Config(m) if m.contains("max_inflight")),
            "{err}"
        );
    }

    #[test]
    fn api_key_wins_over_bearer() {
        // Parity with the Go SDK and TriggerClient.
        let runner = CroniqRunner::builder("http://localhost:4000", "r")
            .bearer_token("tok")
            .api_key("key")
            .build();
        assert_eq!(runner.client.auth_header_for_tests(), Some("ApiKey key"));
        let runner = CroniqRunner::builder("http://localhost:4000", "r")
            .bearer_token("tok")
            .build();
        assert_eq!(runner.client.auth_header_for_tests(), Some("Bearer tok"));
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

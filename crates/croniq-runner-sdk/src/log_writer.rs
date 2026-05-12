//! Streaming log writer for the Croniq runner SDK (issue #115).
//!
//! The direct path — `ctx.log()` and `ctx.push_log_events()` — awaits the
//! HTTP request to the server inline, which forces SDK-based runners that
//! wrap long-running subprocesses to choose between two bad options:
//!
//! - **batch-at-end** (what `croniq-shell-runner` does): collect output
//!   until the process exits and POST it in one shot — no live progress
//!   in the UI for multi-minute jobs.
//! - **per-line `ctx.log().await`** in a stdout reader: live progress,
//!   but a slow server backpressures the reader → the subprocess's
//!   stdout pipe fills → the subprocess blocks on write, potentially
//!   self-induced deadlock for chatty jobs.
//!
//! [`LogWriter`] decouples senders from server latency: a bounded
//! `mpsc::channel` feeds a background flusher that batches events by size
//! (32 events) or time (200 ms), capped at 100 events per HTTP POST.
//! `send().await` only suspends on channel capacity, never on HTTP.
//!
//! # Lifecycle
//!
//! Acquire a writer via [`crate::ExecutionContext::log_writer`] inside a
//! handler. The handle is `Clone` so it can be passed into child tasks
//! that fan out reads of stdout/stderr. The flusher is lazily spawned on
//! the first `log_writer()` call and shared across all clones for one
//! execution. The runner awaits its drain (up to 5 s) before sending the
//! `ack` request, guaranteeing logs are server-side before the execution
//! is marked complete.
//!
//! # Error handling
//!
//! Failures are swallowed with `tracing::warn` to match the existing
//! `ctx.log()` ergonomics — log delivery is best-effort and never the
//! critical path. The `ack` flow is independent.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::{MissedTickBehavior, interval};

use crate::client::{ClientError, CroniqClient, WorkEvent};
use crate::enrichment::{enrich_event, serialize_tags};

// ─── Tunables ──────────────────────────────────────────────────────────────

/// Bounded channel capacity. 256 admits roughly one second of typical
/// bursty output (~250 lines/sec) before producers feel backpressure —
/// big enough that a chatty test suite doesn't stall on every event,
/// small enough that genuine server slowness produces backpressure
/// instead of unbounded memory growth.
pub(crate) const CHANNEL_CAPACITY: usize = 256;

/// Number of buffered events that triggers an immediate flush. Picked
/// to keep HTTP POST sizes predictable for typical line lengths.
pub(crate) const BATCH_SIZE_THRESHOLD: usize = 32;

/// Maximum time the flusher will hold events before posting. The SSE-like
/// "live progress" feel benefits from sub-second cadence.
pub(crate) const BATCH_TIME_THRESHOLD: Duration = Duration::from_millis(200);

/// Hard cap on events per HTTP POST. A single chatty wake-up that fills
/// the channel with 256 events gets posted in three chunks rather than
/// one ~150 KB body.
pub(crate) const MAX_BATCH_PER_POST: usize = 100;

/// Wall-clock budget for [`LogWriterInner::shutdown_and_drain`]. If the
/// server is unreachable at job-end time, the runner moves on to `ack`
/// after this budget regardless — losing late events but not blocking
/// the entire dispatch loop.
pub(crate) const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

// ─── HTTP pusher trait (for mockable tests) ─────────────────────────────────

/// Object-safe interface the flusher uses to post batches. The blanket
/// impl below wraps [`CroniqClient::push_events`]; tests substitute a
/// recording mock without bringing in `wiremock`/`httpmock`.
pub(crate) trait HttpPusher: Send + Sync + 'static {
    fn push_events_boxed<'a>(
        &'a self,
        execution_id: &'a str,
        events: &'a [WorkEvent],
    ) -> Pin<Box<dyn Future<Output = Result<(), ClientError>> + Send + 'a>>;
}

impl HttpPusher for CroniqClient {
    fn push_events_boxed<'a>(
        &'a self,
        execution_id: &'a str,
        events: &'a [WorkEvent],
    ) -> Pin<Box<dyn Future<Output = Result<(), ClientError>> + Send + 'a>> {
        Box::pin(CroniqClient::push_events(self, execution_id, events))
    }
}

// ─── Public handle ──────────────────────────────────────────────────────────

/// Cloneable, fire-and-forget streaming log handle.
///
/// `LogWriter::send` / `send_event` enqueue an event into a bounded
/// channel and return as soon as the slot is allocated — never blocking
/// on the server. Use [`LogWriter::flush`] to wait for currently-queued
/// events to be successfully POSTed (e.g. before emitting a final
/// "completed" log line via the direct path).
#[derive(Clone)]
pub struct LogWriter {
    tx: mpsc::Sender<Cmd>,
}

enum Cmd {
    Event(WorkEvent),
    Flush(oneshot::Sender<()>),
}

impl LogWriter {
    /// Push a structured log event. Async only because the bounded channel
    /// may apply backpressure when the server is genuinely slow — this is
    /// the intended mechanism that propagates pressure back to the caller
    /// without filling stdout pipes. Errors (closed channel) are swallowed
    /// with `tracing::warn` to match the [`crate::ExecutionContext::log`]
    /// ergonomics.
    pub async fn send(&self, level: &str, message: impl Into<String>) {
        self.send_event(WorkEvent {
            level: Some(level.into()),
            message: message.into(),
            fields: Default::default(),
        })
        .await
    }

    /// Push a fully-populated [`WorkEvent`] (use this when you need to
    /// attach custom `fields`). Same fire-and-forget semantics as `send`.
    pub async fn send_event(&self, event: WorkEvent) {
        if self.tx.send(Cmd::Event(event)).await.is_err() {
            tracing::warn!("log_writer channel closed; event dropped");
        }
    }

    /// Wait until every event currently queued has been POSTed to the
    /// server. Returns immediately if the flusher has already exited.
    /// Useful before emitting a summary line that the operator expects
    /// to see *after* the streamed output.
    pub async fn flush(&self) {
        let (ack_tx, ack_rx) = oneshot::channel();
        if self.tx.send(Cmd::Flush(ack_tx)).await.is_err() {
            return; // channel closed → nothing left to flush
        }
        let _ = ack_rx.await; // ignore: flusher might have shut down
    }
}

// ─── Internal: spawned per execution ────────────────────────────────────────

/// One instance per `ExecutionContext`, shared across writer clones via
/// the `OnceLock` on the context. Owns the flusher's `JoinHandle` and the
/// shutdown signal so the runner can deterministically drain before ack.
pub(crate) struct LogWriterInner {
    tx: mpsc::Sender<Cmd>,
    shutdown_tx: Mutex<Option<oneshot::Sender<()>>>,
    join: Mutex<Option<JoinHandle<()>>>,
}

impl LogWriterInner {
    /// Spawn the flusher task. Must be called from inside a Tokio
    /// runtime (handlers are — they're spawned by the runner's runtime).
    pub(crate) fn spawn(
        client: Arc<dyn HttpPusher>,
        execution_id: String,
        job_key: String,
        runner_id: String,
        runner_tags: Vec<String>,
    ) -> Self {
        let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let serialized_tags = serialize_tags(&runner_tags);

        let join = tokio::spawn(flusher_task(
            rx,
            shutdown_rx,
            client,
            execution_id,
            job_key,
            runner_id,
            serialized_tags,
        ));

        Self {
            tx,
            shutdown_tx: Mutex::new(Some(shutdown_tx)),
            join: Mutex::new(Some(join)),
        }
    }

    pub(crate) fn handle(&self) -> LogWriter {
        LogWriter {
            tx: self.tx.clone(),
        }
    }

    /// Signal the flusher to drain and exit, then await with a 5s
    /// timeout. Idempotent — repeat calls are no-ops.
    pub(crate) async fn shutdown_and_drain(&self) {
        if let Some(tx) = self.shutdown_tx.lock().await.take() {
            let _ = tx.send(());
        }
        let join = self.join.lock().await.take();
        if let Some(handle) = join
            && tokio::time::timeout(SHUTDOWN_TIMEOUT, handle)
                .await
                .is_err()
        {
            tracing::warn!(
                timeout_secs = SHUTDOWN_TIMEOUT.as_secs(),
                "log_writer drain timed out — late events may be lost"
            );
        }
    }
}

// ─── Flusher task ──────────────────────────────────────────────────────────

async fn flusher_task(
    mut rx: mpsc::Receiver<Cmd>,
    mut shutdown_rx: oneshot::Receiver<()>,
    client: Arc<dyn HttpPusher>,
    execution_id: String,
    job_key: String,
    runner_id: String,
    serialized_tags: Option<String>,
) {
    let mut buffer: Vec<WorkEvent> = Vec::new();
    let mut ticker = interval(BATCH_TIME_THRESHOLD);
    // Don't try to catch up after a long flush — just space ticks BATCH_TIME
    // apart from "now".
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    // First tick fires immediately; consume it so the time-flush trigger is
    // actually relative to the first queued event, not task spawn.
    ticker.tick().await;

    loop {
        tokio::select! {
            biased;
            // Shutdown signal: drain remaining events from the channel
            // without awaiting more, flush, and exit. Late `send()`s from
            // user-held clones after this point will hit a closed channel.
            _ = &mut shutdown_rx => {
                while let Ok(cmd) = rx.try_recv() {
                    match cmd {
                        Cmd::Event(e) => buffer.push(e),
                        Cmd::Flush(ack) => { let _ = ack.send(()); }
                    }
                }
                flush_all(
                    &client,
                    &execution_id,
                    &job_key,
                    &runner_id,
                    serialized_tags.as_deref(),
                    &mut buffer,
                ).await;
                return;
            }
            cmd = rx.recv() => {
                match cmd {
                    Some(Cmd::Event(e)) => {
                        buffer.push(e);
                        if buffer.len() >= BATCH_SIZE_THRESHOLD {
                            flush_all(
                                &client,
                                &execution_id,
                                &job_key,
                                &runner_id,
                                serialized_tags.as_deref(),
                                &mut buffer,
                            ).await;
                        }
                    }
                    Some(Cmd::Flush(ack)) => {
                        flush_all(
                            &client,
                            &execution_id,
                            &job_key,
                            &runner_id,
                            serialized_tags.as_deref(),
                            &mut buffer,
                        ).await;
                        let _ = ack.send(());
                    }
                    None => {
                        // All senders dropped → drain remainder and exit.
                        flush_all(
                            &client,
                            &execution_id,
                            &job_key,
                            &runner_id,
                            serialized_tags.as_deref(),
                            &mut buffer,
                        ).await;
                        return;
                    }
                }
            }
            _ = ticker.tick() => {
                if !buffer.is_empty() {
                    flush_all(
                        &client,
                        &execution_id,
                        &job_key,
                        &runner_id,
                        serialized_tags.as_deref(),
                        &mut buffer,
                    ).await;
                }
            }
        }
    }
}

/// Drain `buffer` into the HTTP client, chunked at [`MAX_BATCH_PER_POST`].
/// Each chunk is enriched with `job_key`/`runner_id`/`runner_tags` so the
/// payload matches what `ctx.push_log_events` already produces. HTTP
/// failures drop the chunk with a `tracing::warn` — matches the existing
/// `ctx.log()` semantics; retries would block the next batch and worsen
/// tail latency for chatty jobs.
async fn flush_all(
    client: &Arc<dyn HttpPusher>,
    execution_id: &str,
    job_key: &str,
    runner_id: &str,
    serialized_tags: Option<&str>,
    buffer: &mut Vec<WorkEvent>,
) {
    while !buffer.is_empty() {
        let take = buffer.len().min(MAX_BATCH_PER_POST);
        let chunk: Vec<WorkEvent> = buffer
            .drain(..take)
            .map(|e| enrich_event(&e, job_key, runner_id, serialized_tags))
            .collect();
        if let Err(err) = client.push_events_boxed(execution_id, &chunk).await {
            tracing::warn!(
                execution_id = %execution_id,
                error = %err,
                dropped = chunk.len(),
                "log_writer: batch POST failed — events lost"
            );
            // Continue draining; if this is a transient server issue,
            // later batches may succeed.
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex as StdMutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Recording mock that captures every batch posted, with an optional
    /// failure injector and per-call latency knob.
    struct MockPusher {
        posts: Arc<StdMutex<Vec<Vec<WorkEvent>>>>,
        fail_first: Arc<AtomicUsize>,
        latency: Option<Duration>,
    }

    impl MockPusher {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                posts: Arc::new(StdMutex::new(Vec::new())),
                fail_first: Arc::new(AtomicUsize::new(0)),
                latency: None,
            })
        }

        fn with_failures(n: usize) -> Arc<Self> {
            Arc::new(Self {
                posts: Arc::new(StdMutex::new(Vec::new())),
                fail_first: Arc::new(AtomicUsize::new(n)),
                latency: None,
            })
        }

        fn with_latency(latency: Duration) -> Arc<Self> {
            Arc::new(Self {
                posts: Arc::new(StdMutex::new(Vec::new())),
                fail_first: Arc::new(AtomicUsize::new(0)),
                latency: Some(latency),
            })
        }

        fn captured(&self) -> Vec<Vec<WorkEvent>> {
            self.posts.lock().unwrap().clone()
        }

        fn total_events(&self) -> usize {
            self.posts.lock().unwrap().iter().map(|b| b.len()).sum()
        }
    }

    impl HttpPusher for MockPusher {
        fn push_events_boxed<'a>(
            &'a self,
            _execution_id: &'a str,
            events: &'a [WorkEvent],
        ) -> Pin<Box<dyn Future<Output = Result<(), ClientError>> + Send + 'a>> {
            Box::pin(async move {
                if let Some(d) = self.latency {
                    tokio::time::sleep(d).await;
                }
                if self.fail_first.load(Ordering::SeqCst) > 0 {
                    self.fail_first.fetch_sub(1, Ordering::SeqCst);
                    return Err(ClientError::Server {
                        status: 503,
                        body: "mock failure".into(),
                    });
                }
                self.posts.lock().unwrap().push(events.to_vec());
                Ok(())
            })
        }
    }

    fn spawn_writer(pusher: Arc<dyn HttpPusher>) -> (LogWriter, Arc<LogWriterInner>) {
        let inner = Arc::new(LogWriterInner::spawn(
            pusher,
            "exec-1".into(),
            "job:test".into(),
            "runner-1".into(),
            vec!["env=test".into()],
        ));
        let writer = inner.handle();
        (writer, inner)
    }

    fn ev(msg: &str) -> WorkEvent {
        WorkEvent {
            level: Some("info".into()),
            message: msg.into(),
            fields: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn flushes_on_size_threshold() {
        let pusher = MockPusher::new();
        let (w, inner) = spawn_writer(pusher.clone());

        for i in 0..BATCH_SIZE_THRESHOLD {
            w.send_event(ev(&format!("line {i}"))).await;
        }
        // Give the flusher a moment to react to the size trigger.
        tokio::time::sleep(Duration::from_millis(50)).await;
        inner.shutdown_and_drain().await;

        let posts = pusher.captured();
        // Exactly one batch should have been posted, of exactly the
        // threshold size — anything larger means the size trigger didn't
        // fire on the threshold-crossing event.
        assert_eq!(posts.len(), 1, "expected one batch, got {}", posts.len());
        assert_eq!(posts[0].len(), BATCH_SIZE_THRESHOLD);
        // Enrichment must have happened in-flight.
        assert_eq!(posts[0][0].fields.get("job_key").unwrap(), "job:test");
        assert_eq!(posts[0][0].fields.get("runner_id").unwrap(), "runner-1");
        assert_eq!(
            posts[0][0].fields.get("runner_tags").unwrap(),
            r#"["env=test"]"#
        );
    }

    #[tokio::test]
    async fn flushes_on_time_threshold() {
        let pusher = MockPusher::new();
        let (w, inner) = spawn_writer(pusher.clone());

        // Push fewer events than the size threshold so only the time
        // trigger can flush them.
        for i in 0..5 {
            w.send_event(ev(&format!("line {i}"))).await;
        }
        // Wait long enough for the time-based tick to fire (200 ms) plus
        // some slack for scheduling.
        tokio::time::sleep(BATCH_TIME_THRESHOLD + Duration::from_millis(100)).await;
        inner.shutdown_and_drain().await;

        let posts = pusher.captured();
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0].len(), 5);
    }

    #[tokio::test]
    async fn flush_waits_for_pending_post() {
        let pusher = MockPusher::with_latency(Duration::from_millis(80));
        let (w, inner) = spawn_writer(pusher.clone());

        for i in 0..5 {
            w.send_event(ev(&format!("line {i}"))).await;
        }
        w.flush().await; // must return AFTER the POST completes
        assert_eq!(pusher.captured().len(), 1);
        assert_eq!(pusher.total_events(), 5);

        inner.shutdown_and_drain().await;
    }

    #[tokio::test]
    async fn shutdown_drains_remaining_events() {
        let pusher = MockPusher::new();
        let (w, inner) = spawn_writer(pusher.clone());

        for i in 0..7 {
            w.send_event(ev(&format!("line {i}"))).await;
        }
        // Don't wait for the time threshold — shutdown should drain.
        inner.shutdown_and_drain().await;

        assert_eq!(pusher.total_events(), 7);
    }

    #[tokio::test]
    async fn respects_max_batch_per_post() {
        // Push 250 events at once and verify the flusher splits into
        // chunks of at most MAX_BATCH_PER_POST without losing any.
        let pusher = MockPusher::new();
        let (w, inner) = spawn_writer(pusher.clone());

        for i in 0..250 {
            w.send_event(ev(&format!("line {i}"))).await;
        }
        inner.shutdown_and_drain().await;

        let posts = pusher.captured();
        let total: usize = posts.iter().map(|b| b.len()).sum();
        assert_eq!(total, 250, "no events should be lost on shutdown drain");
        for batch in &posts {
            assert!(
                batch.len() <= MAX_BATCH_PER_POST,
                "batch of {} exceeded MAX_BATCH_PER_POST",
                batch.len()
            );
        }
    }

    #[tokio::test]
    async fn http_failure_drops_batch_but_keeps_flusher_alive() {
        // First POST returns 503; subsequent ones succeed. The flusher
        // must keep running and post later events.
        let pusher = MockPusher::with_failures(1);
        let (w, inner) = spawn_writer(pusher.clone());

        for i in 0..BATCH_SIZE_THRESHOLD {
            w.send_event(ev(&format!("first {i}"))).await;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        for i in 0..5 {
            w.send_event(ev(&format!("second {i}"))).await;
        }
        inner.shutdown_and_drain().await;

        let posts = pusher.captured();
        // First batch was failed-and-dropped → captured contains only the
        // second send. Total events captured is 5, not 32+5.
        assert_eq!(pusher.total_events(), 5, "failed batch should be dropped");
        assert!(!posts.is_empty(), "flusher must survive HTTP error");
    }

    #[tokio::test]
    async fn send_after_shutdown_is_silently_swallowed() {
        let pusher = MockPusher::new();
        let (w, inner) = spawn_writer(pusher.clone());

        inner.shutdown_and_drain().await;
        // No panic, no hang — just a `tracing::warn` we cannot easily
        // assert on. The contract is "fire-and-forget after shutdown".
        w.send_event(ev("late line")).await;
    }
}

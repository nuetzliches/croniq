//! End-to-end integration tests for croniq-server.
//!
//! These tests exercise the complete pipeline:
//!
//! ```text
//! Croniqfile ──load──► SchedulerLoop::tick()
//!                           │
//!                           ▼ enqueue WorkItem
//!                      POST /v1/poll  ←── runner
//!                           │
//!                           ▼ dispatch work
//!                      runner executes
//!                           │
//!                      POST /v1/complete ──► CompletionProcessor
//!                                                    │
//!                                      ┌─────────────┼─────────────┐
//!                                      ▼             ▼             ▼
//!                                 Completed       Retry       DeadLetter
//! ```
//!
//! All tests use an in-memory SQLite store and the full axum router — no real
//! TCP sockets needed.

use std::sync::Arc;

use axum::{body::Body, http::Request};
use chrono::{Duration as ChronoDuration, Utc};
use croniq_runner::AppState;
use croniq_scheduler::trigger::TriggerState;
use croniq_server::{
    CompletionProcessor,
    api::{ServerState, server_router},
    loader::load_str,
    scheduler::SchedulerLoop,
    store::{DynStore, sqlite_store},
};
use croniq_store::{models::ExecutionState, sqlite::SqliteStore};
use http_body_util::BodyExt;
use std::time::Duration;
use tokio::sync::mpsc;
use tower::util::ServiceExt;

// ─── Test harness ─────────────────────────────────────────────────────────────

/// A fully wired in-memory Croniq server for integration testing.
struct TestServer {
    pub state: Arc<ServerState>,
    pub scheduler: SchedulerLoop,
    pub processor: Arc<CompletionProcessor>,
    pub store: DynStore,
    completion_rx: mpsc::UnboundedReceiver<croniq_server::CompletionEvent>,
}

impl TestServer {
    /// Create a test server from a Croniqfile source string.
    ///
    /// All triggers are forced to be immediately due so tests don't need to
    /// wait for real time to pass.
    fn from_config(src: &str) -> Self {
        let loaded = load_str(src).unwrap();
        let store: DynStore = sqlite_store(SqliteStore::in_memory().unwrap());
        let runner = AppState::new();
        let (tx, rx) = mpsc::unbounded_channel();

        // Force all non-disabled triggers to fire on the next tick.
        // Paused/Exhausted triggers are left alone — they reflect the DSL intent.
        let mut triggers = loaded.triggers;
        for trigger in triggers.values_mut() {
            if trigger.state == TriggerState::Armed {
                trigger.next_fire_at = Some(Utc::now() - ChronoDuration::seconds(5));
            }
        }

        let jobs = loaded.runtime.jobs.clone();
        let scheduler = SchedulerLoop::new(
            triggers,
            jobs.clone(),
            Arc::clone(&store),
            Arc::clone(&runner),
        );
        let processor = Arc::new(CompletionProcessor::new(
            jobs,
            Arc::clone(&store),
            Arc::clone(&runner),
        ));
        // Use a short long-poll timeout so tests don't block for 30 seconds
        // when the queue happens to be empty at poll time.
        let state = ServerState::with_timeout(runner, tx, Duration::from_millis(50));

        Self {
            state,
            scheduler,
            processor,
            store,
            completion_rx: rx,
        }
    }

    /// Advance the scheduler by one tick.
    async fn tick(&mut self) {
        self.scheduler.tick(Utc::now()).await;
    }

    /// Drain and process all pending completion events.
    async fn process_completions(&mut self) {
        while let Ok(event) = self.completion_rx.try_recv() {
            self.processor.process(event).await;
        }
    }

    /// Returns a fresh clone of the axum router (needed since `oneshot` consumes it).
    fn router(&self) -> axum::Router {
        server_router(Arc::clone(&self.state))
    }
}

// ─── HTTP helpers ─────────────────────────────────────────────────────────────

async fn post_json(app: axum::Router, uri: &str, body: serde_json::Value) -> serde_json::Value {
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

async fn get_json(app: axum::Router, uri: &str) -> serde_json::Value {
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

// ─── Tests ────────────────────────────────────────────────────────────────────

/// Happy path: scheduler fires job → runner polls → runner completes (success)
/// → execution is marked Completed in the store.
#[tokio::test]
async fn happy_path_schedule_poll_complete() {
    let mut srv = TestServer::from_config(
        r#"
        job billing:invoice {
            every 15 minutes
            timeout 10m
        }
    "#,
    );

    // 1. Tick: scheduler enqueues the job
    srv.tick().await;

    // 2. Runner polls and receives work
    let poll_resp = post_json(
        srv.router(),
        "/v1/poll",
        serde_json::json!({
            "runner_id": "worker-1",
            "capabilities": [],
            "max_inflight": 3,
            "inflight": []
        }),
    )
    .await;

    let work = poll_resp["work"].as_array().unwrap();
    assert_eq!(work.len(), 1, "runner should receive exactly one work item");
    let exec_id = work[0]["execution_id"].as_str().unwrap();
    let attempt = work[0]["attempt"].as_u64().unwrap();
    assert_eq!(work[0]["job_key"], "billing:invoice");
    assert_eq!(attempt, 1, "first dispatch should be attempt 1");

    // 3. Runner completes successfully
    let complete_resp = post_json(
        srv.router(),
        "/v1/complete",
        serde_json::json!({
            "runner_id": "worker-1",
            "execution_id": exec_id,
            "status": "success",
            "duration_ms": 800,
            "attempt": attempt
        }),
    )
    .await;
    assert_eq!(complete_resp["received"], true);

    // 4. Process the completion event
    srv.process_completions().await;

    // 5. Verify execution is Completed in the store
    let exec_uuid = uuid::Uuid::parse_str(exec_id).unwrap();
    let exec = srv.store.get_execution(exec_uuid).unwrap().unwrap();
    assert_eq!(exec.state, ExecutionState::Completed);
    assert_eq!(exec.job_key, "billing:invoice");
}

/// Failure → retry → success: runner fails, a retry is enqueued, runner
/// claims and succeeds on the second attempt.
#[tokio::test]
async fn failure_then_retry_then_success() {
    let mut srv = TestServer::from_config(
        r#"
        job etl:sync {
            every 1 hours
            retry fixed { max_attempts 3; delay 1s; jitter 0.0 }
        }
    "#,
    );

    srv.tick().await;

    // Runner polls → gets work
    let poll1 = post_json(
        srv.router(),
        "/v1/poll",
        serde_json::json!({
            "runner_id": "worker-1",
            "capabilities": [],
            "max_inflight": 3,
            "inflight": []
        }),
    )
    .await;
    let exec_id_1 = poll1["work"][0]["execution_id"]
        .as_str()
        .unwrap()
        .to_string();

    // Runner fails on attempt 1
    post_json(
        srv.router(),
        "/v1/complete",
        serde_json::json!({
            "runner_id": "worker-1",
            "execution_id": exec_id_1,
            "status": "failure",
            "error": "Connection refused",
            "duration_ms": 100,
            "attempt": 1
        }),
    )
    .await;

    // Process: should enqueue a retry
    srv.process_completions().await;

    // Attempt 1 is Failed in store
    let uuid1 = uuid::Uuid::parse_str(&exec_id_1).unwrap();
    let exec1 = srv.store.get_execution(uuid1).unwrap().unwrap();
    assert_eq!(exec1.state, ExecutionState::Failed);

    // Runner polls again → gets retry
    let poll2 = post_json(
        srv.router(),
        "/v1/poll",
        serde_json::json!({
            "runner_id": "worker-1",
            "capabilities": [],
            "max_inflight": 3,
            "inflight": []
        }),
    )
    .await;

    let work2 = poll2["work"].as_array().unwrap();
    assert_eq!(work2.len(), 1, "retry should be dispatched");
    let exec_id_2 = work2[0]["execution_id"].as_str().unwrap().to_string();
    let attempt2 = work2[0]["attempt"].as_u64().unwrap();
    assert_ne!(exec_id_2, exec_id_1, "retry should have a new execution ID");
    assert_eq!(attempt2, 2, "retry should be attempt 2");

    // Runner succeeds on attempt 2
    post_json(
        srv.router(),
        "/v1/complete",
        serde_json::json!({
            "runner_id": "worker-1",
            "execution_id": exec_id_2,
            "status": "success",
            "duration_ms": 500,
            "attempt": attempt2
        }),
    )
    .await;
    srv.process_completions().await;

    let uuid2 = uuid::Uuid::parse_str(&exec_id_2).unwrap();
    let exec2 = srv.store.get_execution(uuid2).unwrap().unwrap();
    assert_eq!(exec2.state, ExecutionState::Completed);
}

/// After exhausting all retry attempts, the execution is dead-lettered.
#[tokio::test]
async fn exhausted_retries_dead_lettered() {
    let mut srv = TestServer::from_config(
        r#"
        job reports:weekly {
            every 1 hours
            retry fixed { max_attempts 1; delay 1s; jitter 0.0 }
            dead_letter { retention 7d }
        }
    "#,
    );

    srv.tick().await;

    // Poll → get work
    let poll = post_json(
        srv.router(),
        "/v1/poll",
        serde_json::json!({
            "runner_id": "worker-1",
            "capabilities": [],
            "max_inflight": 3,
            "inflight": []
        }),
    )
    .await;
    let exec_id = poll["work"][0]["execution_id"]
        .as_str()
        .unwrap()
        .to_string();

    // Fail on the only attempt
    post_json(
        srv.router(),
        "/v1/complete",
        serde_json::json!({
            "runner_id": "worker-1",
            "execution_id": exec_id,
            "status": "failure",
            "error": "Permanent DB error",
            "duration_ms": 50
        }),
    )
    .await;
    srv.process_completions().await;

    // Execution → Dead
    let uuid = uuid::Uuid::parse_str(&exec_id).unwrap();
    let exec = srv.store.get_execution(uuid).unwrap().unwrap();
    assert_eq!(exec.state, ExecutionState::Dead);

    // Dead-letter record created
    let dls = srv
        .store
        .list_dead_letters(&croniq_store::models::DeadLetterFilter {
            job_key: Some("reports:weekly".into()),
            limit: None,
        })
        .unwrap();
    assert_eq!(dls.len(), 1);
    assert_eq!(dls[0].job_key, "reports:weekly");
}

/// `POST /v1/dead-letters/bulk-delete { all: true }` clears the queue
/// end-to-end through the real router → handler → store (issue #348).
/// Also proves the new static route coexists with `/v1/dead-letters/{id}`
/// without a matchit conflict at router construction.
#[tokio::test]
async fn bulk_delete_all_clears_dead_letter_queue() {
    // `store_backed_router` wires ServerState.store (the shared TestServer
    // harness leaves it None), which the endpoint reads.
    let store: DynStore = sqlite_store(SqliteStore::in_memory().unwrap());

    // Seed two dead letters.
    for _ in 0..2 {
        store
            .add_dead_letter(&croniq_store::models::DeadLetter {
                id: uuid::Uuid::new_v4(),
                execution_id: uuid::Uuid::new_v4(),
                job_key: "reports:weekly".into(),
                fire_at: Utc::now(),
                attempt: 1,
                error: "boom".into(),
                dead_reason: "timeout".into(),
                metadata: Default::default(),
                created_at: Utc::now(),
                expires_at: None,
            })
            .unwrap();
    }

    let resp = post_json(
        store_backed_router(Arc::clone(&store)),
        "/v1/dead-letters/bulk-delete",
        serde_json::json!({ "all": true }),
    )
    .await;
    assert_eq!(resp["deleted"].as_u64(), Some(2), "resp: {resp}");

    assert!(
        store
            .list_dead_letters(&croniq_store::models::DeadLetterFilter::default())
            .unwrap()
            .is_empty(),
        "bulk-delete all should empty the queue"
    );
}

/// Capability routing: a job requiring a capability is only dispatched to
/// runners that possess it.
#[tokio::test]
async fn capability_routing_respects_requirements() {
    let mut srv = TestServer::from_config(
        r#"
        job billing:invoice {
            every 1 hours
            runner { require billing }
        }
    "#,
    );

    srv.tick().await;

    // ETL runner polls but lacks "billing" capability
    let poll_etl = post_json(
        srv.router(),
        "/v1/poll",
        serde_json::json!({
            "runner_id": "etl-worker",
            "capabilities": ["etl"],
            "max_inflight": 3,
            "inflight": []
        }),
    )
    .await;
    assert_eq!(
        poll_etl["work"].as_array().unwrap().len(),
        0,
        "ETL worker should not receive billing work"
    );

    // Item still in queue
    let queue = srv.state.runner.queue.read().await;
    assert_eq!(queue.len(), 1);
    drop(queue);

    // Billing runner polls and receives the work
    let poll_billing = post_json(
        srv.router(),
        "/v1/poll",
        serde_json::json!({
            "runner_id": "billing-worker",
            "capabilities": ["billing"],
            "max_inflight": 3,
            "inflight": []
        }),
    )
    .await;
    let work = poll_billing["work"].as_array().unwrap();
    assert_eq!(work.len(), 1);
    assert_eq!(work[0]["job_key"], "billing:invoice");
}

/// Multiple jobs all fire on tick; runner with sufficient capacity gets them all.
#[tokio::test]
async fn multiple_jobs_dispatched_in_one_poll() {
    let mut srv = TestServer::from_config(
        r#"
        job billing:invoice  { every 1 hours }
        job etl:sync         { every 1 hours }
        job reports:weekly   { every 1 hours }
    "#,
    );

    srv.tick().await;

    // Runner with capacity 5 polls → gets all 3 items
    let poll = post_json(
        srv.router(),
        "/v1/poll",
        serde_json::json!({
            "runner_id": "worker-1",
            "capabilities": [],
            "max_inflight": 5,
            "inflight": []
        }),
    )
    .await;

    let work = poll["work"].as_array().unwrap();
    assert_eq!(work.len(), 3, "runner should receive all 3 work items");

    let job_keys: Vec<&str> = work
        .iter()
        .map(|w| w["job_key"].as_str().unwrap())
        .collect();
    assert!(job_keys.contains(&"billing:invoice"));
    assert!(job_keys.contains(&"etl:sync"));
    assert!(job_keys.contains(&"reports:weekly"));
}

/// Health endpoint reflects live queue depth and runner counts.
#[tokio::test]
async fn health_reflects_queue_depth_after_tick() {
    let mut srv = TestServer::from_config(
        r#"
        job billing:invoice { every 1 hours }
        job etl:sync        { every 1 hours }
    "#,
    );

    // Before tick: empty queue
    let health_before = get_json(srv.router(), "/health").await;
    assert_eq!(health_before["queued"], 0);

    // After tick: two items queued
    srv.tick().await;

    let health_after = get_json(srv.router(), "/health").await;
    assert_eq!(health_after["queued"], 2);
    assert_eq!(health_after["status"], "ok");
}

/// Completing a job releases it from the runner's inflight list.
#[tokio::test]
async fn complete_releases_runner_inflight() {
    let mut srv = TestServer::from_config(
        r#"
        job etl:sync { every 1 hours }
    "#,
    );

    srv.tick().await;

    // Poll → claim work
    let poll = post_json(
        srv.router(),
        "/v1/poll",
        serde_json::json!({
            "runner_id": "worker-1",
            "capabilities": [],
            "max_inflight": 3,
            "inflight": []
        }),
    )
    .await;
    let exec_id = poll["work"][0]["execution_id"]
        .as_str()
        .unwrap()
        .to_string();

    // Verify inflight before completion
    {
        let reg = srv.state.runner.registry.read().await;
        let runner = reg.get("worker-1").unwrap();
        assert_eq!(runner.inflight.len(), 1);
    }

    // Complete
    post_json(
        srv.router(),
        "/v1/complete",
        serde_json::json!({
            "runner_id": "worker-1",
            "execution_id": exec_id,
            "status": "success",
            "duration_ms": 200
        }),
    )
    .await;

    // Inflight cleared
    let reg = srv.state.runner.registry.read().await;
    let runner = reg.get("worker-1").unwrap();
    assert!(runner.inflight.is_empty());
}

/// Max-inflight is respected: runner with capacity 1 and 1 inflight gets 0 more.
#[tokio::test]
async fn max_inflight_limits_dispatch() {
    let mut srv = TestServer::from_config(
        r#"
        job job:a { every 1 hours }
        job job:b { every 1 hours }
    "#,
    );

    srv.tick().await; // Both enqueued

    // Runner polls with max_inflight=2 and 1 already inflight → gets 1
    let poll = post_json(
        srv.router(),
        "/v1/poll",
        serde_json::json!({
            "runner_id": "worker-1",
            "capabilities": [],
            "max_inflight": 2,
            "inflight": ["exec-existing"]
        }),
    )
    .await;

    let work = poll["work"].as_array().unwrap();
    assert_eq!(work.len(), 1, "only 1 slot available");

    // Second item still in queue
    let queue = srv.state.runner.queue.read().await;
    assert_eq!(queue.len(), 1);
}

/// Disabled job should not appear in the queue after a tick.
#[tokio::test]
async fn disabled_job_never_fires() {
    let mut srv = TestServer::from_config(
        r#"
        job reports:never { disabled }
    "#,
    );

    srv.tick().await;

    let queue = srv.state.runner.queue.read().await;
    assert!(queue.is_empty(), "disabled job should never be enqueued");
}

/// Full lifecycle: scheduler creates execution, runner completes, store is
/// consistent throughout.
#[tokio::test]
async fn store_state_consistent_through_lifecycle() {
    let mut srv = TestServer::from_config(
        r#"
        job billing:invoice {
            every 1 hours
            timeout 5m
            retry fixed { max_attempts 2; delay 1s; jitter 0.0 }
        }
    "#,
    );

    // Initially no executions
    let counts_before = srv.store.count_by_state().unwrap();
    let total_before: u64 = counts_before.values().sum();
    assert_eq!(total_before, 0);

    // Tick → 1 queued
    srv.tick().await;

    let counts_after_tick = srv.store.count_by_state().unwrap();
    assert_eq!(
        counts_after_tick
            .get(&ExecutionState::Queued)
            .copied()
            .unwrap_or(0),
        1
    );

    // Runner claims it
    let poll = post_json(
        srv.router(),
        "/v1/poll",
        serde_json::json!({
            "runner_id": "worker-1",
            "capabilities": [],
            "max_inflight": 3,
            "inflight": []
        }),
    )
    .await;
    let exec_id = poll["work"][0]["execution_id"]
        .as_str()
        .unwrap()
        .to_string();

    // Complete (success)
    post_json(
        srv.router(),
        "/v1/complete",
        serde_json::json!({
            "runner_id": "worker-1",
            "execution_id": exec_id,
            "status": "success",
            "duration_ms": 1500
        }),
    )
    .await;
    srv.process_completions().await;

    // 1 completed, 0 queued
    let counts_final = srv.store.count_by_state().unwrap();
    assert_eq!(
        counts_final
            .get(&ExecutionState::Completed)
            .copied()
            .unwrap_or(0),
        1
    );
    assert_eq!(
        counts_final
            .get(&ExecutionState::Queued)
            .copied()
            .unwrap_or(0),
        0
    );
}

/// Build a store-backed server router for endpoint tests. The shared
/// `TestServer` harness leaves `ServerState.store = None` (it drives the
/// scheduler/processor directly), so HTTP endpoints that read the store need
/// their own wiring.
fn store_backed_router(store: DynStore) -> axum::Router {
    let runner = AppState::new();
    let (tx, _rx) = mpsc::unbounded_channel();
    let state = ServerState::with_auth(runner, tx, None, Some(store));
    server_router(state)
}

/// `GET /v1/jobs/states` flags an active job whose next fire slipped into
/// the past as `overdue` (issue #250 dashboard surface). Also proves the
/// static route wins over `/v1/jobs/{job_key}`.
#[tokio::test]
async fn job_states_endpoint_flags_overdue() {
    use croniq_store::models::{JobState, JobStatus};

    let store: DynStore = sqlite_store(SqliteStore::in_memory().unwrap());
    let now = Utc::now();

    // Active trigger whose next fire is an hour overdue → the scheduler
    // never advanced it.
    store
        .upsert_job_state(&JobState {
            job_key: "billing:backup".into(),
            next_fire_at: Some(now - ChronoDuration::hours(1)),
            last_fired_at: Some(now - ChronoDuration::days(1)),
            fire_count: 4,
            status: JobStatus::Active,
            updated_at: now,
        })
        .unwrap();

    let body = get_json(store_backed_router(store), "/v1/jobs/states").await;
    let arr = body.as_array().expect("array response");
    let s = arr
        .iter()
        .find(|x| x["job_key"] == "billing:backup")
        .expect("billing:backup present");

    assert_eq!(s["overdue"], true);
    assert_eq!(s["status"], "active");
    assert!(s["next_fire_at"].is_string());
    assert!(s["last_fired_at"].is_string());
}

/// A healthy job (next fire in the future) is not flagged overdue.
#[tokio::test]
async fn job_states_endpoint_healthy_not_overdue() {
    use croniq_store::models::{JobState, JobStatus};

    let store: DynStore = sqlite_store(SqliteStore::in_memory().unwrap());
    let now = Utc::now();
    store
        .upsert_job_state(&JobState {
            job_key: "etl:sync".into(),
            next_fire_at: Some(now + ChronoDuration::hours(1)),
            last_fired_at: Some(now - ChronoDuration::minutes(5)),
            fire_count: 1,
            status: JobStatus::Active,
            updated_at: now,
        })
        .unwrap();

    let body = get_json(store_backed_router(store), "/v1/jobs/states").await;
    let s = body
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["job_key"] == "etl:sync")
        .unwrap()
        .clone();
    assert_eq!(s["overdue"], false);
}

//! End-to-end tests for SIGHUP / admin-endpoint config reload.
//!
//! These tests wire up the pieces that `main.rs` wires up in production:
//! a live scheduler task receiving commands, a shared trigger snapshot,
//! shared DSL job state, and a full axum router. They verify:
//!
//! - Admin HTTP endpoint: dry-run, apply, auth, validation errors, diff.
//! - SIGHUP-equivalent path (sending a path to `reload_rx`): triggers the
//!   same reconcile via `reload::build_plan` + `apply_plan_direct`.
//! - API-registered triggers survive DSL reload.
//! - Lease-active executions are unaffected by reload.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::{body::Body, http::Request};
use chrono::Utc;
use croniq_auth::CallerType;
use croniq_auth::jwt::{JwtConfig, issue_token_pair};
use croniq_runner::AppState;
use croniq_scheduler::trigger::Trigger;
use croniq_server::{
    SchedulerLoop,
    api::{ServerState, server_router},
    loader::load_str,
    reload,
    scheduler::SchedulerCommand,
    store::{DynStore, sqlite_store},
};
use croniq_store::sqlite::SqliteStore;
use http_body_util::BodyExt;
use tempfile::TempDir;
use tokio::sync::{RwLock, mpsc};
use tower::util::ServiceExt;

// ─── Harness ──────────────────────────────────────────────────────────────────

const TEST_JWT_SECRET: &str = "reload-test-secret";

struct Harness {
    state: Arc<ServerState>,
    config_path: PathBuf,
    reload_tx: mpsc::UnboundedSender<PathBuf>,
    store: DynStore,
    trigger_snapshot: Arc<RwLock<HashMap<String, Trigger>>>,
    dsl_jobs_shared: Arc<RwLock<Vec<croniq_config::compile::JobConfig>>>,
    #[allow(dead_code)] // exposed for test harness symmetry; new tests will exercise it
    dsl_calendars_shared: Arc<RwLock<Vec<croniq_config::compile::CalendarConfig>>>,
    _tmp: TempDir,
    _scheduler_task: tokio::task::JoinHandle<()>,
}

impl Harness {
    async fn new(initial_config: &str) -> Self {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("Croniqfile");
        std::fs::write(&config_path, initial_config).unwrap();

        let loaded = load_str(initial_config).unwrap();
        let store: DynStore = sqlite_store(SqliteStore::in_memory().unwrap());
        let runner = AppState::new();

        let (completion_tx, mut completion_rx) = mpsc::unbounded_channel();
        // Drain completion events silently so tests don't block.
        tokio::spawn(async move { while completion_rx.recv().await.is_some() {} });

        let (scheduler_cmd_tx, mut scheduler_cmd_rx) =
            mpsc::unbounded_channel::<SchedulerCommand>();
        let (reload_tx, mut reload_rx) = mpsc::unbounded_channel::<PathBuf>();

        let dsl_jobs_shared = Arc::new(RwLock::new(loaded.runtime.jobs.clone()));
        let dsl_calendars_shared = Arc::new(RwLock::new(loaded.runtime.calendars.clone()));
        let trigger_snapshot = Arc::new(RwLock::new(loaded.triggers.clone()));

        let mut state = ServerState::with_timeout(
            Arc::clone(&runner),
            completion_tx,
            Duration::from_millis(50),
        );
        {
            let s = Arc::get_mut(&mut state).unwrap();
            s.store = Some(Arc::clone(&store));
            s.scheduler_tx = Some(scheduler_cmd_tx);
            s.dsl_jobs = Some(Arc::clone(&dsl_jobs_shared));
            s.dsl_calendars = Some(Arc::clone(&dsl_calendars_shared));
            s.triggers = Some(Arc::clone(&trigger_snapshot));
            s.config_path = Some(config_path.clone());
            s.jwt_config = Some(JwtConfig {
                secret: TEST_JWT_SECRET.into(),
                ..Default::default()
            });
        }

        let mut scheduler_loop = SchedulerLoop::new(
            loaded.triggers,
            loaded.runtime.jobs,
            Arc::clone(&store),
            Arc::clone(&runner),
        );

        let task_store = Arc::clone(&store);
        let task_snapshot = Arc::clone(&trigger_snapshot);
        let task_dsl = Arc::clone(&dsl_jobs_shared);
        let task_dsl_cals = Arc::clone(&dsl_calendars_shared);
        let task_policy = Arc::clone(&state.policy_dsl_adopt_on_mutate);
        let task_counters = Arc::clone(&state.reload_counters);

        let _scheduler_task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(cmd) = scheduler_cmd_rx.recv() => {
                        scheduler_loop.apply_command(cmd);
                    }
                    Some(path) = reload_rx.recv() => {
                        match reload::build_plan(&path, &task_store, &task_snapshot, &task_dsl).await {
                            Ok(plan) => {
                                reload::apply_plan_direct(
                                    plan,
                                    &mut scheduler_loop,
                                    &task_dsl,
                                    &task_dsl_cals,
                                    &task_policy,
                                    &task_snapshot,
                                ).await;
                                task_counters.inc_success();
                            }
                            Err(_) => {
                                task_counters.inc_validation_error();
                            }
                        }
                    }
                    else => break,
                }
            }
        });

        Self {
            state,
            config_path,
            reload_tx,
            store,
            trigger_snapshot,
            dsl_jobs_shared,
            dsl_calendars_shared,
            _tmp: tmp,
            _scheduler_task,
        }
    }

    fn admin_token(&self) -> String {
        let cfg = self.state.jwt_config.as_ref().unwrap();
        let pair = issue_token_pair(
            cfg,
            "admin-user",
            "admin-client",
            CallerType::User,
            Some("admin-user"),
            Some(croniq_auth::Role::Admin),
            croniq_auth::AuthMethod::Password,
            &["admin".into()],
        )
        .unwrap();
        pair.access_token
    }

    fn user_token(&self, scopes: &[&str]) -> String {
        let cfg = self.state.jwt_config.as_ref().unwrap();
        let scopes: Vec<String> = scopes.iter().map(|s| (*s).into()).collect();
        let pair = issue_token_pair(
            cfg,
            "test-user",
            "test-client",
            CallerType::User,
            Some("test-user"),
            Some(croniq_auth::Role::Operator),
            croniq_auth::AuthMethod::Password,
            &scopes,
        )
        .unwrap();
        pair.access_token
    }

    fn router(&self) -> axum::Router {
        server_router(Arc::clone(&self.state))
    }

    fn rewrite_config(&self, new_src: &str) {
        std::fs::write(&self.config_path, new_src).unwrap();
    }
}

// ─── HTTP helpers ─────────────────────────────────────────────────────────────

struct Response {
    status: u16,
    body: serde_json::Value,
}

async fn admin_post(app: axum::Router, uri: &str, token: Option<&str>) -> Response {
    let mut builder = Request::builder().method("POST").uri(uri);
    if let Some(t) = token {
        builder = builder.header("authorization", format!("Bearer {t}"));
    }
    let resp = app
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status().as_u16();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    Response { status, body }
}

// ─── Tests: admin endpoint ────────────────────────────────────────────────────

#[tokio::test]
async fn reload_requires_auth() {
    let h = Harness::new("job a:one { every 1 hours }").await;
    let resp = admin_post(h.router(), "/v1/admin/reload-config", None).await;
    assert_eq!(resp.status, 401);
}

#[tokio::test]
async fn reload_requires_admin_scope() {
    let h = Harness::new("job a:one { every 1 hours }").await;
    // A token with only jobs:read — admin should reject.
    let token = h.user_token(&["jobs:read"]);
    let resp = admin_post(h.router(), "/v1/admin/reload-config", Some(&token)).await;
    assert_eq!(resp.status, 403);
}

#[tokio::test]
async fn reload_dry_run_returns_diff_without_applying() {
    let h = Harness::new("job a:one { every 1 hours }").await;
    h.rewrite_config("job b:two { every 1 hours }");

    let token = h.admin_token();
    let resp = admin_post(
        h.router(),
        "/v1/admin/reload-config?dry_run=true",
        Some(&token),
    )
    .await;

    assert_eq!(resp.status, 200);
    assert_eq!(resp.body["applied"], false);
    assert_eq!(resp.body["dry_run"], true);
    assert_eq!(resp.body["diff"]["added"], serde_json::json!(["b:two"]));
    assert_eq!(resp.body["diff"]["removed"], serde_json::json!(["a:one"]));

    // State unchanged: snapshot still has the old job.
    let snap = h.trigger_snapshot.read().await;
    assert!(snap.contains_key("a:one"));
    assert!(!snap.contains_key("b:two"));
}

#[tokio::test]
async fn reload_apply_swaps_running_config() {
    let h = Harness::new("job a:one { every 1 hours }").await;
    h.rewrite_config("job b:two { every 1 hours }");

    let token = h.admin_token();
    let resp = admin_post(h.router(), "/v1/admin/reload-config", Some(&token)).await;

    assert_eq!(resp.status, 200);
    assert_eq!(resp.body["applied"], true);
    assert_eq!(resp.body["dry_run"], false);

    // State swapped.
    let snap = h.trigger_snapshot.read().await;
    assert!(!snap.contains_key("a:one"));
    assert!(snap.contains_key("b:two"));

    let dsl = h.dsl_jobs_shared.read().await;
    let keys: Vec<&str> = dsl.iter().map(|j| j.key.as_str()).collect();
    assert_eq!(keys, vec!["b:two"]);
}

#[tokio::test]
async fn reload_invalid_file_returns_422_with_line_col() {
    let h = Harness::new("job a:one { every 1 hours }").await;
    // Junk on line 2 → parser error with a span on that line.
    h.rewrite_config("\n@@@garbage@@@\n");

    let token = h.admin_token();
    let resp = admin_post(h.router(), "/v1/admin/reload-config", Some(&token)).await;

    assert_eq!(resp.status, 422);
    assert_eq!(resp.body["error"], "validation_error");
    assert!(
        resp.body["line"].is_u64(),
        "line field should be populated: body={}",
        resp.body
    );
    assert!(
        resp.body["column"].is_u64(),
        "column field should be populated: body={}",
        resp.body
    );

    // Scheduler state unchanged.
    let snap = h.trigger_snapshot.read().await;
    assert!(snap.contains_key("a:one"));
}

#[tokio::test]
async fn reload_invalid_file_does_not_increment_success_counter() {
    use std::sync::atomic::Ordering;

    let h = Harness::new("job a:one { every 1 hours }").await;
    h.rewrite_config("@@@garbage@@@");
    let token = h.admin_token();

    let before = h
        .state
        .reload_counters
        .validation_error
        .load(Ordering::Relaxed);
    let _ = admin_post(h.router(), "/v1/admin/reload-config", Some(&token)).await;
    let after = h
        .state
        .reload_counters
        .validation_error
        .load(Ordering::Relaxed);

    assert_eq!(after, before + 1, "validation_error counter should advance");
    assert_eq!(h.state.reload_counters.success.load(Ordering::Relaxed), 0);
}

// ─── Tests: API-registered jobs survive reload ────────────────────────────────

#[tokio::test]
async fn reload_preserves_api_registered_triggers() {
    let h = Harness::new("job dsl:original { every 1 hours }").await;

    // Seed an API-registered trigger in the store (bypassing the API since
    // we don't need the full registration flow — we just need a row).
    h.store
        .create_trigger(&croniq_store::models::TriggerDefinition {
            trigger_id: "api-1".into(),
            job_key: "api:survivor".into(),
            cron_expression: Some("5m".into()),
            timezone: None,
            calendar: None,
            window: None,
            not_before: None,
            not_after: None,
            enabled: true,
            managed_by: "api".into(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        })
        .unwrap();

    // Swap the DSL config — the API trigger should survive.
    h.rewrite_config("job dsl:replacement { every 2 hours }");
    let token = h.admin_token();
    let resp = admin_post(h.router(), "/v1/admin/reload-config", Some(&token)).await;
    assert_eq!(resp.status, 200);

    let snap = h.trigger_snapshot.read().await;
    assert!(
        snap.contains_key("api:survivor"),
        "API-registered job must not be dropped by DSL reload"
    );
    assert!(snap.contains_key("dsl:replacement"));
    assert!(!snap.contains_key("dsl:original"));
}

#[tokio::test]
async fn reload_dsl_wins_on_job_key_conflict_with_api_trigger() {
    let h = Harness::new("job conflict:key { every 1 hours }").await;

    h.store
        .create_trigger(&croniq_store::models::TriggerDefinition {
            trigger_id: "api-conflict".into(),
            job_key: "conflict:key".into(),
            cron_expression: Some("5m".into()),
            timezone: None,
            calendar: None,
            window: None,
            not_before: None,
            not_after: None,
            enabled: true,
            managed_by: "api".into(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        })
        .unwrap();

    h.rewrite_config("job conflict:key { every 30 minutes }");
    let token = h.admin_token();
    let _ = admin_post(h.router(), "/v1/admin/reload-config", Some(&token)).await;

    let dsl = h.dsl_jobs_shared.read().await;
    let job = dsl.iter().find(|j| j.key == "conflict:key").unwrap();
    assert_eq!(
        job.schedule_summary, "every 30 minutes",
        "DSL schedule must take precedence over API trigger"
    );
}

// ─── Tests: SIGHUP-equivalent path ────────────────────────────────────────────

#[tokio::test]
async fn sighup_path_reloads_via_reload_channel() {
    use std::sync::atomic::Ordering;

    let h = Harness::new("job a:one { every 1 hours }").await;
    h.rewrite_config("job b:two { every 1 hours }");

    // Send the path to reload_tx — this is what the SIGHUP handler does.
    h.reload_tx.send(h.config_path.clone()).unwrap();

    // Wait briefly for the scheduler task to process the reload.
    for _ in 0..50 {
        if h.state.reload_counters.success.load(Ordering::Relaxed) >= 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    assert_eq!(
        h.state.reload_counters.success.load(Ordering::Relaxed),
        1,
        "scheduler task should have applied the reload once"
    );

    let snap = h.trigger_snapshot.read().await;
    assert!(snap.contains_key("b:two"));
    assert!(!snap.contains_key("a:one"));
}

#[tokio::test]
async fn sighup_path_with_invalid_file_keeps_state_and_increments_validation() {
    use std::sync::atomic::Ordering;

    let h = Harness::new("job a:one { every 1 hours }").await;
    h.rewrite_config("\n@@@garbage@@@");

    h.reload_tx.send(h.config_path.clone()).unwrap();

    for _ in 0..50 {
        if h.state
            .reload_counters
            .validation_error
            .load(Ordering::Relaxed)
            >= 1
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    assert_eq!(
        h.state
            .reload_counters
            .validation_error
            .load(Ordering::Relaxed),
        1
    );

    // Old state preserved.
    let snap = h.trigger_snapshot.read().await;
    assert!(snap.contains_key("a:one"));
}

// ─── Tests: lease-active executions unaffected ────────────────────────────────

#[tokio::test]
async fn reload_does_not_touch_claimed_executions() {
    use croniq_store::models::{Execution, ExecutionState};
    use uuid::Uuid;

    let h = Harness::new("job billing:invoice { every 1 hours }").await;

    // Seed a claimed execution directly in the store.
    let exec_id = Uuid::new_v4();
    let now = Utc::now();
    h.store
        .create_execution(&Execution {
            id: exec_id,
            job_key: "billing:invoice".into(),
            fire_at: now,
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
            metadata: Default::default(),
            created_at: now,
        })
        .unwrap();
    h.store
        .claim_execution(exec_id, "worker-alive", now)
        .unwrap();

    // Apply a reload (same config, so no-op diff but the apply path still runs).
    h.rewrite_config("job billing:invoice { every 1 hours }");
    let token = h.admin_token();
    let resp = admin_post(h.router(), "/v1/admin/reload-config", Some(&token)).await;
    assert_eq!(resp.status, 200);

    // The claimed execution is untouched.
    let exec = h.store.get_execution(exec_id).unwrap().unwrap();
    assert_eq!(exec.state, ExecutionState::Claimed);
    assert_eq!(exec.runner_id.as_deref(), Some("worker-alive"));
}

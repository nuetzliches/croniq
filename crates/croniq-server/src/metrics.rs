//! Prometheus-compatible metrics endpoint.
//!
//! Exposes key runtime metrics in Prometheus text exposition format at a
//! configurable endpoint (default: `/metrics`).

use std::sync::Arc;

use axum::{
    Router,
    extract::State,
    http::{StatusCode, header},
    response::IntoResponse,
    routing::get,
};

use crate::api::ServerState;
use croniq_runner::RunnerStatus;
use croniq_store::models::{
    JOB_DURATION_BUCKETS_SECONDS, JobExecutionMetrics, JobState, JobStatus,
};

/// Create a router for the metrics endpoint.
pub fn metrics_router(state: Arc<ServerState>) -> Router {
    Router::new()
        .route("/metrics", get(handle_metrics))
        .with_state(state)
}

async fn handle_metrics(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
    use std::sync::atomic::Ordering;

    let now = chrono::Utc::now();
    let reg = state.runner.registry.read().await;
    let queue = state.runner.queue.read().await;

    let runners_online = reg.by_status(RunnerStatus::Online, now).len();
    let runners_stale = reg.by_status(RunnerStatus::Stale, now).len();
    let runners_dead = reg.by_status(RunnerStatus::Dead, now).len();
    let queue_depth = queue.len();

    let reload_success = state.reload_counters.success.load(Ordering::Relaxed);
    let reload_validation_err = state
        .reload_counters
        .validation_error
        .load(Ordering::Relaxed);
    let reload_apply_err = state.reload_counters.apply_error.load(Ordering::Relaxed);

    // Jobs paused because a referenced calendar did not resolve (issue #361).
    // A non-zero value means jobs are fail-closed and not firing on schedule.
    let calendar_faults = state.config_faults.read().map(|f| f.len()).unwrap_or(0);

    let mut body = format!(
        "# HELP croniq_runners_total Number of known runners by status.\n\
         # TYPE croniq_runners_total gauge\n\
         croniq_runners_total{{status=\"online\"}} {runners_online}\n\
         croniq_runners_total{{status=\"stale\"}} {runners_stale}\n\
         croniq_runners_total{{status=\"dead\"}} {runners_dead}\n\
         # HELP croniq_queue_depth Number of work items in the queue.\n\
         # TYPE croniq_queue_depth gauge\n\
         croniq_queue_depth {queue_depth}\n\
         # HELP croniq_config_reload_total Config reload attempts by outcome.\n\
         # TYPE croniq_config_reload_total counter\n\
         croniq_config_reload_total{{result=\"success\"}} {reload_success}\n\
         croniq_config_reload_total{{result=\"validation_error\"}} {reload_validation_err}\n\
         croniq_config_reload_total{{result=\"apply_error\"}} {reload_apply_err}\n\
         # HELP croniq_config_calendar_faults Jobs paused because a referenced calendar did not resolve.\n\
         # TYPE croniq_config_calendar_faults gauge\n\
         croniq_config_calendar_faults {calendar_faults}\n"
    );

    // Scheduler liveness (issue #248). The scheduler updates the heartbeat
    // after every successful tick; a stale `last_tick_timestamp` means the
    // scheduler task is wedged or dead even though this HTTP endpoint answered.
    // Emitted only when a scheduler is wired in (always so in production).
    if let Some(hb) = &state.scheduler_heartbeat {
        body.push_str(&format!(
            "# HELP croniq_scheduler_last_tick_timestamp Unix time (seconds) of the last successful scheduler tick.\n\
             # TYPE croniq_scheduler_last_tick_timestamp gauge\n\
             croniq_scheduler_last_tick_timestamp {}\n\
             # HELP croniq_scheduler_ticks_total Successful scheduler ticks since process start.\n\
             # TYPE croniq_scheduler_ticks_total counter\n\
             croniq_scheduler_ticks_total {}\n",
            hb.last_tick_unix(),
            hb.ticks_total(),
        ));
    }

    // Per-job series are derived from the executions store at scrape time
    // (one grouped query; nothing is persisted separately). Skipped when the
    // server runs without a store, or logged-and-skipped on query error so a
    // scrape never fails.
    if let Some(store) = &state.store {
        match store.job_execution_metrics() {
            Ok(jobs) => render_job_metrics(&mut body, &jobs),
            Err(e) => tracing::warn!(error = %e, "metrics: job_execution_metrics query failed"),
        }
        // Per-job scheduling liveness (issue #250): last/next fire and an
        // "overdue" flag derived from persisted job_states. These let
        // external monitoring alert on a job that silently stopped being
        // scheduled — `time() - croniq_job_last_fire_timestamp > period` or a
        // stuck `croniq_job_overdue == 1` — even when the in-process
        // scheduler is wedged (issue #248) and no run failed.
        match store.list_job_states() {
            Ok(states) => render_job_state_metrics(&mut body, &states, now),
            Err(e) => tracing::warn!(error = %e, "metrics: list_job_states query failed"),
        }
    }

    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
}

/// Append the per-job metric families to the exposition body. Each family
/// gets a single `# HELP`/`# TYPE` header followed by all of its job samples,
/// so the output stays valid Prometheus text regardless of job count.
fn render_job_metrics(out: &mut String, jobs: &[JobExecutionMetrics]) {
    if jobs.is_empty() {
        return;
    }

    out.push_str(
        "# HELP croniq_job_executions_total Terminal executions per job, by final state.\n\
         # TYPE croniq_job_executions_total counter\n",
    );
    for j in jobs {
        let key = escape_label(&j.job_key);
        out.push_str(&format!(
            "croniq_job_executions_total{{job_key=\"{key}\",state=\"completed\"}} {}\n",
            j.completed
        ));
        out.push_str(&format!(
            "croniq_job_executions_total{{job_key=\"{key}\",state=\"failed\"}} {}\n",
            j.failed
        ));
        out.push_str(&format!(
            "croniq_job_executions_total{{job_key=\"{key}\",state=\"dead\"}} {}\n",
            j.dead
        ));
        out.push_str(&format!(
            "croniq_job_executions_total{{job_key=\"{key}\",state=\"cancelled\"}} {}\n",
            j.cancelled
        ));
    }

    out.push_str(
        "# HELP croniq_job_duration_seconds Execution wall-clock duration per job, in seconds.\n\
         # TYPE croniq_job_duration_seconds histogram\n",
    );
    for j in jobs {
        let key = escape_label(&j.job_key);
        for (idx, bound) in JOB_DURATION_BUCKETS_SECONDS.iter().enumerate() {
            let count = j.duration_buckets.get(idx).copied().unwrap_or(0);
            out.push_str(&format!(
                "croniq_job_duration_seconds_bucket{{job_key=\"{key}\",le=\"{bound}\"}} {count}\n"
            ));
        }
        out.push_str(&format!(
            "croniq_job_duration_seconds_bucket{{job_key=\"{key}\",le=\"+Inf\"}} {}\n",
            j.duration_count
        ));
        out.push_str(&format!(
            "croniq_job_duration_seconds_sum{{job_key=\"{key}\"}} {}\n",
            j.duration_sum_ms as f64 / 1000.0
        ));
        out.push_str(&format!(
            "croniq_job_duration_seconds_count{{job_key=\"{key}\"}} {}\n",
            j.duration_count
        ));
    }

    out.push_str(
        "# HELP croniq_job_last_run_timestamp Unix time (seconds) of the most recent finished execution per job.\n\
         # TYPE croniq_job_last_run_timestamp gauge\n",
    );
    for j in jobs {
        if let Some(ts) = j.last_run_at {
            let key = escape_label(&j.job_key);
            out.push_str(&format!(
                "croniq_job_last_run_timestamp{{job_key=\"{key}\"}} {}\n",
                ts.timestamp()
            ));
        }
    }
}

/// Append the per-job scheduling-liveness families from `job_states`
/// (issue #250): last fire, next fire, and an overdue flag. Each family
/// gets one `# HELP`/`# TYPE` header followed by its samples.
fn render_job_state_metrics(
    out: &mut String,
    states: &[JobState],
    now: chrono::DateTime<chrono::Utc>,
) {
    if states.is_empty() {
        return;
    }

    out.push_str(
        "# HELP croniq_job_last_fire_timestamp Unix time (seconds) of the last scheduled fire per job.\n\
         # TYPE croniq_job_last_fire_timestamp gauge\n",
    );
    for s in states {
        if let Some(ts) = s.last_fired_at {
            let key = escape_label(&s.job_key);
            out.push_str(&format!(
                "croniq_job_last_fire_timestamp{{job_key=\"{key}\"}} {}\n",
                ts.timestamp()
            ));
        }
    }

    out.push_str(
        "# HELP croniq_job_next_fire_timestamp Unix time (seconds) of the next scheduled fire per job.\n\
         # TYPE croniq_job_next_fire_timestamp gauge\n",
    );
    for s in states {
        if let Some(ts) = s.next_fire_at {
            let key = escape_label(&s.job_key);
            out.push_str(&format!(
                "croniq_job_next_fire_timestamp{{job_key=\"{key}\"}} {}\n",
                ts.timestamp()
            ));
        }
    }

    out.push_str(
        "# HELP croniq_job_overdue Whether an active job's next scheduled fire is in the past (1) or not (0). A stuck 1 signals a stalled scheduler.\n\
         # TYPE croniq_job_overdue gauge\n",
    );
    for s in states {
        if s.status != JobStatus::Active {
            continue;
        }
        if let Some(ts) = s.next_fire_at {
            let key = escape_label(&s.job_key);
            let overdue = if ts < now { 1 } else { 0 };
            out.push_str(&format!(
                "croniq_job_overdue{{job_key=\"{key}\"}} {overdue}\n"
            ));
        }
    }
}

/// Escape a Prometheus label value per the text exposition format
/// (backslash, double-quote, newline).
fn escape_label(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use chrono::{TimeZone, Utc};
    use croniq_runner::AppState;
    use http_body_util::BodyExt;
    use tokio::sync::mpsc;
    use tower::util::ServiceExt;

    #[tokio::test]
    async fn metrics_returns_prometheus_format() {
        let runner = AppState::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        let state = ServerState::new(runner, tx);
        let app = metrics_router(Arc::clone(&state));

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(ct.contains("text/plain"));

        let body = String::from_utf8(
            resp.into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
        .unwrap();

        assert!(body.contains("croniq_runners_total"));
        assert!(body.contains("croniq_queue_depth"));
    }

    #[tokio::test]
    async fn metrics_includes_scheduler_heartbeat_when_present() {
        use crate::scheduler::SchedulerHeartbeat;

        let runner = AppState::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut state = ServerState::new(runner, tx);

        let hb = Arc::new(SchedulerHeartbeat::default());
        hb.record_tick(Utc.timestamp_opt(1_700_000_000, 0).unwrap());
        Arc::get_mut(&mut state).unwrap().scheduler_heartbeat = Some(Arc::clone(&hb));

        let app = metrics_router(Arc::clone(&state));
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = String::from_utf8(
            resp.into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
        .unwrap();

        assert!(body.contains("# TYPE croniq_scheduler_last_tick_timestamp gauge"));
        assert!(body.contains("croniq_scheduler_last_tick_timestamp 1700000000"));
        assert!(body.contains("croniq_scheduler_ticks_total 1"));
    }

    #[tokio::test]
    async fn metrics_omits_scheduler_heartbeat_when_absent() {
        let runner = AppState::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        let state = ServerState::new(runner, tx);
        let app = metrics_router(Arc::clone(&state));

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = String::from_utf8(
            resp.into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
        .unwrap();

        assert!(!body.contains("croniq_scheduler_last_tick_timestamp"));
    }

    fn sample_job_metrics(job_key: &str) -> JobExecutionMetrics {
        JobExecutionMetrics {
            job_key: job_key.into(),
            completed: 7,
            failed: 1,
            dead: 2,
            cancelled: 0,
            // Cumulative over {0.2s, 4.5s, 120s} for the shared boundaries.
            duration_buckets: vec![0, 1, 1, 2, 2, 2, 2, 3],
            duration_count: 3,
            duration_sum_ms: 124_700,
            last_run_at: Some(Utc.timestamp_opt(1_700_000_000, 0).unwrap()),
        }
    }

    #[test]
    fn render_job_metrics_emits_counter_histogram_and_gauge() {
        let mut out = String::new();
        render_job_metrics(&mut out, &[sample_job_metrics("billing:invoice")]);

        assert!(out.contains("# TYPE croniq_job_executions_total counter"));
        assert!(out.contains(
            "croniq_job_executions_total{job_key=\"billing:invoice\",state=\"completed\"} 7"
        ));
        assert!(
            out.contains(
                "croniq_job_executions_total{job_key=\"billing:invoice\",state=\"dead\"} 2"
            )
        );

        assert!(out.contains("# TYPE croniq_job_duration_seconds histogram"));
        // The +Inf bucket equals the histogram _count.
        assert!(out.contains(
            "croniq_job_duration_seconds_bucket{job_key=\"billing:invoice\",le=\"+Inf\"} 3"
        ));
        assert!(out.contains("croniq_job_duration_seconds_sum{job_key=\"billing:invoice\"} 124.7"));
        assert!(out.contains("croniq_job_duration_seconds_count{job_key=\"billing:invoice\"} 3"));

        assert!(
            out.contains("croniq_job_last_run_timestamp{job_key=\"billing:invoice\"} 1700000000")
        );
    }

    #[test]
    fn render_job_metrics_escapes_label_and_skips_missing_last_run() {
        let mut metrics = sample_job_metrics("weird\"key");
        metrics.last_run_at = None;
        let mut out = String::new();
        render_job_metrics(&mut out, &[metrics]);

        assert!(out.contains("job_key=\"weird\\\"key\""));
        // The gauge family header may still appear, but there must be no
        // sample line (samples always carry a `{job_key=...}` label set).
        assert!(!out.contains("croniq_job_last_run_timestamp{"));
    }

    #[test]
    fn render_job_state_metrics_emits_fire_timestamps_and_overdue() {
        let now = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let states = vec![
            JobState {
                job_key: "billing:backup".into(),
                next_fire_at: Some(now - chrono::Duration::hours(1)), // overdue
                last_fired_at: Some(now - chrono::Duration::days(1)),
                fire_count: 5,
                status: JobStatus::Active,
                updated_at: now,
            },
            JobState {
                job_key: "etl:sync".into(),
                next_fire_at: Some(now + chrono::Duration::hours(1)), // healthy
                last_fired_at: None,
                fire_count: 0,
                status: JobStatus::Active,
                updated_at: now,
            },
        ];
        let mut out = String::new();
        render_job_state_metrics(&mut out, &states, now);

        assert!(out.contains("# TYPE croniq_job_last_fire_timestamp gauge"));
        assert!(
            out.contains("croniq_job_last_fire_timestamp{job_key=\"billing:backup\"} 1699913600")
        );
        // etl:sync never fired → no last-fire sample.
        assert!(!out.contains("croniq_job_last_fire_timestamp{job_key=\"etl:sync\"}"));

        assert!(out.contains("croniq_job_next_fire_timestamp{job_key=\"billing:backup\"}"));

        // Overdue: the backup's next fire is in the past, etl:sync's is not.
        assert!(out.contains("croniq_job_overdue{job_key=\"billing:backup\"} 1"));
        assert!(out.contains("croniq_job_overdue{job_key=\"etl:sync\"} 0"));
    }

    #[tokio::test]
    async fn metrics_includes_job_series_when_store_present() {
        use croniq_store::models::{Execution, ExecutionState};
        use croniq_store::traits::ExecutionStore;

        let sqlite = croniq_store::sqlite::SqliteStore::in_memory().unwrap();
        let exec = Execution {
            id: uuid::Uuid::new_v4(),
            job_key: "etl:sync".into(),
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
            metadata: std::collections::HashMap::new(),
            created_at: Utc::now(),
        };
        sqlite.create_execution(&exec).unwrap();
        sqlite.claim_execution(exec.id, "r1", Utc::now()).unwrap();
        sqlite
            .complete_execution(
                exec.id,
                ExecutionState::Completed,
                Some(2500),
                None,
                None,
                Utc::now(),
            )
            .unwrap();

        let store = crate::store::sqlite_store(sqlite);
        let runner = AppState::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        let state = ServerState::with_auth(runner, tx, None, Some(store));
        let app = metrics_router(Arc::clone(&state));

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = String::from_utf8(
            resp.into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
        .unwrap();

        assert!(
            body.contains(
                "croniq_job_executions_total{job_key=\"etl:sync\",state=\"completed\"} 1"
            )
        );
        assert!(body.contains("croniq_job_duration_seconds_count{job_key=\"etl:sync\"} 1"));
        assert!(body.contains("croniq_job_last_run_timestamp{job_key=\"etl:sync\"}"));
    }
}

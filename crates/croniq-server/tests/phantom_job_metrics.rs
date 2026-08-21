//! A job removed from the configuration stops reporting (issue #470).
//!
//! `job_states` rows outlive the jobs that created them — nothing deletes one
//! — and the exporter read straight from that table. So a job removed from the
//! Croniqfile months earlier kept emitting
//! `croniq_job_overdue{job_key="demo:smoke"} 1` with a `next_fire_at` far in
//! the past, forever.
//!
//! That defeats the exact alarm the series exists for. An operator who wires up
//! the documented `croniq_job_overdue == 1` alert gets a permanent false
//! positive they cannot clear without hand-editing SQLite, which is the fastest
//! way to teach them to ignore the one signal that catches a wedged scheduler.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use chrono::{Duration, Utc};
use croniq_auth::CallerType;
use croniq_auth::jwt::{JwtConfig, issue_token_pair};
use croniq_runner::AppState;
use croniq_scheduler::misfire::MisfirePolicy;
use croniq_scheduler::schedule::Schedule;
use croniq_scheduler::trigger::Trigger;
use croniq_server::api::ServerState;
use croniq_server::api::server_router;
use croniq_server::metrics::metrics_router;
use croniq_server::sqlite_store;
use croniq_server::store::DynStore;
use croniq_store::models::{JobState, JobStatus};
use croniq_store::sqlite::SqliteStore;
use http_body_util::BodyExt;
use tokio::sync::{RwLock, mpsc};
use tower::util::ServiceExt;

/// The job still in the Croniqfile — overdue and expected to say so.
const LIVE: &str = "etl:sync";
/// The job removed months ago whose state row was left behind.
const PHANTOM: &str = "demo:smoke";

fn overdue_state(job_key: &str, ago: Duration) -> JobState {
    let when = Utc::now() - ago;
    JobState {
        job_key: job_key.into(),
        next_fire_at: Some(when),
        last_fired_at: Some(when),
        fire_count: 1,
        status: JobStatus::Active,
        updated_at: when,
    }
}

/// The fixture both endpoints share: one live overdue job, one phantom whose
/// job was removed 90 days ago, and optionally the trigger snapshot that says
/// which of the two the configuration still defines.
fn fixture(with_triggers: bool) -> Arc<ServerState> {
    let store: DynStore = sqlite_store(SqliteStore::in_memory().unwrap());
    store
        .upsert_job_state(&overdue_state(LIVE, Duration::minutes(5)))
        .unwrap();
    store
        .upsert_job_state(&overdue_state(PHANTOM, Duration::days(90)))
        .unwrap();
    // The `/v1` routes fail closed without a JWT config (issue #431), and the
    // middleware checks the named user exists, so the states half of this
    // fixture needs both. `/metrics` needs neither and ignores them.
    let now = Utc::now();
    store
        .users_create(&croniq_store::models::User {
            user_id: "test-user".into(),
            username: "test-user".into(),
            email: None,
            display_name: None,
            role: croniq_auth::Role::Viewer,
            is_active: true,
            created_at: now,
            updated_at: now,
            last_login_at: None,
        })
        .unwrap();

    let (tx, _rx) = mpsc::unbounded_channel();
    let mut state = ServerState::with_auth(
        AppState::new(),
        tx,
        Some(JwtConfig::for_tests()),
        Some(store),
    );
    if with_triggers {
        // Only the live job is loaded — exactly the state after the phantom
        // was removed from the Croniqfile and the server restarted.
        let mut triggers = HashMap::new();
        triggers.insert(
            LIVE.to_string(),
            Trigger::new(
                LIVE.to_string(),
                Schedule::Disabled,
                chrono_tz::UTC,
                None,
                None,
                MisfirePolicy::default(),
                Utc::now(),
            ),
        );
        Arc::get_mut(&mut state).unwrap().triggers = Some(Arc::new(RwLock::new(triggers)));
    }
    state
}

async fn metrics_body(with_triggers: bool) -> String {
    let state = fixture(with_triggers);
    let resp = metrics_router(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    String::from_utf8(
        resp.into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap()
}

#[tokio::test]
async fn a_removed_job_stops_reporting_overdue() {
    let body = metrics_body(true).await;

    // The live job keeps its signal — the fix must not blunt the alarm.
    assert!(
        body.contains(&format!("croniq_job_overdue{{job_key=\"{LIVE}\"}} 1")),
        "the live overdue job must still report:\n{body}"
    );

    // The phantom is gone from every family, not only the overdue one: a
    // `croniq_job_next_fire_timestamp` months in the past is its own false
    // signal for a "next fire is stale" alert.
    assert!(
        !body.contains(PHANTOM),
        "a job the server does not know about must not appear at all:\n{body}"
    );
}

#[tokio::test]
async fn a_server_without_a_trigger_map_still_reports_every_job() {
    // "Cannot tell which jobs are live" must mean "emit everything", not
    // "emit nothing" — otherwise an embedding without a trigger map silently
    // loses its per-job series.
    let body = metrics_body(false).await;
    assert!(body.contains(LIVE), "{body}");
    assert!(body.contains(PHANTOM), "{body}");
}

/// `GET /v1/jobs/states` is what the dashboard reads, and it computed
/// `overdue` straight from the stored rows — so a removed job was badged
/// permanently overdue in the UI long after #470 had cleaned up the metrics
/// (issue #506).
async fn states_body(with_triggers: bool) -> String {
    let state = fixture(with_triggers);
    let token = issue_token_pair(
        state.jwt_config.as_ref().unwrap(),
        "test-user",
        "test-client",
        CallerType::User,
        Some("test-user"),
        Some(croniq_auth::Role::Viewer),
        croniq_auth::AuthMethod::Password,
        &["jobs:read".to_string()],
        None,
    )
    .unwrap()
    .access_token;
    let resp = server_router(Arc::clone(&state))
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/jobs/states")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    String::from_utf8(
        resp.into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap()
}

#[tokio::test]
async fn the_states_endpoint_drops_a_removed_job() {
    let body = states_body(true).await;
    assert!(
        body.contains(LIVE),
        "the live job must still be listed:
{body}"
    );
    assert!(
        !body.contains(PHANTOM),
        "a job the server does not know about must not be listed:
{body}"
    );
}

#[tokio::test]
async fn the_states_endpoint_lists_everything_without_a_trigger_map() {
    // Same fail-open rule as the exporter: "cannot tell" is not "show none".
    let body = states_body(false).await;
    assert!(body.contains(LIVE), "{body}");
    assert!(body.contains(PHANTOM), "{body}");
}

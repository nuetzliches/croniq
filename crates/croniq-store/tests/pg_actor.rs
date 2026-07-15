//! Regression test: `PgStoreHandle` must be usable from *inside* a Tokio
//! runtime.
//!
//! The raw `PgStore` uses the synchronous `postgres` crate, which calls
//! `runtime.block_on(...)` internally; invoking it from within a Tokio runtime
//! panics ("Cannot start a runtime from within a runtime"). That is exactly why
//! croniq-server could not use it. `PgStoreHandle` runs `PgStore` on a
//! dedicated OS thread to sidestep the panic.
//!
//! This test reproduces the exact failing scenario — connect + drive the store
//! from async tasks — so a regression fails loudly here instead of only blowing
//! up at server boot. Driving it through both the test's own runtime thread and
//! a spawned task confirms it is not merely the calling thread that is safe.
//!
//! No-op unless `CRONIQ_TEST_PG_URL` is set (same convention as
//! `pg_integration.rs`). Gated on the `postgres` feature so the default build
//! compiles it away.
#![cfg(feature = "postgres")]

use chrono::{TimeZone, Utc};
use croniq_store::models::*;
use croniq_store::pg_actor::PgStoreHandle;
use croniq_store::traits::*;
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn pg_handle_usable_inside_tokio_runtime() {
    let Ok(url) = std::env::var("CRONIQ_TEST_PG_URL") else {
        eprintln!("CRONIQ_TEST_PG_URL not set — skipping PgStoreHandle runtime test");
        return;
    };

    // Connect from *inside* the async runtime. With the raw PgStore, connect
    // (and every call below) would panic here.
    let store = PgStoreHandle::connect(&url).expect("connect inside tokio runtime");

    let suffix = Uuid::new_v4().simple().to_string();
    let now = Utc.with_ymd_and_hms(2026, 7, 14, 12, 0, 0).unwrap();
    let job_key = format!("actor:probe:{suffix}");

    // A write, a read-back, and a list — enough round-trips through the actor
    // channel to prove marshalling + the blocking response recv work under
    // Tokio without tripping the nested-runtime panic.
    store
        .upsert_job_state(&JobState {
            job_key: job_key.clone(),
            next_fire_at: Some(now),
            last_fired_at: None,
            fire_count: 1,
            status: JobStatus::Active,
            updated_at: now,
        })
        .expect("upsert via actor");

    let got = store
        .get_job_state(&job_key)
        .expect("get via actor")
        .expect("row exists");
    assert_eq!(got.job_key, job_key);
    assert_eq!(got.fire_count, 1);

    assert!(
        store
            .list_job_states()
            .expect("list via actor")
            .iter()
            .any(|s| s.job_key == job_key)
    );

    // Also drive a call from a *spawned* task (a different Tokio worker) to
    // confirm it isn't only the test's own thread that is safe.
    let store = Arc::new(store);
    let task_store = Arc::clone(&store);
    let task_key = job_key.clone();
    tokio::spawn(async move {
        task_store
            .delete_job_state(&task_key)
            .expect("delete via actor from spawned task");
    })
    .await
    .expect("spawned task joined");

    assert!(
        store
            .get_job_state(&job_key)
            .expect("get after delete")
            .is_none()
    );
}

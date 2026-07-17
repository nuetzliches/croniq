//! Benchmarks for SQLite store operations (in-memory, no disk I/O).

use std::collections::HashMap;

use chrono::{TimeZone, Utc};
use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use croniq_store::{
    models::{Execution, ExecutionState, JobState, JobStatus},
    sqlite::SqliteStore,
    traits::{ExecutionStore, JobStore},
};
use uuid::Uuid;

fn utc(y: i32, m: u32, d: u32, h: u32, min: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(y, m, d, h, min, 0).unwrap()
}

fn make_execution(job_key: &str, _i: usize) -> Execution {
    Execution {
        id: Uuid::new_v4(),
        job_key: job_key.into(),
        fire_at: utc(2026, 4, 12, 10, 0),
        scheduled_for: utc(2026, 4, 12, 10, 0),
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
        created_at: utc(2026, 4, 12, 10, 0),
    }
}

fn make_execution_with_caps(job_key: &str, caps: &[&str]) -> Execution {
    let mut meta = HashMap::new();
    if !caps.is_empty() {
        meta.insert(
            "__require".into(),
            serde_json::to_string(&caps).unwrap_or_default(),
        );
    }
    Execution {
        id: Uuid::new_v4(),
        job_key: job_key.into(),
        fire_at: utc(2026, 4, 12, 10, 0),
        scheduled_for: utc(2026, 4, 12, 10, 0),
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
        metadata: meta,
        created_at: utc(2026, 4, 12, 10, 0),
    }
}

fn make_job_state(job_key: &str) -> JobState {
    JobState {
        job_key: job_key.into(),
        next_fire_at: Some(utc(2026, 4, 12, 11, 0)),
        last_fired_at: Some(utc(2026, 4, 12, 10, 0)),
        fire_count: 42,
        status: JobStatus::Active,
        updated_at: utc(2026, 4, 12, 10, 0),
    }
}

fn seed_store(n: usize) -> SqliteStore {
    let store = SqliteStore::in_memory().unwrap();
    for i in 0..n {
        let exec = make_execution(&format!("bench:job-{}", i % 10), i);
        store.create_execution(&exec).unwrap();
    }
    store
}

// ─── create_execution ────────────────────────────────────────────────────────

fn bench_create_execution(c: &mut Criterion) {
    let mut group = c.benchmark_group("store/create_execution");
    for n in [1, 10, 100, 1000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || {
                    let store = SqliteStore::in_memory().unwrap();
                    let execs: Vec<_> = (0..n).map(|i| make_execution("bench:job", i)).collect();
                    (store, execs)
                },
                |(store, execs)| {
                    for exec in &execs {
                        store.create_execution(exec).unwrap();
                    }
                    black_box(())
                },
                criterion::BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

// ─── upsert_job_state ────────────────────────────────────────────────────────

fn bench_upsert_job_state(c: &mut Criterion) {
    let mut group = c.benchmark_group("store/upsert_job_state");
    for n in [1, 10, 100] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || {
                    let store = SqliteStore::in_memory().unwrap();
                    let states: Vec<_> = (0..n)
                        .map(|i| make_job_state(&format!("bench:job-{i}")))
                        .collect();
                    (store, states)
                },
                |(store, states)| {
                    for state in &states {
                        store.upsert_job_state(state).unwrap();
                    }
                    black_box(())
                },
                criterion::BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

// ─── get_execution ───────────────────────────────────────────────────────────

fn bench_get_execution(c: &mut Criterion) {
    let mut group = c.benchmark_group("store/get_execution");
    for n in [100, 1000, 10_000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            let store = seed_store(n);
            // Pick a known ID from the middle (re-query to get a real UUID)
            let execs = store
                .list_executions(&croniq_store::models::ExecutionFilter {
                    limit: Some(1),
                    ..Default::default()
                })
                .unwrap();
            let target_id = execs[0].id;

            b.iter(|| black_box(store.get_execution(black_box(target_id)).unwrap()))
        });
    }
    group.finish();
}

// ─── find_queued_executions ──────────────────────────────────────────────────

fn bench_find_queued(c: &mut Criterion) {
    let mut group = c.benchmark_group("store/find_queued");
    for n in [100, 1000, 10_000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            let store = seed_store(n);
            b.iter(|| black_box(store.find_queued_executions(&[], 10).unwrap()))
        });
    }
    group.finish();
}

fn bench_find_queued_with_caps(c: &mut Criterion) {
    let mut group = c.benchmark_group("store/find_queued_caps");
    for n in [100, 1000, 10_000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            let store = SqliteStore::in_memory().unwrap();
            for i in 0..n {
                let exec = if i % 2 == 0 {
                    make_execution_with_caps("bench:job", &["billing"])
                } else {
                    make_execution_with_caps("bench:job", &["etl"])
                };
                store.create_execution(&exec).unwrap();
            }

            let caps = vec!["billing".to_string()];
            b.iter(|| black_box(store.find_queued_executions(&caps, 10).unwrap()))
        });
    }
    group.finish();
}

// ─── complete_execution ──────────────────────────────────────────────────────

fn bench_complete_execution(c: &mut Criterion) {
    let mut group = c.benchmark_group("store/complete_execution");
    for n in [100, 1000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || {
                    let store = seed_store(n);
                    let execs = store.find_queued_executions(&[], 1).unwrap();
                    let id = execs[0].id;
                    // Claim it first
                    store
                        .claim_execution(id, "bench-runner", Utc::now())
                        .unwrap();
                    (store, id)
                },
                |(store, id)| {
                    store
                        .complete_execution(
                            id,
                            ExecutionState::Completed,
                            Some(500),
                            None,
                            None,
                            Utc::now(),
                        )
                        .unwrap();
                    black_box(id);
                },
                criterion::BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

// ─── list_job_states ─────────────────────────────────────────────────────────

fn bench_list_job_states(c: &mut Criterion) {
    let mut group = c.benchmark_group("store/list_job_states");
    for n in [10, 100, 1000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            let store = SqliteStore::in_memory().unwrap();
            for i in 0..n {
                store
                    .upsert_job_state(&make_job_state(&format!("bench:job-{i}")))
                    .unwrap();
            }
            b.iter(|| black_box(store.list_job_states().unwrap()))
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_create_execution,
    bench_upsert_job_state,
    bench_get_execution,
    bench_find_queued,
    bench_find_queued_with_caps,
    bench_complete_execution,
    bench_list_job_states,
);
criterion_main!(benches);

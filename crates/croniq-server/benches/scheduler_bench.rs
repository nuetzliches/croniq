//! Integration benchmarks: full scheduler tick with store + queue.
//!
//! The key benchmark is `ephemeral_vs_queued`: proves the performance gain
//! of ephemeral mode (no SQLite INSERT per fire) vs. queued mode.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{Duration as ChronoDuration, Utc};
use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use croniq_config::compile::{
    CatchUpPolicy, DeadLetterConfig, ExecutionMode, JobConfig, RetryConfig, RunnerConfig,
};
use croniq_runner::AppState;
use croniq_scheduler::{misfire::MisfirePolicy, schedule::Schedule, trigger::Trigger};
use croniq_server::{
    SchedulerLoop,
    store::{DynStore, sqlite_store},
};
use croniq_store::sqlite::SqliteStore;

fn make_job(key: &str, mode: ExecutionMode) -> JobConfig {
    JobConfig {
        key: key.into(),
        namespace: "bench".into(),
        name: key.split(':').nth(1).unwrap_or(key).into(),
        variant: None,
        description: None,
        schedule: croniq_config::schedule::CompiledSchedule::Disabled,
        schedule_summary: "every 10 seconds".into(),
        timezone: None,
        calendar: None,
        window: None,
        not_before: None,
        not_after: None,
        runner: RunnerConfig::default(),
        retry: RetryConfig::default(),
        timeout: Some("5m".into()),
        dead_letter: DeadLetterConfig::default(),
        metadata: Default::default(),
        execution_mode: mode,
        catch_up: CatchUpPolicy::default(),
        queue_ttl: None,
        max_queue_depth: None,
    }
}

fn make_trigger_due(key: &str) -> Trigger {
    let schedule = Schedule::Interval { seconds: 10 };
    let now = Utc::now();
    let mut trigger = Trigger::new(
        key.into(),
        schedule,
        chrono_tz::UTC,
        None,
        None,
        MisfirePolicy::FireNow,
        now - ChronoDuration::seconds(60),
    );
    // Set next_fire_at to 5s ago so it fires immediately
    trigger.next_fire_at = Some(now - ChronoDuration::seconds(5));
    trigger
}

fn make_trigger_future(key: &str) -> Trigger {
    let schedule = Schedule::Interval { seconds: 3600 };
    Trigger::new(
        key.into(),
        schedule,
        chrono_tz::UTC,
        None,
        None,
        MisfirePolicy::FireNow,
        Utc::now(),
    )
}

struct BenchSetup {
    triggers: HashMap<String, Trigger>,
    jobs: Vec<JobConfig>,
    store: DynStore,
    runner: Arc<AppState>,
}

fn build_setup(n: usize, mode: ExecutionMode, all_due: bool) -> BenchSetup {
    let mut triggers = HashMap::new();
    let mut jobs = Vec::new();

    for i in 0..n {
        let key = format!("bench:job-{i}");
        let trigger = if all_due {
            make_trigger_due(&key)
        } else {
            make_trigger_future(&key)
        };
        triggers.insert(key.clone(), trigger);
        jobs.push(make_job(&key, mode));
    }

    BenchSetup {
        triggers,
        jobs,
        store: sqlite_store(SqliteStore::in_memory().unwrap()),
        runner: AppState::new(),
    }
}

fn build_scheduler(setup: BenchSetup) -> SchedulerLoop {
    let mut scheduler = SchedulerLoop::new(setup.triggers, setup.jobs, setup.store, setup.runner);
    // Disable quota limits for benchmarking
    scheduler.set_quota_defaults(100_000, 100_000);
    scheduler
}

// ─── Tick benchmarks ─────────────────────────────────────────────────────────

fn bench_tick_none_due(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("tick/none_due");

    for n in [10, 100, 1000, 10_000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || build_scheduler(build_setup(n, ExecutionMode::Queued, false)),
                |mut scheduler| rt.block_on(async { black_box(scheduler.tick(Utc::now()).await) }),
                criterion::BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

fn bench_tick_all_due_queued(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("tick/all_due_queued");

    for n in [10, 100, 1000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || build_scheduler(build_setup(n, ExecutionMode::Queued, true)),
                |mut scheduler| rt.block_on(async { black_box(scheduler.tick(Utc::now()).await) }),
                criterion::BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

fn bench_tick_all_due_ephemeral(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("tick/all_due_ephemeral");

    for n in [10, 100, 1000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || build_scheduler(build_setup(n, ExecutionMode::Ephemeral, true)),
                |mut scheduler| rt.block_on(async { black_box(scheduler.tick(Utc::now()).await) }),
                criterion::BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

fn bench_tick_10pct_due(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("tick/10pct_due");

    for n in [100, 1000, 10_000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || {
                    let mut triggers = HashMap::new();
                    let mut jobs = Vec::new();
                    for i in 0..n {
                        let key = format!("bench:job-{i}");
                        let trigger = if i % 10 == 0 {
                            make_trigger_due(&key)
                        } else {
                            make_trigger_future(&key)
                        };
                        triggers.insert(key.clone(), trigger);
                        jobs.push(make_job(&key, ExecutionMode::Queued));
                    }
                    let setup = BenchSetup {
                        triggers,
                        jobs,
                        store: sqlite_store(SqliteStore::in_memory().unwrap()),
                        runner: AppState::new(),
                    };
                    build_scheduler(setup)
                },
                |mut scheduler| rt.block_on(async { black_box(scheduler.tick(Utc::now()).await) }),
                criterion::BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

fn bench_tick_sustained(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("tick/sustained_100_triggers_100_ticks", |b| {
        b.iter_batched(
            || build_scheduler(build_setup(100, ExecutionMode::Queued, true)),
            |mut scheduler| {
                rt.block_on(async {
                    let mut now = Utc::now();
                    for _ in 0..100 {
                        black_box(scheduler.tick(now).await);
                        now = now + ChronoDuration::seconds(1);
                    }
                })
            },
            criterion::BatchSize::SmallInput,
        )
    });
}

/// The key benchmark: side-by-side ephemeral vs queued in one criterion group.
fn bench_ephemeral_vs_queued(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("ephemeral_vs_queued");

    for n in [10, 100, 1000] {
        group.bench_with_input(BenchmarkId::new("queued", n), &n, |b, &n| {
            b.iter_batched(
                || build_scheduler(build_setup(n, ExecutionMode::Queued, true)),
                |mut s| rt.block_on(async { black_box(s.tick(Utc::now()).await) }),
                criterion::BatchSize::SmallInput,
            )
        });
        group.bench_with_input(BenchmarkId::new("ephemeral", n), &n, |b, &n| {
            b.iter_batched(
                || build_scheduler(build_setup(n, ExecutionMode::Ephemeral, true)),
                |mut s| rt.block_on(async { black_box(s.tick(Utc::now()).await) }),
                criterion::BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_tick_none_due,
    bench_tick_all_due_queued,
    bench_tick_all_due_ephemeral,
    bench_tick_10pct_due,
    bench_tick_sustained,
    bench_ephemeral_vs_queued,
);
criterion_main!(benches);

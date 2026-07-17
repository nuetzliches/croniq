//! Benchmarks for the WorkQueue: enqueue, dequeue, capability matching, removal.

use chrono::Utc;
use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use croniq_runner::WorkItem;
use croniq_runner::queue::WorkQueue;

fn make_item(id: usize, require: Vec<&str>) -> WorkItem {
    WorkItem {
        execution_id: format!("exec-{id}"),
        job_key: format!("job:{id}"),
        fire_at: Utc::now(),
        scheduled_for: Utc::now(),
        attempt: 1,
        require: require.into_iter().map(String::from).collect(),
        prefer: vec![],
        metadata: serde_json::Value::Null,
        timeout: "5m".into(),
    }
}

fn make_queue(n: usize, require: Vec<&str>) -> WorkQueue {
    let mut q = WorkQueue::new();
    for i in 0..n {
        q.enqueue(make_item(i, require.clone()));
    }
    q
}

// ─── Enqueue ─────────────────────────────────────────────────────────────────

fn bench_enqueue(c: &mut Criterion) {
    let mut group = c.benchmark_group("queue/enqueue");
    for n in [100, 1000, 10_000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || {
                    (
                        WorkQueue::new(),
                        (0..n).map(|i| make_item(i, vec![])).collect::<Vec<_>>(),
                    )
                },
                |(mut q, items)| {
                    for item in items {
                        q.enqueue(item);
                    }
                    black_box(q.len())
                },
                criterion::BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

// ─── Dequeue ─────────────────────────────────────────────────────────────────

fn bench_dequeue_no_caps(c: &mut Criterion) {
    let mut group = c.benchmark_group("queue/dequeue_no_caps");
    for n in [100, 1000, 10_000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || make_queue(n, vec![]),
                |mut q| black_box(q.dequeue_for(&[])),
                criterion::BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

fn bench_dequeue_last_match(c: &mut Criterion) {
    let mut group = c.benchmark_group("queue/dequeue_last_match");
    for n in [100, 1000, 10_000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || {
                    let mut q = make_queue(n - 1, vec!["billing"]);
                    q.enqueue(make_item(n, vec!["special"]));
                    q
                },
                |mut q| black_box(q.dequeue_for(&["special".into()])),
                criterion::BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

fn bench_dequeue_no_match(c: &mut Criterion) {
    let mut group = c.benchmark_group("queue/dequeue_no_match");
    for n in [100, 1000, 10_000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || make_queue(n, vec!["billing"]),
                |mut q| black_box(q.dequeue_for(&["nonexistent".into()])),
                criterion::BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

fn bench_dequeue_many(c: &mut Criterion) {
    let mut group = c.benchmark_group("queue/dequeue_many");
    for limit in [1, 5, 10, 50] {
        group.bench_with_input(BenchmarkId::from_parameter(limit), &limit, |b, &limit| {
            b.iter_batched(
                || make_queue(1000, vec![]),
                |mut q| black_box(q.dequeue_many_for(&[], limit)),
                criterion::BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

// ─── Peek ────────────────────────────────────────────────────────────────────

fn bench_peek_n(c: &mut Criterion) {
    let mut group = c.benchmark_group("queue/peek_n");
    for n in [100, 1000, 10_000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            let q = make_queue(n, vec![]);
            b.iter(|| black_box(q.peek_n(n)))
        });
    }
    group.finish();
}

// ─── Remove ──────────────────────────────────────────────────────────────────

fn bench_remove_by_id(c: &mut Criterion) {
    let mut group = c.benchmark_group("queue/remove_mid");
    for n in [100, 1000, 10_000] {
        let target = format!("exec-{}", n / 2);
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || make_queue(n, vec![]),
                |mut q| black_box(q.remove(&target)),
                criterion::BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_enqueue,
    bench_dequeue_no_caps,
    bench_dequeue_last_match,
    bench_dequeue_no_match,
    bench_dequeue_many,
    bench_peek_n,
    bench_remove_by_id,
);
criterion_main!(benches);

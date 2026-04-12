//! Benchmarks for schedule evaluation and trigger state machine.

use chrono::{Duration as ChronoDuration, NaiveTime, TimeZone, Utc};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use croniq_scheduler::{
    misfire::MisfirePolicy,
    schedule::{MonthDay, Schedule},
    trigger::Trigger,
};

fn utc(y: i32, m: u32, d: u32, h: u32, min: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(y, m, d, h, min, 0).unwrap()
}

// ─── Schedule::next_fire_after ───────────────────────────────────────────────

fn bench_interval_next_fire(c: &mut Criterion) {
    let schedule = Schedule::Interval { seconds: 300 };
    let after = utc(2026, 4, 12, 10, 0);
    let tz = chrono_tz::UTC;

    c.bench_function("schedule/interval_next_fire", |b| {
        b.iter(|| black_box(schedule.next_fire_after(black_box(after), &tz)))
    });
}

fn bench_daily_next_fire(c: &mut Criterion) {
    let schedule = Schedule::Daily {
        time: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
    };
    let after = utc(2026, 4, 12, 10, 0);
    let tz = chrono_tz::Europe::Vienna;

    c.bench_function("schedule/daily_next_fire_tz", |b| {
        b.iter(|| black_box(schedule.next_fire_after(black_box(after), &tz)))
    });
}

fn bench_weekday_next_fire(c: &mut Criterion) {
    let schedule = Schedule::Weekdays {
        days: vec![
            chrono::Weekday::Mon,
            chrono::Weekday::Tue,
            chrono::Weekday::Wed,
            chrono::Weekday::Thu,
            chrono::Weekday::Fri,
        ],
        time: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
    };
    // Saturday 10:00 → must skip to Monday 09:00
    let after = utc(2026, 4, 11, 10, 0); // Saturday
    let tz = chrono_tz::UTC;

    c.bench_function("schedule/weekday_next_fire_worst", |b| {
        b.iter(|| black_box(schedule.next_fire_after(black_box(after), &tz)))
    });
}

fn bench_monthly_next_fire(c: &mut Criterion) {
    let schedule = Schedule::Monthly {
        ordinals: vec![MonthDay::Day(1), MonthDay::Day(15), MonthDay::Last],
        time: NaiveTime::from_hms_opt(6, 0, 0).unwrap(),
    };
    let after = utc(2026, 4, 16, 0, 0);
    let tz = chrono_tz::UTC;

    c.bench_function("schedule/monthly_next_fire_3ord", |b| {
        b.iter(|| black_box(schedule.next_fire_after(black_box(after), &tz)))
    });
}

fn bench_next_n_fires(c: &mut Criterion) {
    let schedule = Schedule::Daily {
        time: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
    };
    let after = utc(2026, 1, 1, 0, 0);
    let tz = chrono_tz::UTC;

    let mut group = c.benchmark_group("schedule/next_n_fires");
    for n in [10, 100, 1000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter(|| black_box(schedule.next_n_fires(after, &tz, n)))
        });
    }
    group.finish();
}

// ─── Trigger ─────────────────────────────────────────────────────────────────

fn make_trigger(schedule: Schedule, now: chrono::DateTime<Utc>) -> Trigger {
    Trigger::new(
        "bench:job".into(),
        schedule,
        chrono_tz::UTC,
        None,
        None,
        MisfirePolicy::FireNow,
        now,
    )
}

fn bench_trigger_evaluate(c: &mut Criterion) {
    let now = utc(2026, 4, 12, 12, 0);
    let mut trigger = make_trigger(Schedule::Interval { seconds: 10 }, now);
    // Set next_fire_at to 5s ago so evaluate returns Some
    trigger.next_fire_at = Some(now - ChronoDuration::seconds(5));

    c.bench_function("trigger/evaluate_due", |b| {
        b.iter(|| black_box(trigger.evaluate(black_box(now))))
    });
}

fn bench_trigger_evaluate_not_due(c: &mut Criterion) {
    let now = utc(2026, 4, 12, 12, 0);
    let trigger = make_trigger(Schedule::Interval { seconds: 3600 }, now);
    // next_fire_at is ~1h in the future

    c.bench_function("trigger/evaluate_not_due", |b| {
        b.iter(|| black_box(trigger.evaluate(black_box(now))))
    });
}

fn bench_trigger_lifecycle(c: &mut Criterion) {
    let start = utc(2026, 4, 12, 12, 0);

    c.bench_function("trigger/lifecycle_1000_cycles", |b| {
        b.iter_batched(
            || make_trigger(Schedule::Interval { seconds: 10 }, start),
            |mut trigger| {
                let mut now = start + ChronoDuration::seconds(10);
                for _ in 0..1000 {
                    if let Some(fire_at) = trigger.evaluate(now) {
                        trigger.mark_fired(fire_at, now);
                    }
                    now = now + ChronoDuration::seconds(10);
                }
            },
            criterion::BatchSize::SmallInput,
        )
    });
}

fn bench_compute_next_fire_calendar(c: &mut Criterion) {
    use croniq_scheduler::calendar::{Calendar, CalendarRule};

    // Calendar that excludes weekends — forces iteration in compute_next_fire
    let calendar = Calendar {
        name: "business_days".into(),
        timezone: None,
        includes: vec![],
        excludes: vec![
            CalendarRule::Weekly(vec![chrono::Weekday::Sat, chrono::Weekday::Sun]),
        ],
    };

    let now = utc(2026, 4, 11, 10, 0); // Saturday
    let schedule = Schedule::Daily {
        time: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
    };

    let trigger = Trigger::new(
        "bench:cal".into(),
        schedule,
        chrono_tz::UTC,
        Some(calendar),
        None,
        MisfirePolicy::FireNow,
        now,
    );

    c.bench_function("trigger/compute_next_fire_calendar", |b| {
        b.iter(|| {
            let t = trigger.clone();
            black_box(t.next_fire_at)
        })
    });
}

criterion_group!(
    benches,
    bench_interval_next_fire,
    bench_daily_next_fire,
    bench_weekday_next_fire,
    bench_monthly_next_fire,
    bench_next_n_fires,
    bench_trigger_evaluate,
    bench_trigger_evaluate_not_due,
    bench_trigger_lifecycle,
    bench_compute_next_fire_calendar,
);
criterion_main!(benches);

//! Read-only aggregation endpoints powering the redesigned Dashboard
//! and Insights pages. Everything here is computed on-the-fly from the
//! existing `executions` table — no extra schema. If volumes grow past
//! ~50k executions/day a follow-up PR can add a materialised view.

use std::sync::Arc;

use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Timelike, Utc};
use chrono_tz::Tz;
use croniq_auth::CallerContext;
use croniq_auth::context::Scope;
use croniq_store::models::{ExecutionFilter, ExecutionState};
use serde::{Deserialize, Serialize};

use super::ServerState;
use crate::api::auth_middleware::require_scope;

// ─── /v1/jobs/{key}/stats ────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct JobStatsParams {
    /// Look-back window in days. Defaults to 7. Clamped to [1, 90].
    #[serde(default)]
    pub days: Option<u32>,
}

#[derive(Serialize)]
pub struct JobStatsResponse {
    pub job_key: String,
    pub window_days: u32,
    pub total: u64,
    pub completed: u64,
    pub failed: u64,
    pub dead: u64,
    pub success_rate: f64,
    pub p50_ms: Option<i64>,
    pub p95_ms: Option<i64>,
    pub p99_ms: Option<i64>,
    pub last_failure_at: Option<DateTime<Utc>>,
}

pub async fn handle_job_stats(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Path(job_key): Path<String>,
    Query(params): Query<JobStatsParams>,
) -> Result<Json<JobStatsResponse>, StatusCode> {
    require_scope(&ctx, Scope::EXECUTIONS_READ)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let days = params.days.unwrap_or(7).clamp(1, 90);
    let since = Utc::now() - Duration::days(days as i64);
    let filter = ExecutionFilter {
        job_key: Some(job_key.clone()),
        since: Some(since),
        // Pull a generous slice — the API consumer can downsample later.
        limit: Some(10_000),
        ..Default::default()
    };
    let execs = store
        .list_executions(&filter)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut completed = 0u64;
    let mut failed = 0u64;
    let mut dead = 0u64;
    let mut durations: Vec<i64> = Vec::new();
    let mut last_failure_at: Option<DateTime<Utc>> = None;
    for e in &execs {
        match e.state {
            ExecutionState::Completed => completed += 1,
            ExecutionState::Failed => {
                failed += 1;
                if last_failure_at.map(|t| e.fire_at > t).unwrap_or(true) {
                    last_failure_at = Some(e.fire_at);
                }
            }
            ExecutionState::Dead => {
                dead += 1;
                if last_failure_at.map(|t| e.fire_at > t).unwrap_or(true) {
                    last_failure_at = Some(e.fire_at);
                }
            }
            _ => {}
        }
        if let Some(d) = e.duration_ms {
            durations.push(d);
        }
    }
    let total = completed + failed + dead;
    let terminal_for_rate = (completed + failed + dead) as f64;
    let success_rate = if terminal_for_rate > 0.0 {
        completed as f64 / terminal_for_rate
    } else {
        0.0
    };

    durations.sort_unstable();
    let pct = |q: f64| -> Option<i64> {
        if durations.is_empty() {
            return None;
        }
        let idx = ((durations.len() as f64 - 1.0) * q).round() as usize;
        durations.get(idx).copied()
    };

    Ok(Json(JobStatsResponse {
        job_key,
        window_days: days,
        total,
        completed,
        failed,
        dead,
        success_rate,
        p50_ms: pct(0.50),
        p95_ms: pct(0.95),
        p99_ms: pct(0.99),
        last_failure_at,
    }))
}

// ─── /v1/executions/throughput ───────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ThroughputParams {
    /// "24h", "7d", "30d". Defaults to "24h". Bucket size is implied:
    /// hourly for 24h, daily for 7d/30d.
    #[serde(default)]
    pub window: Option<String>,
}

#[derive(Serialize)]
pub struct ThroughputBucket {
    /// Bucket start time (UTC, ISO 8601).
    pub start: DateTime<Utc>,
    pub ok: u64,
    pub err: u64,
}

#[derive(Serialize)]
pub struct ThroughputResponse {
    pub window: String,
    pub bucket: String, // "hour" or "day"
    pub buckets: Vec<ThroughputBucket>,
}

pub async fn handle_throughput(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Query(params): Query<ThroughputParams>,
) -> Result<Json<ThroughputResponse>, StatusCode> {
    require_scope(&ctx, Scope::EXECUTIONS_READ)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let window_str = params.window.unwrap_or_else(|| "24h".into());
    let (since, bucket_secs, bucket_kind, bucket_count) = match window_str.as_str() {
        "24h" => (Utc::now() - Duration::hours(24), 3600, "hour", 24usize),
        "7d" => (Utc::now() - Duration::days(7), 86_400, "day", 7),
        "30d" => (Utc::now() - Duration::days(30), 86_400, "day", 30),
        _ => return Err(StatusCode::BAD_REQUEST),
    };

    let filter = ExecutionFilter {
        since: Some(since),
        limit: Some(100_000),
        ..Default::default()
    };
    let execs = store
        .list_executions(&filter)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Bucket boundaries are aligned to UTC hour/day starts so two
    // consecutive calls return the same x-axis. Anchor on `since`
    // floored to the bucket boundary.
    let anchor = match bucket_kind {
        "hour" => Utc
            .with_ymd_and_hms(since.year(), since.month(), since.day(), since.hour(), 0, 0)
            .single()
            .unwrap_or(since),
        _ => Utc
            .with_ymd_and_hms(since.year(), since.month(), since.day(), 0, 0, 0)
            .single()
            .unwrap_or(since),
    };
    let mut buckets: Vec<ThroughputBucket> = (0..bucket_count)
        .map(|i| ThroughputBucket {
            start: anchor + Duration::seconds(bucket_secs * i as i64),
            ok: 0,
            err: 0,
        })
        .collect();

    for e in &execs {
        let secs_since_anchor = (e.fire_at - anchor).num_seconds();
        if secs_since_anchor < 0 {
            continue;
        }
        let idx = (secs_since_anchor / bucket_secs) as usize;
        if idx >= buckets.len() {
            continue;
        }
        match e.state {
            ExecutionState::Completed => buckets[idx].ok += 1,
            ExecutionState::Failed | ExecutionState::Dead => buckets[idx].err += 1,
            _ => {}
        }
    }

    Ok(Json(ThroughputResponse {
        window: window_str,
        bucket: bucket_kind.into(),
        buckets,
    }))
}

// ─── /v1/insights/failures ───────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct FailuresParams {
    /// Window in days. Clamped to [7, 90]. Default 28.
    #[serde(default)]
    pub days: Option<u32>,
    /// IANA zone the days and hours are counted in. Default UTC. The
    /// dashboard sends the browser's zone, the one it shows every other
    /// time in; an unknown name is a `400`, not a silent UTC (ADR-0007).
    #[serde(default)]
    pub tz: Option<String>,
}

#[derive(Serialize)]
pub struct FailureHeatmap {
    pub days: u32,
    /// The zone `dates` and the hour columns are in.
    pub timezone: String,
    /// The calendar date of each row, oldest first; the last is today.
    pub dates: Vec<NaiveDate>,
    /// `rows[day_index][hour_of_day]` = failure count.
    /// `day_index = 0` is the oldest day in the window;
    /// `day_index = days-1` is today.
    pub rows: Vec<Vec<u32>>,
    pub hotspots: Vec<HeatmapHotspot>,
}

#[derive(Serialize)]
pub struct HeatmapHotspot {
    pub hour: u32,
    pub failures: u32,
}

pub async fn handle_failure_heatmap(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Query(params): Query<FailuresParams>,
) -> Result<Json<FailureHeatmap>, StatusCode> {
    require_scope(&ctx, Scope::EXECUTIONS_READ)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let days = params.days.unwrap_or(28).clamp(7, 90);
    let tz = match params.tz.as_deref().filter(|t| !t.is_empty()) {
        Some(name) => croniq_config::timezone::parse(name).map_err(|_| StatusCode::BAD_REQUEST)?,
        None => chrono_tz::UTC,
    };
    let now = Utc::now();
    // A day more than the window. The oldest row starts at a local midnight
    // less than `days` days ago, but a DST change in between moves that
    // midnight by an hour; the spare day covers it. What falls outside the
    // rows is dropped when it is bucketed.
    let since = now - Duration::days(days as i64 + 1);
    let filter = ExecutionFilter {
        states: vec![ExecutionState::Failed],
        since: Some(since),
        limit: Some(100_000),
        ..Default::default()
    };
    let mut execs = store
        .list_executions(&filter)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    // Also count dead-states as "failures" in the heatmap — they're
    // exhausted retries, which is the user-visible outcome.
    let dead_filter = ExecutionFilter {
        states: vec![ExecutionState::Dead],
        since: Some(since),
        limit: Some(100_000),
        ..Default::default()
    };
    execs.extend(
        store
            .list_executions(&dead_filter)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    );

    let today = now.with_timezone(&tz).date_naive();
    let (rows, hourly_totals) = bucket_failures(execs.iter().map(|e| e.fire_at), tz, today, days);
    let dates = (0..days)
        .map(|i| today - Duration::days((days - 1 - i) as i64))
        .collect();

    let mut hotspots: Vec<HeatmapHotspot> = hourly_totals
        .iter()
        .enumerate()
        .filter(|&(_, &v)| v > 0)
        .map(|(h, &v)| HeatmapHotspot {
            hour: h as u32,
            failures: v,
        })
        .collect();
    hotspots.sort_by_key(|h| std::cmp::Reverse(h.failures));
    hotspots.truncate(3);

    Ok(Json(FailureHeatmap {
        days,
        timezone: tz.name().to_string(),
        dates,
        rows,
        hotspots,
    }))
}

/// Count failures into `days` rows of 24 hours, by the calendar date and the
/// hour of day each one fired at in `tz`; the last row is `today`. Returns
/// the rows and the per-hour totals across them.
///
/// By date, not by `(today_midnight - fire_at).num_days()`: that truncates
/// toward zero, so all of yesterday counted as today and every older day
/// slid one row towards it (#864). A failure outside the rows — before the
/// oldest day, or dated after `today` by a clock ahead of ours — is left out.
fn bucket_failures(
    fire_ats: impl Iterator<Item = DateTime<Utc>>,
    tz: Tz,
    today: NaiveDate,
    days: u32,
) -> (Vec<Vec<u32>>, [u32; 24]) {
    let mut rows = vec![vec![0u32; 24]; days as usize];
    let mut hourly_totals = [0u32; 24];
    for fire_at in fire_ats {
        let local = fire_at.with_timezone(&tz);
        let days_back = (today - local.date_naive()).num_days();
        if days_back < 0 || days_back >= days as i64 {
            continue;
        }
        let day_index = (days as i64 - 1 - days_back) as usize;
        let hour = local.hour() as usize;
        rows[day_index][hour] = rows[day_index][hour].saturating_add(1);
        hourly_totals[hour] = hourly_totals[hour].saturating_add(1);
    }
    (rows, hourly_totals)
}

// ─── /v1/insights/runtime ────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct RuntimeParams {
    /// "24h", "7d", "30d". Defaults to "24h".
    #[serde(default)]
    pub window: Option<String>,
}

#[derive(Serialize)]
pub struct JobRuntimeRow {
    pub job_key: String,
    pub runs: u64,
    pub failed: u64,
    pub total_ms: i64,
    pub wait_median_ms: i64,
    pub failed_ms: i64,
    pub succeeded_median_ms: Option<i64>,
    pub failed_median_ms: Option<i64>,
}

#[derive(Serialize)]
pub struct RuntimeResponse {
    pub window: String,
    pub since: DateTime<Utc>,
    /// Largest total first.
    pub jobs: Vec<JobRuntimeRow>,
}

/// `GET /v1/insights/runtime?window=24h|7d|30d`: how much run time each job
/// used in the window, from the runs that finished in it with a duration.
///
/// Summed in the store (one grouped scan) rather than over a capped
/// `list_executions` like the endpoints above: a month of a busy job set is
/// well past their 100 000-row limit, and a total that quietly stops counting
/// would understate exactly the jobs it is meant to show.
pub async fn handle_runtime(
    State(state): State<Arc<ServerState>>,
    Extension(ctx): Extension<CallerContext>,
    Query(params): Query<RuntimeParams>,
) -> Result<Json<RuntimeResponse>, StatusCode> {
    require_scope(&ctx, Scope::EXECUTIONS_READ)?;
    let store = state
        .store
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let window = params.window.unwrap_or_else(|| "24h".into());
    let span = match window.as_str() {
        "24h" => Duration::hours(24),
        "7d" => Duration::days(7),
        "30d" => Duration::days(30),
        _ => return Err(StatusCode::BAD_REQUEST),
    };
    let since = Utc::now() - span;
    let jobs = store
        .job_runtime_since(since)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .into_iter()
        .map(|r| JobRuntimeRow {
            job_key: r.job_key,
            runs: r.runs,
            failed: r.failed,
            total_ms: r.total_ms,
            wait_median_ms: r.wait_median_ms,
            failed_ms: r.failed_ms,
            succeeded_median_ms: r.succeeded_median_ms,
            failed_median_ms: r.failed_median_ms,
        })
        .collect();
    Ok(Json(RuntimeResponse {
        window,
        since,
        jobs,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn date(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    /// The repro from #864: yesterday evening's failures got today's row.
    #[test]
    fn yesterday_evening_is_yesterdays_row_not_todays() {
        let fires = [at("2026-10-21T20:00:00Z"), at("2026-10-22T13:00:00Z")];
        let (rows, _) = bucket_failures(fires.into_iter(), chrono_tz::UTC, date("2026-10-22"), 7);
        assert_eq!(rows[6][13], 1, "today, 13:00");
        assert_eq!(
            rows[6][20], 0,
            "nothing at 20:00 today — it has not happened yet"
        );
        assert_eq!(rows[5][20], 1, "yesterday, 20:00");
    }

    #[test]
    fn each_older_day_keeps_its_own_row() {
        let fires = [at("2026-10-16T05:00:00Z"), at("2026-10-19T05:00:00Z")];
        let (rows, _) = bucket_failures(fires.into_iter(), chrono_tz::UTC, date("2026-10-22"), 7);
        assert_eq!(rows[0][5], 1, "the oldest row is 2026-10-16");
        assert_eq!(rows[3][5], 1, "2026-10-19 is three rows below it");
        assert_eq!(rows.iter().flatten().sum::<u32>(), 2);
    }

    #[test]
    fn days_and_hours_are_counted_in_the_requested_zone() {
        // 23:30 UTC on the 21st is 01:30 on the 22nd in Vienna (CEST, +2).
        let fires = [at("2026-10-21T23:30:00Z")];
        let (rows, hourly) = bucket_failures(
            fires.into_iter(),
            chrono_tz::Europe::Vienna,
            date("2026-10-22"),
            7,
        );
        assert_eq!(rows[6][1], 1);
        assert_eq!(hourly[1], 1);

        let (rows, hourly) =
            bucket_failures(fires.into_iter(), chrono_tz::UTC, date("2026-10-22"), 7);
        assert_eq!(rows[5][23], 1);
        assert_eq!(hourly[23], 1);
    }

    #[test]
    fn failures_outside_the_rows_are_left_out() {
        let fires = [
            at("2026-10-15T12:00:00Z"), // a day before the oldest row
            at("2026-10-23T00:30:00Z"), // tomorrow, from a clock ahead of ours
        ];
        let (rows, hourly) =
            bucket_failures(fires.into_iter(), chrono_tz::UTC, date("2026-10-22"), 7);
        assert_eq!(rows.iter().flatten().sum::<u32>(), 0);
        assert_eq!(hourly.iter().sum::<u32>(), 0);
    }
}

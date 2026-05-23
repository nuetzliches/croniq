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
use chrono::{DateTime, Datelike, Duration, TimeZone, Timelike, Utc};
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
}

#[derive(Serialize)]
pub struct FailureHeatmap {
    pub days: u32,
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
    let since = Utc::now() - Duration::days(days as i64);
    let filter = ExecutionFilter {
        state: Some(ExecutionState::Failed),
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
        state: Some(ExecutionState::Dead),
        since: Some(since),
        limit: Some(100_000),
        ..Default::default()
    };
    execs.extend(
        store
            .list_executions(&dead_filter)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    );

    let mut rows = vec![vec![0u32; 24]; days as usize];
    let mut hourly_totals = [0u32; 24];
    let today = Utc::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc();
    for e in &execs {
        let days_back = ((today - e.fire_at).num_days()).max(0) as usize;
        let day_index = (days as usize).saturating_sub(1).saturating_sub(days_back);
        if day_index >= rows.len() {
            continue;
        }
        let hour = e.fire_at.hour() as usize;
        rows[day_index][hour] = rows[day_index][hour].saturating_add(1);
        hourly_totals[hour] = hourly_totals[hour].saturating_add(1);
    }

    let mut hotspots: Vec<HeatmapHotspot> = hourly_totals
        .iter()
        .enumerate()
        .filter(|&(_, &v)| v > 0)
        .map(|(h, &v)| HeatmapHotspot {
            hour: h as u32,
            failures: v,
        })
        .collect();
    hotspots.sort_by(|a, b| b.failures.cmp(&a.failures));
    hotspots.truncate(3);

    Ok(Json(FailureHeatmap {
        days,
        rows,
        hotspots,
    }))
}

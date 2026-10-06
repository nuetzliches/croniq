//! Dashboard forecast — server-side wrapper around
//! [`croniq_scheduler::forecast`].
//!
//! The HTTP handler in `api/dashboard.rs` calls [`compute_forecast`] directly;
//! the MCP `dashboard_forecast` tool calls the same function from
//! `croniq-scheduler` so both surfaces produce identical bucketing.

use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;

/// How long past its due time an active job's next fire may be before the job
/// counts as overdue.
///
/// The scheduler ticks once a second and writes the new `next_fire_at` when it
/// fires, so every job is briefly past due right before its fire is taken.
/// Without a margin, anything that read the state in that instant — the
/// dashboard, which re-reads the schedule right after a fire, or a Prometheus
/// scrape — saw a healthy job as overdue, and a `croniq_job_overdue == 1`
/// alert could trip at random. Thirty seconds is far past any tick, and a
/// scheduler that has really stopped is still flagged within half a minute.
pub const OVERDUE_GRACE_SECS: i64 = 30;

/// Whether an active job whose next fire is `next_fire_at` is overdue at `now`.
pub fn is_overdue(next_fire_at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    next_fire_at < now - Duration::seconds(OVERDUE_GRACE_SECS)
}

pub use croniq_scheduler::forecast::{
    ForecastBucket, ForecastResponse, compute_forecast, compute_forecast_seconds,
};

#[derive(Deserialize)]
pub struct ForecastQuery {
    /// Forecast window in minutes (max 240). Default: 60.
    #[serde(default = "default_window")]
    pub window_minutes: u32,
    /// Bucket size in minutes. Default: 5.
    #[serde(default = "default_bucket")]
    pub bucket_minutes: u32,
}

fn default_window() -> u32 {
    60
}

fn default_bucket() -> u32 {
    5
}

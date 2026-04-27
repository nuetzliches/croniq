//! Dashboard forecast — server-side wrapper around
//! [`croniq_scheduler::forecast`].
//!
//! The HTTP handler in `api/dashboard.rs` calls [`compute_forecast`] directly;
//! the MCP `dashboard_forecast` tool calls the same function from
//! `croniq-scheduler` so both surfaces produce identical bucketing.

use serde::Deserialize;

pub use croniq_scheduler::forecast::{ForecastBucket, ForecastResponse, compute_forecast};

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

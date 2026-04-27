//! Project upcoming fire times for all armed triggers into discrete time
//! buckets. Used by both the dashboard's `/v1/dashboard/forecast` HTTP
//! endpoint and the `dashboard_forecast` MCP tool.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

use crate::trigger::Trigger;

/// One forecast bucket — start/end timestamps plus the jobs that fire in it.
#[derive(Serialize, Debug, Clone)]
pub struct ForecastBucket {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub count: u32,
    pub jobs: Vec<String>,
}

/// Forecast response: the original window/bucket sizes plus the projection.
#[derive(Serialize, Debug, Clone)]
pub struct ForecastResponse {
    pub window_minutes: u32,
    pub bucket_minutes: u32,
    pub buckets: Vec<ForecastBucket>,
}

/// Project fire times for all armed triggers into time buckets.
///
/// `window_minutes` is clamped to 240 (4 hours) and `bucket_minutes` is
/// floored at 1. A trigger contributes a bucket entry once per fire time;
/// duplicates within the same bucket merge under `count` but the job key
/// only appears once in `jobs`.
pub fn compute_forecast(
    triggers: &HashMap<String, Trigger>,
    now: DateTime<Utc>,
    window_minutes: u32,
    bucket_minutes: u32,
) -> ForecastResponse {
    let window = window_minutes.min(240) as i64;
    let bucket_size = bucket_minutes.max(1) as i64;
    let end = now + Duration::minutes(window);
    let num_buckets = (window / bucket_size) as usize;

    let mut buckets: Vec<ForecastBucket> = (0..num_buckets)
        .map(|i| {
            let start = now + Duration::minutes(i as i64 * bucket_size);
            let end = start + Duration::minutes(bucket_size);
            ForecastBucket {
                start,
                end,
                count: 0,
                jobs: Vec::new(),
            }
        })
        .collect();

    for (job_key, trigger) in triggers {
        let mut candidate = trigger.next_fire_at;
        let mut iterations = 0;
        while let Some(fire_at) = candidate {
            if fire_at >= end || iterations > 500 {
                break;
            }
            iterations += 1;

            if fire_at >= now {
                let offset = (fire_at - now).num_minutes();
                let bucket_idx = (offset / bucket_size) as usize;
                if bucket_idx < buckets.len() {
                    buckets[bucket_idx].count += 1;
                    if !buckets[bucket_idx].jobs.contains(job_key) {
                        buckets[bucket_idx].jobs.push(job_key.clone());
                    }
                }
            }

            candidate = trigger.schedule.next_fire_after(fire_at, &trigger.timezone);
        }
    }

    ForecastResponse {
        window_minutes,
        bucket_minutes: bucket_size as u32,
        buckets,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_triggers_empty_buckets() {
        let triggers = HashMap::new();
        let result = compute_forecast(&triggers, Utc::now(), 60, 5);
        assert_eq!(result.buckets.len(), 12);
        assert!(result.buckets.iter().all(|b| b.count == 0));
    }
}

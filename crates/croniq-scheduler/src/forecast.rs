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
    /// Whole minutes per bucket — `0` for a sub-minute bucket, whose size is
    /// in [`Self::bucket_seconds`].
    pub bucket_minutes: u32,
    /// The bucket size in seconds, always set (issue #829).
    pub bucket_seconds: u32,
    pub buckets: Vec<ForecastBucket>,
}

/// Smallest sub-minute bucket: a fire every five seconds is as fine as the
/// dashboard's live views draw.
pub const MIN_BUCKET_SECONDS: u32 = 5;

/// Most buckets one forecast's window is cut into. A four-hour window in
/// five-second buckets would be 2880; the bucket grows instead. The grid adds
/// one more (see [`compute_forecast_seconds`]).
pub const MAX_BUCKETS: u32 = 720;

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
    compute_forecast_seconds(
        triggers,
        now,
        window_minutes,
        bucket_minutes.max(1).saturating_mul(60),
    )
}

/// [`compute_forecast`] with the bucket size in seconds (issue #829), for the
/// dashboard's live range selector, which draws the next few minutes at the
/// resolution it draws the last few.
///
/// The bucket is floored at [`MIN_BUCKET_SECONDS`] and grown so the window
/// never yields more than [`MAX_BUCKETS`] buckets.
///
/// Buckets sit on a fixed grid, whole multiples of the bucket size since the
/// epoch, not on `now`. Anchored at `now`, every request cut the schedule at
/// different places: two fires three seconds apart shared a bucket on one
/// poll and were split on the next, and a chart drawn from it jumped with
/// every refetch. On the grid a fire stays in its bucket for as long as it is
/// ahead. The first bucket therefore starts up to one bucket before `now` and
/// counts only what is still to come, and there is always one bucket more
/// than the window holds, so the last one is whole and the count is steady.
pub fn compute_forecast_seconds(
    triggers: &HashMap<String, Trigger>,
    now: DateTime<Utc>,
    window_minutes: u32,
    bucket_seconds: u32,
) -> ForecastResponse {
    let window_minutes = window_minutes.min(240);
    let window_secs = i64::from(window_minutes) * 60;
    let bucket_secs = i64::from(
        bucket_seconds
            .max(MIN_BUCKET_SECONDS)
            .max((window_minutes * 60).div_ceil(MAX_BUCKETS)),
    );
    let num_buckets = (window_secs / bucket_secs) as usize + 1;
    let bucket_size_ms = bucket_secs * 1000;
    let grid_start = DateTime::<Utc>::from_timestamp_millis(
        now.timestamp_millis().div_euclid(bucket_size_ms) * bucket_size_ms,
    )
    .unwrap_or(now);
    let end = grid_start + Duration::seconds(num_buckets as i64 * bucket_secs);

    let mut buckets: Vec<ForecastBucket> = (0..num_buckets)
        .map(|i| {
            let start = grid_start + Duration::seconds(i as i64 * bucket_secs);
            let end = start + Duration::seconds(bucket_secs);
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
                let offset = (fire_at - grid_start).num_milliseconds();
                let bucket_idx = (offset / bucket_size_ms) as usize;
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
        bucket_minutes: if bucket_secs % 60 == 0 {
            (bucket_secs / 60) as u32
        } else {
            0
        },
        bucket_seconds: bucket_secs as u32,
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
        assert_eq!(result.buckets.len(), 13);
        assert!(result.buckets.iter().all(|b| b.count == 0));
        assert_eq!(result.bucket_minutes, 5);
        assert_eq!(result.bucket_seconds, 300);
    }

    #[test]
    fn seconds_buckets_place_each_fire_in_its_slice() {
        use crate::misfire::MisfirePolicy;
        use crate::schedule::Schedule;
        // On the grid, so the slices are the ones the offsets name.
        let now = DateTime::from_timestamp(1_790_000_000, 0).unwrap();
        let mut trigger = Trigger::new(
            "a:b".into(),
            Schedule::Interval { seconds: 10 },
            chrono_tz::UTC,
            None,
            None,
            MisfirePolicy::FireNow,
            now,
        );
        trigger.next_fire_at = Some(now + Duration::seconds(7));
        let triggers = HashMap::from([("a:b".to_string(), trigger)]);

        let result = compute_forecast_seconds(&triggers, now, 1, 5);
        assert_eq!(result.bucket_seconds, 5);
        assert_eq!(result.bucket_minutes, 0);
        assert_eq!(result.buckets.len(), 13);
        // Fires at +7 s, +17 s, … +57 s: one in every other five-second slice.
        let hits: Vec<usize> = (0..13).filter(|&i| result.buckets[i].count > 0).collect();
        assert_eq!(hits, vec![1, 3, 5, 7, 9, 11]);
    }

    /// The bug the grid fixes: asked again a moment later, the same fires
    /// must land in the same buckets, not be regrouped around the new `now`.
    #[test]
    fn buckets_stay_put_as_now_moves() {
        use crate::misfire::MisfirePolicy;
        use crate::schedule::Schedule;
        let t0 = DateTime::from_timestamp(1_790_000_000, 0).unwrap();
        let mut trigger = Trigger::new(
            "a:b".into(),
            Schedule::Interval { seconds: 3 },
            chrono_tz::UTC,
            None,
            None,
            MisfirePolicy::FireNow,
            t0,
        );
        trigger.next_fire_at = Some(t0 + Duration::seconds(20));
        let triggers = HashMap::from([("a:b".to_string(), trigger)]);

        let slices = |now: DateTime<Utc>| -> Vec<(DateTime<Utc>, u32)> {
            let result = compute_forecast_seconds(&triggers, now, 1, 5);
            assert_eq!(result.buckets.len(), 13);
            result
                .buckets
                .iter()
                .map(|b| (b.start, b.count))
                .filter(|(start, _)| {
                    *start >= t0 + Duration::seconds(20) && *start < t0 + Duration::seconds(55)
                })
                .collect()
        };
        let first = slices(t0);
        assert!(first.iter().all(|(start, _)| start.timestamp() % 5 == 0));
        for ms in [1_300, 2_700, 4_100] {
            assert_eq!(
                slices(t0 + Duration::milliseconds(ms)),
                first,
                "at +{ms} ms"
            );
        }
    }

    #[test]
    fn the_first_bucket_counts_only_what_is_still_ahead() {
        use crate::misfire::MisfirePolicy;
        use crate::schedule::Schedule;
        let grid = DateTime::from_timestamp(1_790_000_000, 0).unwrap();
        let now = grid + Duration::seconds(3);
        let mut trigger = Trigger::new(
            "a:b".into(),
            Schedule::Interval { seconds: 1 },
            chrono_tz::UTC,
            None,
            None,
            MisfirePolicy::FireNow,
            grid,
        );
        trigger.next_fire_at = Some(grid + Duration::seconds(1));
        let triggers = HashMap::from([("a:b".to_string(), trigger)]);

        let result = compute_forecast_seconds(&triggers, now, 1, 5);
        assert_eq!(result.buckets[0].start, grid);
        // +1 s and +2 s are behind `now`; +3 s and +4 s are not.
        assert_eq!(result.buckets[0].count, 2);
        assert_eq!(result.buckets[1].count, 5);
    }

    #[test]
    fn seconds_buckets_grow_to_stay_under_the_cap() {
        let result = compute_forecast_seconds(&HashMap::new(), Utc::now(), 240, 5);
        assert!(result.buckets.len() <= MAX_BUCKETS as usize + 1);
        assert_eq!(result.bucket_seconds, 20);
    }
}

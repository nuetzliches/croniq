//! Misfire policy: determines what happens when a trigger fires late.
//!
//! A misfire occurs when the scheduler detects that a fire time has passed
//! without the job being executed (e.g., scheduler was down, or a long GC pause).

use chrono::{DateTime, Duration, Utc};

/// What to do when a fire time is missed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum MisfirePolicy {
    /// Fire immediately, regardless of how late.
    /// Good for: jobs that must always run (billing, data sync).
    FireNow,

    /// Skip the missed fire and wait for the next one.
    /// Good for: high-frequency jobs where missing one isn't critical (health checks).
    Skip,

    /// Fire only if the misfire is within a grace period.
    /// If missed by more than the grace period, skip to next fire.
    GracePeriod {
        /// Maximum delay in seconds before skipping.
        max_delay_secs: u64,
    },
}

impl Default for MisfirePolicy {
    fn default() -> Self {
        MisfirePolicy::FireNow
    }
}

/// Result of evaluating a misfire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MisfireAction {
    /// Execute the job now.
    Fire,
    /// Skip this fire time.
    Skip,
}

impl MisfirePolicy {
    /// Evaluate whether a late fire should execute or be skipped.
    ///
    /// - `fire_at`: the scheduled fire time
    /// - `now`: the current time
    pub fn evaluate(&self, fire_at: DateTime<Utc>, now: DateTime<Utc>) -> MisfireAction {
        if now <= fire_at {
            // Not late — always fire.
            return MisfireAction::Fire;
        }

        match self {
            MisfirePolicy::FireNow => MisfireAction::Fire,

            MisfirePolicy::Skip => MisfireAction::Skip,

            MisfirePolicy::GracePeriod { max_delay_secs } => {
                let delay = now - fire_at;
                if delay <= Duration::seconds(*max_delay_secs as i64) {
                    MisfireAction::Fire
                } else {
                    MisfireAction::Skip
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn utc(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, min, 0).unwrap()
    }

    #[test]
    fn fire_now_always_fires() {
        let policy = MisfirePolicy::FireNow;
        let fire_at = utc(2026, 3, 29, 2, 0);

        // 1 hour late
        assert_eq!(
            policy.evaluate(fire_at, utc(2026, 3, 29, 3, 0)),
            MisfireAction::Fire
        );

        // 1 day late
        assert_eq!(
            policy.evaluate(fire_at, utc(2026, 3, 30, 2, 0)),
            MisfireAction::Fire
        );
    }

    #[test]
    fn skip_always_skips() {
        let policy = MisfirePolicy::Skip;
        let fire_at = utc(2026, 3, 29, 2, 0);

        assert_eq!(
            policy.evaluate(fire_at, utc(2026, 3, 29, 2, 1)),
            MisfireAction::Skip
        );
    }

    #[test]
    fn skip_fires_if_on_time() {
        let policy = MisfirePolicy::Skip;
        let fire_at = utc(2026, 3, 29, 2, 0);

        assert_eq!(
            policy.evaluate(fire_at, utc(2026, 3, 29, 2, 0)),
            MisfireAction::Fire
        );
    }

    #[test]
    fn grace_period_fires_within_window() {
        let policy = MisfirePolicy::GracePeriod {
            max_delay_secs: 300,
        };
        let fire_at = utc(2026, 3, 29, 2, 0);

        // 3 minutes late (within 5 min grace)
        assert_eq!(
            policy.evaluate(fire_at, utc(2026, 3, 29, 2, 3)),
            MisfireAction::Fire
        );
    }

    #[test]
    fn grace_period_skips_past_window() {
        let policy = MisfirePolicy::GracePeriod {
            max_delay_secs: 300,
        };
        let fire_at = utc(2026, 3, 29, 2, 0);

        // 10 minutes late (beyond 5 min grace)
        assert_eq!(
            policy.evaluate(fire_at, utc(2026, 3, 29, 2, 10)),
            MisfireAction::Skip
        );
    }

    #[test]
    fn on_time_always_fires() {
        for policy in [
            MisfirePolicy::FireNow,
            MisfirePolicy::Skip,
            MisfirePolicy::GracePeriod {
                max_delay_secs: 60,
            },
        ] {
            let fire_at = utc(2026, 3, 29, 2, 0);
            assert_eq!(policy.evaluate(fire_at, fire_at), MisfireAction::Fire);
        }
    }
}

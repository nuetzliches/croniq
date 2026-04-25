//! Trigger state machine: manages the lifecycle of a job's schedule.
//!
//! A trigger is the runtime representation of a job's schedule. It tracks when
//! the job should next fire, and transitions through states as executions occur.

use chrono::{DateTime, NaiveTime, Utc};
use chrono_tz::Tz;

use crate::calendar::Calendar;
use crate::misfire::MisfirePolicy;
use crate::schedule::Schedule;

/// Trigger state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum TriggerState {
    /// Waiting for next fire time.
    Armed,
    /// Fire time reached, execution queued.
    Fired,
    /// Job completed successfully for this fire.
    Completed,
    /// Trigger is paused (manual or disabled in config).
    Paused,
    /// Once-trigger has fired and is exhausted.
    Exhausted,
}

/// Runtime trigger for a job.
#[derive(Debug, Clone)]
pub struct Trigger {
    pub job_key: String,
    pub schedule: Schedule,
    pub timezone: Tz,
    pub calendar: Option<Calendar>,
    pub window: Option<TimeWindow>,
    pub misfire_policy: MisfirePolicy,

    /// Job must not fire before this time.
    pub not_before: Option<DateTime<Utc>>,
    /// Job must not fire after this time (trigger becomes Exhausted).
    pub not_after: Option<DateTime<Utc>>,

    // State
    pub state: TriggerState,
    pub next_fire_at: Option<DateTime<Utc>>,
    pub last_fired_at: Option<DateTime<Utc>>,
    pub last_completed_at: Option<DateTime<Utc>>,
    pub fire_count: u64,
}

/// A daily time window constraint.
#[derive(Debug, Clone)]
pub struct TimeWindow {
    pub start: NaiveTime,
    pub end: NaiveTime,
}

impl TimeWindow {
    pub fn parse(s: &str) -> Option<Self> {
        let (start_str, end_str) = s.split_once("..")?;
        let start = parse_time(start_str)?;
        let end = parse_time(end_str)?;
        Some(TimeWindow { start, end })
    }

    /// Check if a time falls within this window.
    pub fn contains(&self, time: NaiveTime) -> bool {
        if self.start <= self.end {
            time >= self.start && time < self.end
        } else {
            time >= self.start || time < self.end
        }
    }
}

impl Trigger {
    /// Create a new trigger, computing the initial next fire time.
    pub fn new(
        job_key: String,
        schedule: Schedule,
        timezone: Tz,
        calendar: Option<Calendar>,
        window: Option<TimeWindow>,
        misfire_policy: MisfirePolicy,
        now: DateTime<Utc>,
    ) -> Self {
        Self::with_bounds(
            job_key,
            schedule,
            timezone,
            calendar,
            window,
            misfire_policy,
            None,
            None,
            now,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_bounds(
        job_key: String,
        schedule: Schedule,
        timezone: Tz,
        calendar: Option<Calendar>,
        window: Option<TimeWindow>,
        misfire_policy: MisfirePolicy,
        not_before: Option<DateTime<Utc>>,
        not_after: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> Self {
        let mut trigger = Trigger {
            job_key,
            schedule,
            timezone,
            calendar,
            window,
            misfire_policy,
            not_before,
            not_after,
            state: TriggerState::Armed,
            next_fire_at: None,
            last_fired_at: None,
            last_completed_at: None,
            fire_count: 0,
        };

        // Check if schedule is disabled
        if matches!(trigger.schedule, Schedule::Disabled) {
            trigger.state = TriggerState::Paused;
        } else {
            trigger.next_fire_at = trigger.compute_next_fire(now);
            if trigger.next_fire_at.is_none() {
                trigger.state = TriggerState::Exhausted;
            }
        }

        trigger
    }

    /// Evaluate whether this trigger should fire at the given time.
    /// Returns `Some(fire_at)` if it should fire, `None` otherwise.
    pub fn evaluate(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        if self.state != TriggerState::Armed {
            return None;
        }

        // Respect not_before: suppress firing until the boundary
        if let Some(nb) = self.not_before
            && now < nb
        {
            return None;
        }

        // Respect not_after: suppress firing past the boundary
        if let Some(na) = self.not_after
            && now > na
        {
            return None;
        }

        let fire_at = self.next_fire_at?;

        if now >= fire_at { Some(fire_at) } else { None }
    }

    /// Mark this trigger as fired and advance to the next fire time.
    ///
    /// The trigger transitions back to Armed immediately (not waiting for
    /// execution completion) so it can fire again on its next schedule.
    /// Execution lifecycle is tracked independently via the Execution entity.
    pub fn mark_fired(&mut self, fire_at: DateTime<Utc>, now: DateTime<Utc>) {
        self.last_fired_at = Some(fire_at);
        self.fire_count += 1;

        // Compute next fire time
        self.next_fire_at = self.compute_next_fire(now);

        // Determine new state
        if self.next_fire_at.is_none() {
            // No more fire times (once-trigger or schedule exhausted)
            self.state = TriggerState::Exhausted;
        } else if let (Some(next), Some(na)) = (self.next_fire_at, self.not_after) {
            if next > na {
                self.next_fire_at = None;
                self.state = TriggerState::Exhausted;
            } else {
                self.state = TriggerState::Armed;
            }
        } else {
            self.state = TriggerState::Armed;
        }
    }

    /// Mark the execution as completed, transition back to Armed.
    pub fn mark_completed(&mut self, now: DateTime<Utc>) {
        self.last_completed_at = Some(now);

        if self.next_fire_at.is_some() {
            self.state = TriggerState::Armed;
        } else {
            self.state = TriggerState::Exhausted;
        }
    }

    /// Mark the execution as failed. Trigger returns to Armed for next fire.
    /// (Retry logic is handled by the execution pipeline, not the trigger.)
    pub fn mark_failed(&mut self, _now: DateTime<Utc>) {
        if self.next_fire_at.is_some() {
            self.state = TriggerState::Armed;
        } else {
            self.state = TriggerState::Exhausted;
        }
    }

    /// Pause this trigger.
    pub fn pause(&mut self) {
        self.state = TriggerState::Paused;
    }

    /// Resume a paused trigger, recomputing next fire time.
    pub fn resume(&mut self, now: DateTime<Utc>) {
        self.next_fire_at = self.compute_next_fire(now);
        if self.next_fire_at.is_some() {
            self.state = TriggerState::Armed;
        } else {
            self.state = TriggerState::Exhausted;
        }
    }

    /// Compute the next valid fire time, respecting calendar and window constraints.
    fn compute_next_fire(&self, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let mut candidate = self.schedule.next_fire_after(after, &self.timezone)?;

        // Apply calendar and window filtering — try up to 366 candidates
        // (worst case: every day excluded except one)
        for _ in 0..366 {
            let local = candidate.with_timezone(&self.timezone);
            let date = local.date_naive();
            let time = local.time();

            // Check calendar
            let calendar_ok = self
                .calendar
                .as_ref()
                .map(|c| c.is_allowed(date, time))
                .unwrap_or(true);

            // Check window
            let window_ok = self
                .window
                .as_ref()
                .map(|w| w.contains(time))
                .unwrap_or(true);

            if calendar_ok && window_ok {
                return Some(candidate);
            }

            // Skip to next candidate
            candidate = self.schedule.next_fire_after(candidate, &self.timezone)?;
        }

        None // All candidates excluded
    }
}

fn parse_time(s: &str) -> Option<NaiveTime> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 {
        return None;
    }
    let h: u32 = parts[0].parse().ok()?;
    let m: u32 = parts[1].parse().ok()?;
    NaiveTime::from_hms_opt(h, m, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule::Schedule;
    use chrono::{NaiveTime, TimeZone};

    fn utc(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, min, 0).unwrap()
    }

    fn tz_utc() -> Tz {
        "UTC".parse().unwrap()
    }

    fn make_trigger(schedule: Schedule, now: DateTime<Utc>) -> Trigger {
        Trigger::new(
            "test:job".into(),
            schedule,
            tz_utc(),
            None,
            None,
            MisfirePolicy::FireNow,
            now,
        )
    }

    #[test]
    fn armed_trigger_fires_when_due() {
        let now = utc(2026, 3, 29, 0, 0);
        let trigger = make_trigger(Schedule::Interval { seconds: 300 }, now);

        assert_eq!(trigger.state, TriggerState::Armed);
        assert!(trigger.next_fire_at.is_some());

        // Before fire time
        assert!(trigger.evaluate(utc(2026, 3, 29, 0, 3)).is_none());

        // At/after fire time
        let fire = trigger.evaluate(utc(2026, 3, 29, 0, 5));
        assert!(fire.is_some());
    }

    #[test]
    fn trigger_lifecycle() {
        let now = utc(2026, 3, 29, 0, 0);
        let mut trigger = make_trigger(
            Schedule::Daily {
                time: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            },
            now,
        );

        assert_eq!(trigger.state, TriggerState::Armed);
        assert_eq!(trigger.next_fire_at, Some(utc(2026, 3, 29, 9, 0)));

        // Fire — trigger goes directly back to Armed (async execution model)
        let fire_at = trigger.evaluate(utc(2026, 3, 29, 9, 0)).unwrap();
        trigger.mark_fired(fire_at, utc(2026, 3, 29, 9, 0));
        assert_eq!(trigger.state, TriggerState::Armed);
        assert_eq!(trigger.fire_count, 1);

        // Next fire time should be tomorrow
        assert_eq!(trigger.next_fire_at, Some(utc(2026, 3, 30, 9, 0)));
    }

    #[test]
    fn once_trigger_exhausts_after_fire() {
        let now = utc(2026, 3, 29, 0, 0);
        let mut trigger = make_trigger(
            Schedule::Once {
                at: utc(2026, 4, 1, 3, 0),
            },
            now,
        );

        assert_eq!(trigger.state, TriggerState::Armed);

        let fire_at = trigger.evaluate(utc(2026, 4, 1, 3, 0)).unwrap();
        trigger.mark_fired(fire_at, utc(2026, 4, 1, 3, 0));

        // No next fire
        assert!(trigger.next_fire_at.is_none());

        trigger.mark_completed(utc(2026, 4, 1, 3, 30));
        assert_eq!(trigger.state, TriggerState::Exhausted);
    }

    #[test]
    fn disabled_trigger_is_paused() {
        let trigger = make_trigger(Schedule::Disabled, utc(2026, 3, 29, 0, 0));
        assert_eq!(trigger.state, TriggerState::Paused);
        assert!(trigger.evaluate(utc(2026, 3, 29, 12, 0)).is_none());
    }

    #[test]
    fn pause_and_resume() {
        let now = utc(2026, 3, 29, 0, 0);
        let mut trigger = make_trigger(Schedule::Interval { seconds: 60 }, now);

        assert_eq!(trigger.state, TriggerState::Armed);
        trigger.pause();
        assert_eq!(trigger.state, TriggerState::Paused);
        assert!(trigger.evaluate(utc(2026, 3, 29, 1, 0)).is_none());

        trigger.resume(utc(2026, 3, 29, 1, 0));
        assert_eq!(trigger.state, TriggerState::Armed);
        assert!(trigger.next_fire_at.is_some());
    }

    #[test]
    fn calendar_excludes_fire_times() {
        use crate::calendar::{Calendar, CalendarRule};

        let calendar = Calendar {
            name: "no-weekends".into(),
            timezone: None,
            includes: vec![CalendarRule::Weekly(vec![
                chrono::Weekday::Mon,
                chrono::Weekday::Tue,
                chrono::Weekday::Wed,
                chrono::Weekday::Thu,
                chrono::Weekday::Fri,
            ])],
            excludes: vec![],
        };

        let now = utc(2026, 3, 28, 0, 0); // Saturday
        let trigger = Trigger::new(
            "test:job".into(),
            Schedule::Daily {
                time: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            },
            tz_utc(),
            Some(calendar),
            None,
            MisfirePolicy::FireNow,
            now,
        );

        // Should skip Saturday and Sunday, fire on Monday
        assert_eq!(trigger.next_fire_at, Some(utc(2026, 3, 30, 9, 0)));
    }

    #[test]
    fn window_constrains_fire_times() {
        let now = utc(2026, 3, 29, 0, 0);
        let window = TimeWindow {
            start: NaiveTime::from_hms_opt(2, 0, 0).unwrap(),
            end: NaiveTime::from_hms_opt(6, 0, 0).unwrap(),
        };

        // Schedule fires at 09:00 but window is 02:00..06:00
        let trigger = Trigger::new(
            "test:job".into(),
            Schedule::Daily {
                time: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            },
            tz_utc(),
            None,
            Some(window),
            MisfirePolicy::FireNow,
            now,
        );

        // 09:00 is outside window → no valid fire time
        assert!(trigger.next_fire_at.is_none());
    }

    #[test]
    fn window_allows_matching_time() {
        let now = utc(2026, 3, 29, 0, 0);
        let window = TimeWindow {
            start: NaiveTime::from_hms_opt(2, 0, 0).unwrap(),
            end: NaiveTime::from_hms_opt(6, 0, 0).unwrap(),
        };

        let trigger = Trigger::new(
            "test:job".into(),
            Schedule::Daily {
                time: NaiveTime::from_hms_opt(3, 0, 0).unwrap(),
            },
            tz_utc(),
            None,
            Some(window),
            MisfirePolicy::FireNow,
            now,
        );

        assert_eq!(trigger.next_fire_at, Some(utc(2026, 3, 29, 3, 0)));
    }
}

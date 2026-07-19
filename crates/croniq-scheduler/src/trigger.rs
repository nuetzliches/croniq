//! Trigger state machine: manages the lifecycle of a job's schedule.
//!
//! A trigger is the runtime representation of a job's schedule. It tracks when
//! the job should next fire, and transitions through states as executions occur.

use chrono::{DateTime, Duration, NaiveTime, Utc};
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

    /// True when the calendar and window gate permit firing at `at`
    /// (localized to the trigger's timezone).
    pub fn gate_allows(&self, at: DateTime<Utc>) -> bool {
        let local = at.with_timezone(&self.timezone);
        let date = local.date_naive();
        let time = local.time();

        let calendar_ok = self
            .calendar
            .as_ref()
            .map(|c| c.is_allowed(date, time))
            .unwrap_or(true);

        let window_ok = self
            .window
            .as_ref()
            .map(|w| w.contains(time))
            .unwrap_or(true);

        calendar_ok && window_ok
    }

    /// Human-readable description of the gate(s) blocking `at`, or `None`
    /// when the gate is open (or the trigger has no gate). Used by the API
    /// to explain why a job is intentionally idle (#391), e.g.
    /// `calendar 'business-hours'` or `window 08:00..18:00`.
    pub fn gate_closed_reason(&self, at: DateTime<Utc>) -> Option<String> {
        let local = at.with_timezone(&self.timezone);
        let date = local.date_naive();
        let time = local.time();

        let mut reasons = Vec::new();
        if let Some(c) = &self.calendar
            && !c.is_allowed(date, time)
        {
            reasons.push(format!("calendar '{}'", c.name));
        }
        if let Some(w) = &self.window
            && !w.contains(time)
        {
            reasons.push(format!(
                "window {}..{}",
                w.start.format("%H:%M"),
                w.end.format("%H:%M")
            ));
        }
        if reasons.is_empty() {
            None
        } else {
            Some(reasons.join(" and "))
        }
    }

    /// Earliest local instant `>= from_local` where both the calendar and
    /// the trigger-level window are open. `None` = never within the scan
    /// horizon (genuinely exhausted).
    fn next_gate_open(&self, from_local: chrono::NaiveDateTime) -> Option<chrono::NaiveDateTime> {
        crate::calendar::next_instant_in(from_local, |date| {
            let mut set = match &self.calendar {
                Some(c) => c.allowed_intervals_on(date),
                None => vec![(0, 86_400)],
            };
            if let Some(w) = &self.window {
                set = crate::calendar::intersect_intervals(
                    &set,
                    &crate::calendar::window_intervals(w.start, w.end),
                );
            }
            set
        })
    }

    /// Compute the next valid fire time, respecting calendar and window constraints.
    ///
    /// When a candidate is gate-blocked, jump straight to the next gate-open
    /// instant instead of stepping raw schedule ticks (#391 — the old
    /// 366-tick walk gave `every 1 minute` a ~6h horizon, so any overnight
    /// calendar gap wrongly exhausted the trigger).
    fn compute_next_fire(&self, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
        // Counts gate JUMPS, not schedule ticks. Wall-clock schedules fire at
        // most once per day, so each iteration advances >= 1 day; 1500
        // iterations out-lasts the gate scan horizon (MAX_SCAN_DAYS) before
        // declaring exhaustion.
        const MAX_GATE_JUMPS: u32 = 1500;

        let mut candidate = self.schedule.next_fire_after(after, &self.timezone)?;
        for _ in 0..MAX_GATE_JUMPS {
            if self.gate_allows(candidate) {
                return Some(candidate);
            }

            let local = candidate.with_timezone(&self.timezone).naive_local();
            let open_local = self.next_gate_open(local)?; // None => genuinely exhausted
            let open_utc = crate::schedule::resolve_local(&self.timezone, open_local)?;

            candidate = match &self.schedule {
                // No wall-clock anchor: the first fire after a closed gate is
                // the opening instant itself (matches FireNow semantics —
                // "run as soon as permitted"); the cadence re-anchors there.
                Schedule::Interval { .. } => open_utc,
                // Wall-clock schedules re-derive their own next tick at or
                // after the opening (a daily-09:00 job fires at 09:00, never
                // at window-open 08:00). next_fire_after is strictly-after,
                // so step back 1s to make open_utc itself eligible.
                _ => self
                    .schedule
                    .next_fire_after(open_utc - Duration::seconds(1), &self.timezone)?,
            };
            // Loop re-verifies gate_allows(candidate): a DST roll-forward in
            // resolve_local can land outside the window, and a wall-clock
            // tick can miss the open period entirely — both re-jump.
        }

        None // No gate-open schedule tick within the scan horizon
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

    // ─── Gate-jump advancement (#391) ───

    /// Mon–Fri 08:00..18:00 — the business-hours shape from issue #391.
    fn business_hours_calendar() -> crate::calendar::Calendar {
        use crate::calendar::CalendarRule;
        crate::calendar::Calendar {
            name: "business-hours".into(),
            timezone: None,
            includes: vec![
                CalendarRule::Weekly(vec![
                    chrono::Weekday::Mon,
                    chrono::Weekday::Tue,
                    chrono::Weekday::Wed,
                    chrono::Weekday::Thu,
                    chrono::Weekday::Fri,
                ]),
                CalendarRule::Window(
                    NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
                    NaiveTime::from_hms_opt(18, 0, 0).unwrap(),
                ),
            ],
            excludes: vec![],
        }
    }

    /// THE #391 regression: `every 1 minute { calendar business-hours }`
    /// must advance from the last in-window fire straight to the next
    /// window open — the old 366-tick walk (~6h horizon for 1-minute
    /// intervals) exhausted the trigger on any overnight gap.
    #[test]
    fn interval_overnight_gap_advances_to_window_open() {
        let now = utc(2026, 3, 30, 17, 58); // Monday, just before close
        let mut trigger = Trigger::new(
            "test:job".into(),
            Schedule::Interval { seconds: 60 },
            tz_utc(),
            Some(business_hours_calendar()),
            None,
            MisfirePolicy::FireNow,
            now,
        );
        // Still inside the window: plain interval advancement.
        assert_eq!(trigger.next_fire_at, Some(utc(2026, 3, 30, 17, 59)));

        let fire_at = utc(2026, 3, 30, 17, 59);
        trigger.mark_fired(fire_at, fire_at);
        assert_eq!(trigger.state, TriggerState::Armed);
        assert_eq!(trigger.next_fire_at, Some(utc(2026, 3, 31, 8, 0)));
    }

    #[test]
    fn interval_weekend_gap_advances_to_monday() {
        use crate::calendar::CalendarRule;
        let calendar = crate::calendar::Calendar {
            name: "weekdays".into(),
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
        // Friday 2026-04-03 23:59 — a ~48h gap that overwhelmed the old
        // 366-tick horizon even for 5-minute intervals.
        let now = utc(2026, 4, 3, 23, 59);
        let trigger = Trigger::new(
            "test:job".into(),
            Schedule::Interval { seconds: 300 },
            tz_utc(),
            Some(calendar),
            None,
            MisfirePolicy::FireNow,
            now,
        );
        assert_eq!(trigger.state, TriggerState::Armed);
        assert_eq!(trigger.next_fire_at, Some(utc(2026, 4, 6, 0, 0)));
    }

    #[test]
    fn trigger_created_during_closed_gate_arms_with_future_fire() {
        // Saturday noon: before #391 this constructed straight into Exhausted.
        let now = utc(2026, 3, 28, 12, 0);
        let trigger = Trigger::new(
            "test:job".into(),
            Schedule::Interval { seconds: 60 },
            tz_utc(),
            Some(business_hours_calendar()),
            None,
            MisfirePolicy::FireNow,
            now,
        );
        assert_eq!(trigger.state, TriggerState::Armed);
        assert_eq!(trigger.next_fire_at, Some(utc(2026, 3, 30, 8, 0)));
    }

    #[test]
    fn wall_clock_rederives_own_tick_after_gap() {
        use crate::calendar::CalendarRule;
        let calendar = crate::calendar::Calendar {
            name: "mondays".into(),
            timezone: None,
            includes: vec![
                CalendarRule::Weekly(vec![chrono::Weekday::Mon]),
                CalendarRule::Window(
                    NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
                    NaiveTime::from_hms_opt(18, 0, 0).unwrap(),
                ),
            ],
            excludes: vec![],
        };
        // Tuesday: a daily-09:00 job must fire next Monday at 09:00 — its
        // own tick — never at the 08:00 window-open instant.
        let now = utc(2026, 3, 31, 12, 0);
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
        assert_eq!(trigger.next_fire_at, Some(utc(2026, 4, 6, 9, 0)));
    }

    #[test]
    fn once_at_gate_disallowed_stays_exhausted() {
        // `once at` a gate-blocked instant must not silently fire at a
        // different time — it exhausts (pre-#391 semantics preserved).
        let window = TimeWindow {
            start: NaiveTime::from_hms_opt(2, 0, 0).unwrap(),
            end: NaiveTime::from_hms_opt(6, 0, 0).unwrap(),
        };
        let trigger = Trigger::new(
            "test:job".into(),
            Schedule::Once {
                at: utc(2026, 4, 1, 9, 0),
            },
            tz_utc(),
            None,
            Some(window),
            MisfirePolicy::FireNow,
            utc(2026, 3, 29, 0, 0),
        );
        assert!(trigger.next_fire_at.is_none());
        assert_eq!(trigger.state, TriggerState::Exhausted);
    }

    #[test]
    fn calendar_and_trigger_window_intersect() {
        use crate::calendar::CalendarRule;
        let calendar = crate::calendar::Calendar {
            name: "daytime".into(),
            timezone: None,
            includes: vec![CalendarRule::Window(
                NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
                NaiveTime::from_hms_opt(18, 0, 0).unwrap(),
            )],
            excludes: vec![],
        };
        let window = TimeWindow {
            start: NaiveTime::from_hms_opt(12, 0, 0).unwrap(),
            end: NaiveTime::from_hms_opt(14, 0, 0).unwrap(),
        };
        // Gate = calendar ∩ window opens at 12:00.
        let now = utc(2026, 3, 30, 6, 0);
        let trigger = Trigger::new(
            "test:job".into(),
            Schedule::Interval { seconds: 60 },
            tz_utc(),
            Some(calendar),
            Some(window),
            MisfirePolicy::FireNow,
            now,
        );
        assert_eq!(trigger.next_fire_at, Some(utc(2026, 3, 30, 12, 0)));
    }

    #[test]
    fn dst_spring_forward_rolls_window_open_without_exhausting() {
        // Europe/Berlin 2026-03-29: clocks jump 02:00→03:00 local. A window
        // opening at 02:00 resolves to the first existing instant
        // (03:00 CEST = 01:00 UTC) instead of exhausting (#249 precedent).
        let tz: Tz = "Europe/Berlin".parse().unwrap();
        let window = TimeWindow {
            start: NaiveTime::from_hms_opt(2, 0, 0).unwrap(),
            end: NaiveTime::from_hms_opt(6, 0, 0).unwrap(),
        };
        let now = utc(2026, 3, 29, 0, 0); // 01:00 CET local
        let trigger = Trigger::new(
            "test:job".into(),
            Schedule::Interval { seconds: 60 },
            tz,
            None,
            Some(window),
            MisfirePolicy::FireNow,
            now,
        );
        assert_eq!(trigger.state, TriggerState::Armed);
        assert_eq!(trigger.next_fire_at, Some(utc(2026, 3, 29, 1, 0)));
    }

    #[test]
    fn gate_closed_reason_names_the_gate() {
        let now = utc(2026, 3, 28, 12, 0); // Saturday noon

        let plain = make_trigger(Schedule::Interval { seconds: 60 }, now);
        assert!(plain.gate_allows(now));
        assert_eq!(plain.gate_closed_reason(now), None);

        let gated = Trigger::new(
            "test:job".into(),
            Schedule::Interval { seconds: 60 },
            tz_utc(),
            Some(business_hours_calendar()),
            Some(TimeWindow {
                start: NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
                end: NaiveTime::from_hms_opt(18, 0, 0).unwrap(),
            }),
            MisfirePolicy::FireNow,
            now,
        );
        // Saturday noon: calendar closed (weekday rule), trigger window open.
        assert_eq!(
            gated.gate_closed_reason(now),
            Some("calendar 'business-hours'".into())
        );
        // Saturday evening: both gates closed.
        assert_eq!(
            gated.gate_closed_reason(utc(2026, 3, 28, 19, 0)),
            Some("calendar 'business-hours' and window 08:00..18:00".into())
        );
        // Monday noon: open.
        assert!(gated.gate_closed_reason(utc(2026, 3, 30, 12, 0)).is_none());
        assert!(gated.gate_allows(utc(2026, 3, 30, 12, 0)));
    }
}

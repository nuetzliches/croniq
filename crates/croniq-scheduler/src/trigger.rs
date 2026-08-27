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

/// Outcome of [`Trigger::carry_over_pending_fire`] — whether the pending fire
/// time from before a config load survived, and if not, why not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingFire {
    /// The carried-over instant is a valid fire time for this trigger and was
    /// adopted unchanged.
    Adopted,
    /// The carried-over instant is blocked by this trigger's calendar/window
    /// gate (only a pre-#391 build could have written it); `next_fire_at` was
    /// recomputed from `now` and the trigger re-armed.
    HealedGateClosed,
    /// The carried-over instant is later than this schedule's own next fire —
    /// it outlived the schedule that produced it (#535, e.g. a shortened
    /// interval); `next_fire_at` was recomputed from `now`.
    HealedOutlivedSchedule,
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

    /// Adopt a pending fire time carried over from before a config load
    /// (a restart's persisted `job_states` row, or the previous in-memory
    /// trigger on hot-reload) onto this freshly-built trigger.
    ///
    /// Carrying the pending instant over is what keeps a restart from
    /// skipping or double-firing. But the instant was computed under the
    /// *previous* schedule and gates, so it is only adopted when it can still
    /// belong to this one:
    ///
    /// - gate-disallowed (#391): only a pre-#391 build could have written it —
    ///   the fixed `compute_next_fire` never emits one. Recomputed.
    /// - later than this schedule's own next fire from `now` (#535):
    ///   `compute_next_fire` is monotone in its argument, so an instant
    ///   computed at any earlier moment under *this* schedule can never be
    ///   later than `compute_next_fire(now)`. A later one therefore belongs
    ///   to a schedule that no longer applies — typically an interval that
    ///   has since been shortened, which would otherwise stay silent for up
    ///   to the whole *old* interval (a day, for daily → hourly). Recomputed.
    /// - otherwise adopted as-is, past instants included: a gate-allowed
    ///   overdue fire is a missed fire, and `MisfirePolicy::FireNow` catches
    ///   it up once.
    ///
    /// The staleness heal only ever moves the pending fire *earlier* (it
    /// replaces `stored` with a strictly smaller instant), so no pending fire
    /// can be delayed or lost by it.
    ///
    /// `state` is left as the freshly-built trigger set it, except on the
    /// gate heal where `resume` re-arms as it did before this method existed.
    pub fn carry_over_pending_fire(
        &mut self,
        stored: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> PendingFire {
        if !self.gate_allows(stored) {
            self.resume(now);
            return PendingFire::HealedGateClosed;
        }

        if let Some(fresh) = self.compute_next_fire(now)
            && fresh < stored
        {
            self.next_fire_at = Some(fresh);
            return PendingFire::HealedOutlivedSchedule;
        }

        self.next_fire_at = Some(stored);
        PendingFire::Adopted
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

    /// True when the calendar and window gate permit firing at `at`.
    ///
    /// The two gates are read on **different clocks** (issue #450): the
    /// calendar on its own (`Calendar::tz`), because a shared calendar means
    /// the same thing to every job that consults it; the trigger-level
    /// `window` on the job's, because it is a property of this job. They
    /// coincide for the usual single-zone Croniqfile.
    pub fn gate_allows(&self, at: DateTime<Utc>) -> bool {
        let calendar_ok = self
            .calendar
            .as_ref()
            .map(|c| c.is_allowed_at(at))
            .unwrap_or(true);

        let window_ok = self
            .window
            .as_ref()
            .map(|w| w.contains(at.with_timezone(&self.timezone).time()))
            .unwrap_or(true);

        calendar_ok && window_ok
    }

    /// Human-readable description of the gate(s) blocking `at`, or `None`
    /// when the gate is open (or the trigger has no gate). Used by the API
    /// to explain why a job is intentionally idle (#391), e.g.
    /// `calendar 'business-hours'` or `window 08:00..18:00`.
    pub fn gate_closed_reason(&self, at: DateTime<Utc>) -> Option<String> {
        let time = at.with_timezone(&self.timezone).time();

        let mut reasons = Vec::new();
        if let Some(c) = &self.calendar
            && !c.is_allowed_at(at)
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

    /// Earliest UTC instant `>= from` where both the calendar and the
    /// trigger-level window are open. `None` = never within the scan horizon
    /// (genuinely exhausted).
    ///
    /// The two gates live on different clocks since #450, so there is no
    /// single local timeline to intersect interval sets on — a Vienna
    /// 08:00..18:00 window projected into New York is not the same
    /// second-of-day set on every day, and moves at each zone's DST switch.
    /// Instead each gate answers "your next opening at or after `t`" in its
    /// own zone, and this advances `t` to the later of the two and re-asks:
    ///
    /// - a round that does not advance means both gates are open at `t`, which
    ///   is the answer;
    /// - a round that does advance skips only a span that at least one gate
    ///   keeps closed throughout, so no opening can be missed.
    ///
    /// Same result as the old single-zone interval intersection when the zones
    /// coincide, and still O(days-to-opening) — the day-jumping happens inside
    /// each gate's own scan, not by iterating here.
    fn next_gate_open(&self, from: DateTime<Utc>) -> Option<DateTime<Utc>> {
        // Each round advances past at least one gate boundary. Both gates have
        // O(1) boundaries per day, so this only has to out-last the boundaries
        // of a single day; the day-scale search is `next_instant_in`'s job and
        // is bounded by MAX_SCAN_DAYS. The horizon check below is what actually
        // terminates a gate pair that never overlaps.
        const MAX_ROUNDS: u32 = 512;
        let horizon = from + Duration::days(crate::calendar::MAX_SCAN_DAYS as i64);

        let mut t = from;
        for _ in 0..MAX_ROUNDS {
            if t > horizon {
                return None;
            }
            let cal_open = match &self.calendar {
                Some(c) => c.next_open_at_or_after(t)?,
                None => t,
            };
            let window_open = match &self.window {
                Some(w) => next_window_open_at_or_after(&self.timezone, w, t)?,
                None => t,
            };
            let next = cal_open.max(window_open);
            if next <= t {
                return Some(t);
            }
            t = next;
        }
        None
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

            let open_utc = self.next_gate_open(candidate)?; // None => genuinely exhausted

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

/// Earliest UTC instant `>= from` at which a trigger-level `window` is open,
/// evaluated in the job's zone. `None` when the window can never open
/// (`start == end` matches nothing, mirroring `TimeWindow::contains`).
fn next_window_open_at_or_after(
    tz: &Tz,
    window: &TimeWindow,
    from: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let local = from.with_timezone(tz).naive_local();
    let open_local = crate::calendar::next_instant_in(local, |_| {
        crate::calendar::window_intervals(window.start, window.end)
    })?;
    crate::schedule::resolve_local_at_or_after(tz, open_local, from)
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
            tz: chrono_tz::UTC,
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
            tz: chrono_tz::UTC,
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
            tz: chrono_tz::UTC,
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
            tz: chrono_tz::UTC,
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
            tz: chrono_tz::UTC,
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

    // ─── Carrying a pending fire across a config load (#535) ───

    #[test]
    fn unchanged_interval_adopts_pending_fire() {
        // The ordinary restart: the pending instant was computed a moment ago
        // under the same schedule, so it must survive verbatim — recomputing
        // would let a restart loop postpone the job indefinitely.
        let now = utc(2026, 3, 29, 12, 4);
        let mut trigger = make_trigger(Schedule::Interval { seconds: 300 }, now);
        let pending = utc(2026, 3, 29, 12, 7); // last fire 12:02 + 5 min

        assert_eq!(
            trigger.carry_over_pending_fire(pending, now),
            PendingFire::Adopted
        );
        assert_eq!(trigger.next_fire_at, Some(pending));
    }

    #[test]
    fn overdue_pending_fire_is_adopted_not_recomputed() {
        // A gate-allowed instant in the past is a *missed* fire, which
        // MisfirePolicy::FireNow catches up. Recomputing would swallow it.
        let now = utc(2026, 3, 29, 12, 4);
        let mut trigger = make_trigger(Schedule::Interval { seconds: 300 }, now);
        let missed = utc(2026, 3, 29, 11, 50);

        assert_eq!(
            trigger.carry_over_pending_fire(missed, now),
            PendingFire::Adopted
        );
        assert_eq!(trigger.next_fire_at, Some(missed));
    }

    #[test]
    fn shortened_interval_discards_the_old_longer_pending_fire() {
        // Issue #535: 1 hour -> 1 minute. The hourly fire computed before the
        // edit is 41 minutes out; adopting it would keep the job silent that
        // whole time while every API surface reports `every 1 minute`.
        let now = utc(2026, 3, 29, 22, 4);
        let mut trigger = make_trigger(Schedule::Interval { seconds: 60 }, now);
        let pending_hourly = utc(2026, 3, 29, 22, 45);

        assert_eq!(
            trigger.carry_over_pending_fire(pending_hourly, now),
            PendingFire::HealedOutlivedSchedule
        );
        assert_eq!(trigger.next_fire_at, Some(utc(2026, 3, 29, 22, 5)));
    }

    #[test]
    fn lengthened_interval_adopts_the_earlier_pending_fire() {
        // The other direction is not a fault: the pending fire is sooner than
        // the new interval would produce, and firing it is what the old
        // schedule promised. The new cadence takes over from there.
        let now = utc(2026, 3, 29, 12, 0);
        let mut trigger = make_trigger(Schedule::Interval { seconds: 3600 }, now);
        let pending_minutely = utc(2026, 3, 29, 12, 1);

        assert_eq!(
            trigger.carry_over_pending_fire(pending_minutely, now),
            PendingFire::Adopted
        );
        assert_eq!(trigger.next_fire_at, Some(pending_minutely));
    }

    #[test]
    fn wall_clock_schedule_moved_earlier_discards_the_old_pending_fire() {
        // Not interval-specific: daily 23:00 -> daily 13:00, edited at noon.
        let now = utc(2026, 3, 29, 12, 0);
        let mut trigger = make_trigger(
            Schedule::Daily {
                time: NaiveTime::from_hms_opt(13, 0, 0).unwrap(),
            },
            now,
        );
        let pending_2300 = utc(2026, 3, 29, 23, 0);

        assert_eq!(
            trigger.carry_over_pending_fire(pending_2300, now),
            PendingFire::HealedOutlivedSchedule
        );
        assert_eq!(trigger.next_fire_at, Some(utc(2026, 3, 29, 13, 0)));
    }

    #[test]
    fn unchanged_wall_clock_schedule_adopts_its_own_pending_fire() {
        // The recompute must not fire on an untouched wall-clock job: its own
        // next tick equals the stored one, and equality is adopted.
        let now = utc(2026, 3, 29, 12, 0);
        let mut trigger = make_trigger(
            Schedule::Daily {
                time: NaiveTime::from_hms_opt(13, 0, 0).unwrap(),
            },
            now,
        );

        assert_eq!(
            trigger.carry_over_pending_fire(utc(2026, 3, 29, 13, 0), now),
            PendingFire::Adopted
        );
        assert_eq!(trigger.next_fire_at, Some(utc(2026, 3, 29, 13, 0)));
    }

    #[test]
    fn gate_disallowed_pending_fire_is_healed_before_the_staleness_check() {
        // Pre-existing #391 behaviour, kept: a gate-blocked instant is
        // recomputed and the trigger re-armed, whatever its distance.
        let now = utc(2026, 3, 28, 12, 0); // Saturday
        let mut trigger = Trigger::new(
            "test:job".into(),
            Schedule::Interval { seconds: 3600 },
            tz_utc(),
            Some(business_hours_calendar()),
            None,
            MisfirePolicy::FireNow,
            now,
        );
        let saturday_pending = utc(2026, 3, 28, 13, 0);

        assert_eq!(
            trigger.carry_over_pending_fire(saturday_pending, now),
            PendingFire::HealedGateClosed
        );
        assert_eq!(trigger.state, TriggerState::Armed);
        assert_eq!(trigger.next_fire_at, Some(utc(2026, 3, 30, 8, 0)));
    }

    #[test]
    fn exhausted_once_schedule_adopts_its_pending_fire() {
        // `compute_next_fire` returns None for a once-schedule whose instant
        // has passed. That is not evidence of staleness — the pending fire is
        // the one thing left to run.
        let at = utc(2026, 3, 29, 12, 0);
        let now = utc(2026, 3, 29, 12, 5);
        let mut trigger = make_trigger(Schedule::Once { at }, utc(2026, 3, 29, 11, 0));

        assert_eq!(
            trigger.carry_over_pending_fire(at, now),
            PendingFire::Adopted
        );
        assert_eq!(trigger.next_fire_at, Some(at));
    }

    // ─── The calendar's own zone (#450) ───
    //
    // A calendar is evaluated on its own clock, the trigger-level `window` on
    // the job's. Every case below uses a Vienna calendar and a New York job,
    // where the two clocks disagree by 5 or 6 hours depending on the date, so
    // an assertion that passes under the old job-zone behaviour cannot pass
    // here by accident.

    fn vienna() -> Tz {
        chrono_tz::Europe::Vienna
    }

    fn new_york() -> Tz {
        chrono_tz::America::New_York
    }

    fn cal_in(tz: Tz, includes: Vec<crate::calendar::CalendarRule>) -> crate::calendar::Calendar {
        crate::calendar::Calendar {
            name: "business-days".into(),
            tz,
            includes,
            excludes: vec![],
        }
    }

    fn weekdays() -> crate::calendar::CalendarRule {
        crate::calendar::CalendarRule::Weekly(vec![
            chrono::Weekday::Mon,
            chrono::Weekday::Tue,
            chrono::Weekday::Wed,
            chrono::Weekday::Thu,
            chrono::Weekday::Fri,
        ])
    }

    fn window_rule(from_h: u32, to_h: u32) -> crate::calendar::CalendarRule {
        crate::calendar::CalendarRule::Window(
            NaiveTime::from_hms_opt(from_h, 0, 0).unwrap(),
            NaiveTime::from_hms_opt(to_h, 0, 0).unwrap(),
        )
    }

    /// A Vienna `weekly monday..friday` calendar gates the **Vienna** day, so a
    /// New York job at 22:00 is already on the calendar's next day: Friday
    /// 22:00 in New York is Saturday 04:00 in Vienna and stays shut. Under the
    /// old job-zone evaluation this instant was a Friday and passed.
    #[test]
    fn calendar_gates_its_own_weekday_not_the_jobs() {
        let trigger = Trigger::with_bounds(
            "test:job".into(),
            Schedule::Daily {
                time: NaiveTime::from_hms_opt(22, 0, 0).unwrap(),
            },
            new_york(),
            Some(cal_in(vienna(), vec![weekdays()])),
            None,
            MisfirePolicy::FireNow,
            None,
            None,
            utc(2026, 6, 5, 12, 0),
        );

        // Friday 22:00 New York = Saturday 04:00 Vienna — closed.
        assert!(!trigger.gate_allows(utc(2026, 6, 6, 2, 0)));
        // Sunday 22:00 New York = Monday 04:00 Vienna — open.
        assert!(trigger.gate_allows(utc(2026, 6, 8, 2, 0)));
        // So the next fire skips Friday, Saturday and Sunday-as-seen-locally
        // and lands on the tick whose Vienna day is a Monday.
        assert_eq!(trigger.next_fire_at, Some(utc(2026, 6, 8, 2, 0)));
    }

    /// A Vienna `window 08:00..18:00` opens at 02:00 New York in summer, not at
    /// 08:00 New York.
    #[test]
    fn calendar_window_opens_on_the_calendars_clock() {
        let trigger = Trigger::new(
            "test:job".into(),
            Schedule::Interval { seconds: 3600 },
            new_york(),
            Some(cal_in(vienna(), vec![window_rule(8, 18)])),
            None,
            MisfirePolicy::FireNow,
            utc(2026, 6, 10, 3, 0), // 05:00 Vienna — before the window
        );

        // 06:00 UTC = 08:00 Vienna = 02:00 New York.
        assert_eq!(trigger.next_fire_at, Some(utc(2026, 6, 10, 6, 0)));
        assert!(trigger.gate_allows(utc(2026, 6, 10, 6, 0)));
        // 16:00 UTC = 18:00 Vienna: half-open, so already shut — while it is
        // only 12:00 in New York.
        assert!(!trigger.gate_allows(utc(2026, 6, 10, 16, 0)));
    }

    /// The two zones' DST switches are three weeks apart in 2026 (New York on
    /// 03-08, Vienna on 03-29), so between them the gap is 5 hours instead of
    /// 6. The *same* wall-clock job time therefore falls inside a Vienna
    /// window in March and outside it in June — which no single-zone
    /// evaluation, and no fixed-offset shortcut, can reproduce.
    #[test]
    fn calendar_window_follows_each_zones_own_dst() {
        let trigger = Trigger::new(
            "test:job".into(),
            Schedule::Daily {
                time: NaiveTime::from_hms_opt(3, 0, 0).unwrap(),
            },
            new_york(),
            Some(cal_in(vienna(), vec![window_rule(8, 9)])),
            None,
            MisfirePolicy::FireNow,
            utc(2026, 3, 9, 12, 0),
        );

        // 03:00 EDT on 03-10 = 07:00 UTC = 08:00 CET — inside [08:00, 09:00).
        assert!(trigger.gate_allows(utc(2026, 3, 10, 7, 0)));
        // 03:00 EDT on 06-10 = 07:00 UTC = 09:00 CEST — Vienna has moved on by
        // an hour, so the same job time is now shut out.
        assert!(!trigger.gate_allows(utc(2026, 6, 10, 7, 0)));
    }

    /// Calendar and trigger `window` are intersected across two clocks: Vienna
    /// business hours (08:00..18:00 = 02:00..12:00 New York in summer) and a
    /// New York `08:00..18:00` window overlap only from 08:00 to 12:00 New
    /// York time.
    #[test]
    fn calendar_and_window_intersect_across_zones() {
        let trigger = Trigger::new(
            "test:job".into(),
            Schedule::Interval { seconds: 3600 },
            new_york(),
            Some(cal_in(vienna(), vec![window_rule(8, 18)])),
            Some(TimeWindow {
                start: NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
                end: NaiveTime::from_hms_opt(18, 0, 0).unwrap(),
            }),
            MisfirePolicy::FireNow,
            utc(2026, 6, 10, 3, 0),
        );

        // First common opening: 12:00 UTC = 08:00 New York = 14:00 Vienna.
        assert_eq!(trigger.next_fire_at, Some(utc(2026, 6, 10, 12, 0)));
        // 11:00 UTC: Vienna open (13:00) but New York not yet (07:00).
        assert!(!trigger.gate_allows(utc(2026, 6, 10, 11, 0)));
        // 16:00 UTC: New York open (12:00) but Vienna shut (18:00).
        assert!(!trigger.gate_allows(utc(2026, 6, 10, 16, 0)));
        // 15:59 UTC = 11:59 New York / 17:59 Vienna — the last open minute.
        assert!(trigger.gate_allows(utc(2026, 6, 10, 15, 59)));
    }

    /// A gate scan that starts inside a fall-back's repeated hour must not be
    /// handed an opening that lies *before* it. Vienna's 2026 fall-back
    /// repeats 02:00–03:00 local: 02:15 exists at 00:15 UTC (CEST) and again
    /// at 01:15 UTC (CET). Asking from 01:00 UTC has to yield the second one —
    /// the first would move the scan backwards and stall `next_gate_open`
    /// forever.
    #[test]
    fn ambiguous_local_opening_never_moves_the_scan_backwards() {
        let cal = cal_in(
            vienna(),
            vec![crate::calendar::CalendarRule::Window(
                NaiveTime::from_hms_opt(2, 15, 0).unwrap(),
                NaiveTime::from_hms_opt(2, 45, 0).unwrap(),
            )],
        );

        // Both instants really are 02:15 Vienna, one hour apart.
        assert!(cal.is_allowed_at(utc(2026, 10, 25, 0, 15)));
        assert!(cal.is_allowed_at(utc(2026, 10, 25, 1, 15)));

        // From 00:00 UTC the first pass is the answer.
        assert_eq!(
            cal.next_open_at_or_after(utc(2026, 10, 25, 0, 0)),
            Some(utc(2026, 10, 25, 0, 15))
        );
        // From 01:00 UTC — inside the repeated hour, past the first pass —
        // the second one is.
        assert_eq!(
            cal.next_open_at_or_after(utc(2026, 10, 25, 1, 0)),
            Some(utc(2026, 10, 25, 1, 15))
        );

        // And the trigger built on it makes progress rather than hanging.
        let trigger = Trigger::new(
            "test:job".into(),
            Schedule::Interval { seconds: 60 },
            new_york(),
            Some(cal),
            None,
            MisfirePolicy::FireNow,
            utc(2026, 10, 25, 1, 0),
        );
        assert_eq!(trigger.next_fire_at, Some(utc(2026, 10, 25, 1, 15)));
    }

    /// A calendar in the job's own zone must behave exactly as before #450 —
    /// the common single-zone Croniqfile is untouched.
    #[test]
    fn same_zone_calendar_is_unchanged() {
        let trigger = Trigger::new(
            "test:job".into(),
            Schedule::Interval { seconds: 3600 },
            vienna(),
            Some(cal_in(vienna(), vec![weekdays(), window_rule(8, 18)])),
            None,
            MisfirePolicy::FireNow,
            utc(2026, 6, 8, 3, 0), // Monday 05:00 Vienna
        );
        // 06:00 UTC = 08:00 Vienna.
        assert_eq!(trigger.next_fire_at, Some(utc(2026, 6, 8, 6, 0)));
    }
}

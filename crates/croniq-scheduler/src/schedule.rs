//! Schedule evaluation: computes next fire times from compiled schedule definitions.

use chrono::{Datelike, Duration, NaiveDate, NaiveTime, TimeZone};
use chrono_tz::Tz;
use croniq_config::ast::{IntervalUnit, MonthOrdinal, ScheduleKind, Weekday as AstWeekday};

/// A compiled, evaluatable schedule.
#[derive(Debug, Clone)]
pub enum Schedule {
    /// Fire every N seconds/minutes/hours.
    Interval { seconds: u64 },

    /// Fire every day at a specific time.
    Daily { time: NaiveTime },

    /// Fire on specific weekdays at a specific time.
    Weekdays {
        days: Vec<chrono::Weekday>,
        time: NaiveTime,
    },

    /// Fire on specific days of month at a specific time.
    Monthly {
        ordinals: Vec<MonthDay>,
        time: NaiveTime,
    },

    /// Fire once at a specific datetime.
    Once { at: chrono::DateTime<chrono::Utc> },

    /// Disabled — never fires.
    Disabled,
}

/// Day-of-month specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonthDay {
    /// Specific day (1-31).
    Day(u8),
    /// Last day of the month.
    Last,
}

impl Schedule {
    /// Compile from AST schedule kind.
    pub fn from_ast(kind: &ScheduleKind) -> Result<Self, ScheduleError> {
        match kind {
            ScheduleKind::Interval { count, unit } => {
                let seconds = match unit {
                    IntervalUnit::Seconds => *count as u64,
                    IntervalUnit::Minutes => *count as u64 * 60,
                    IntervalUnit::Hours => *count as u64 * 3600,
                };
                if seconds == 0 {
                    return Err(ScheduleError::InvalidInterval(
                        "interval must be > 0".into(),
                    ));
                }
                Ok(Schedule::Interval { seconds })
            }
            ScheduleKind::Daily { time } => {
                let t = NaiveTime::from_hms_opt(time.hour as u32, time.minute as u32, 0)
                    .ok_or_else(|| {
                        ScheduleError::InvalidTime(format!("{}:{}", time.hour, time.minute))
                    })?;
                Ok(Schedule::Daily { time: t })
            }
            ScheduleKind::Weekdays { days, time } => {
                let chrono_days: Vec<chrono::Weekday> =
                    days.iter().map(ast_weekday_to_chrono).collect();
                let t = NaiveTime::from_hms_opt(time.hour as u32, time.minute as u32, 0)
                    .ok_or_else(|| {
                        ScheduleError::InvalidTime(format!("{}:{}", time.hour, time.minute))
                    })?;
                Ok(Schedule::Weekdays {
                    days: chrono_days,
                    time: t,
                })
            }
            ScheduleKind::Monthly { ordinals, time } => {
                let month_days: Vec<MonthDay> = ordinals
                    .iter()
                    .map(|o| match o {
                        MonthOrdinal::Day(d) => MonthDay::Day(*d),
                        MonthOrdinal::Last => MonthDay::Last,
                    })
                    .collect();
                let t = NaiveTime::from_hms_opt(time.hour as u32, time.minute as u32, 0)
                    .ok_or_else(|| {
                        ScheduleError::InvalidTime(format!("{}:{}", time.hour, time.minute))
                    })?;
                Ok(Schedule::Monthly {
                    ordinals: month_days,
                    time: t,
                })
            }
            ScheduleKind::Once { at } => {
                let dt = parse_datetime(&at.value)?;
                Ok(Schedule::Once { at: dt })
            }
            ScheduleKind::Disabled => Ok(Schedule::Disabled),
        }
    }

    /// Compute the next fire time after `after` in the given timezone.
    ///
    /// Returns `None` for disabled schedules or if a once-schedule is in the past.
    pub fn next_fire_after(
        &self,
        after: chrono::DateTime<chrono::Utc>,
        tz: &Tz,
    ) -> Option<chrono::DateTime<chrono::Utc>> {
        match self {
            Schedule::Disabled => None,

            Schedule::Interval { seconds } => {
                let next = after + Duration::seconds(*seconds as i64);
                Some(next)
            }

            Schedule::Daily { time } => {
                let local = after.with_timezone(tz);
                let today = local.date_naive();

                // Try today
                if let Some(candidate) = resolve_local(tz, today.and_time(*time))
                    && candidate > after
                {
                    return Some(candidate);
                }

                // Tomorrow
                let tomorrow = today + Duration::days(1);
                resolve_local(tz, tomorrow.and_time(*time))
            }

            Schedule::Weekdays { days, time } => {
                let local = after.with_timezone(tz);
                let today = local.date_naive();

                // Search up to 8 days ahead (covers current week + 1)
                for offset in 0..8 {
                    let date = today + Duration::days(offset);
                    let weekday = date.weekday();

                    if days.contains(&weekday)
                        && let Some(candidate) = resolve_local(tz, date.and_time(*time))
                        && candidate > after
                    {
                        return Some(candidate);
                    }
                }
                None
            }

            Schedule::Monthly { ordinals, time } => {
                let local = after.with_timezone(tz);
                let today = local.date_naive();

                // Search current month and next 2 months
                for month_offset in 0..3 {
                    let (year, month) = add_months(today.year(), today.month(), month_offset);

                    for ordinal in ordinals {
                        let day = match ordinal {
                            MonthDay::Day(d) => *d as u32,
                            MonthDay::Last => last_day_of_month(year, month),
                        };

                        if let Some(date) = NaiveDate::from_ymd_opt(year, month, day)
                            && let Some(candidate) = resolve_local(tz, date.and_time(*time))
                            && candidate > after
                        {
                            return Some(candidate);
                        }
                    }
                }
                None
            }

            Schedule::Once { at } => {
                if *at > after {
                    Some(*at)
                } else {
                    None
                }
            }
        }
    }

    /// Compute the next N fire times after `after`.
    pub fn next_n_fires(
        &self,
        after: chrono::DateTime<chrono::Utc>,
        tz: &Tz,
        n: usize,
    ) -> Vec<chrono::DateTime<chrono::Utc>> {
        let mut fires = Vec::with_capacity(n);
        let mut cursor = after;

        for _ in 0..n {
            match self.next_fire_after(cursor, tz) {
                Some(next) => {
                    fires.push(next);
                    cursor = next;
                }
                None => break,
            }
        }

        fires
    }

    /// Human-readable summary.
    pub fn summary(&self) -> String {
        match self {
            Schedule::Interval { seconds } => {
                if *seconds >= 3600 && seconds % 3600 == 0 {
                    let n = seconds / 3600;
                    format!("every {n} hour{s}", s = if n == 1 { "" } else { "s" })
                } else if *seconds >= 60 && seconds % 60 == 0 {
                    let n = seconds / 60;
                    format!("every {n} minute{s}", s = if n == 1 { "" } else { "s" })
                } else {
                    format!(
                        "every {seconds} second{s}",
                        s = if *seconds == 1 { "" } else { "s" }
                    )
                }
            }
            Schedule::Daily { time } => {
                format!("every day at {}", time.format("%H:%M"))
            }
            Schedule::Weekdays { days, time } => {
                let names: Vec<&str> = days.iter().map(|d| weekday_name(d)).collect();
                format!("every {} at {}", names.join(", "), time.format("%H:%M"))
            }
            Schedule::Monthly { ordinals, time } => {
                let ords: Vec<String> = ordinals
                    .iter()
                    .map(|o| match o {
                        MonthDay::Day(d) => format_ordinal(*d),
                        MonthDay::Last => "last".into(),
                    })
                    .collect();
                format!(
                    "every {} of month at {}",
                    ords.join(", "),
                    time.format("%H:%M")
                )
            }
            Schedule::Once { at } => format!("once at {}", at.to_rfc3339()),
            Schedule::Disabled => "disabled".into(),
        }
    }
}

// ─── Helpers ───

/// Resolve a local wall-clock datetime to a UTC instant, handling the two
/// DST edge cases explicitly (issue #249):
///
/// - **Gap (spring-forward):** the wall-clock time does not exist (e.g. 02:30
///   on a day the clock jumps 02:00→03:00). Roll forward to the first instant
///   that does exist — effectively the moment the clock jumps to. Returning
///   `None` here is what previously exhausted a daily clock-time trigger
///   permanently the first time its fire time fell in the gap.
/// - **Ambiguous (fall-back):** the wall-clock time occurs twice. Pick the
///   earliest (pre-transition) occurrence deliberately.
fn resolve_local(tz: &Tz, naive: chrono::NaiveDateTime) -> Option<chrono::DateTime<chrono::Utc>> {
    use chrono::MappedLocalTime;
    match tz.from_local_datetime(&naive) {
        MappedLocalTime::Single(dt) => Some(dt.with_timezone(&chrono::Utc)),
        MappedLocalTime::Ambiguous(earliest, _latest) => Some(earliest.with_timezone(&chrono::Utc)),
        MappedLocalTime::None => {
            // Spring-forward gap. Step forward minute by minute to the first
            // existing wall-clock time; DST gaps are at most a couple of hours,
            // and all croniq clock-time schedules are minute-resolution.
            for add_min in 1..=180 {
                if let MappedLocalTime::Single(dt) | MappedLocalTime::Ambiguous(dt, _) =
                    tz.from_local_datetime(&(naive + Duration::minutes(add_min)))
                {
                    return Some(dt.with_timezone(&chrono::Utc));
                }
            }
            None
        }
    }
}

fn ast_weekday_to_chrono(day: &AstWeekday) -> chrono::Weekday {
    match day {
        AstWeekday::Monday => chrono::Weekday::Mon,
        AstWeekday::Tuesday => chrono::Weekday::Tue,
        AstWeekday::Wednesday => chrono::Weekday::Wed,
        AstWeekday::Thursday => chrono::Weekday::Thu,
        AstWeekday::Friday => chrono::Weekday::Fri,
        AstWeekday::Saturday => chrono::Weekday::Sat,
        AstWeekday::Sunday => chrono::Weekday::Sun,
    }
}

fn weekday_name(day: &chrono::Weekday) -> &'static str {
    match day {
        chrono::Weekday::Mon => "monday",
        chrono::Weekday::Tue => "tuesday",
        chrono::Weekday::Wed => "wednesday",
        chrono::Weekday::Thu => "thursday",
        chrono::Weekday::Fri => "friday",
        chrono::Weekday::Sat => "saturday",
        chrono::Weekday::Sun => "sunday",
    }
}

fn format_ordinal(day: u8) -> String {
    let suffix = match day {
        1 | 21 | 31 => "st",
        2 | 22 => "nd",
        3 | 23 => "rd",
        _ => "th",
    };
    format!("{day}{suffix}")
}

fn add_months(year: i32, month: u32, offset: u32) -> (i32, u32) {
    let total = (month - 1) + offset;
    let new_year = year + (total / 12) as i32;
    let new_month = (total % 12) + 1;
    (new_year, new_month)
}

fn last_day_of_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    NaiveDate::from_ymd_opt(next_year, next_month, 1)
        .unwrap()
        .pred_opt()
        .unwrap()
        .day()
}

fn parse_datetime(s: &str) -> Result<chrono::DateTime<chrono::Utc>, ScheduleError> {
    // Try RFC3339 first
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&chrono::Utc));
    }
    // Try ISO8601 without timezone (assume UTC)
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Ok(chrono::Utc.from_utc_datetime(&dt));
    }
    Err(ScheduleError::InvalidDatetime(s.to_string()))
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ScheduleError {
    #[error("invalid interval: {0}")]
    InvalidInterval(String),

    #[error("invalid time: {0}")]
    InvalidTime(String),

    #[error("invalid datetime: {0}")]
    InvalidDatetime(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn utc(y: i32, m: u32, d: u32, h: u32, min: u32) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc.with_ymd_and_hms(y, m, d, h, min, 0).unwrap()
    }

    fn tz_vienna() -> Tz {
        "Europe/Vienna".parse().unwrap()
    }

    fn tz_utc() -> Tz {
        "UTC".parse().unwrap()
    }

    // ─── Interval ───

    #[test]
    fn interval_every_5_minutes() {
        let sched = Schedule::Interval { seconds: 300 };
        let after = utc(2026, 3, 29, 10, 0);
        let next = sched.next_fire_after(after, &tz_utc()).unwrap();
        assert_eq!(next, utc(2026, 3, 29, 10, 5));
    }

    #[test]
    fn interval_next_n() {
        let sched = Schedule::Interval { seconds: 3600 };
        let after = utc(2026, 3, 29, 10, 0);
        let fires = sched.next_n_fires(after, &tz_utc(), 3);
        assert_eq!(fires.len(), 3);
        assert_eq!(fires[0], utc(2026, 3, 29, 11, 0));
        assert_eq!(fires[1], utc(2026, 3, 29, 12, 0));
        assert_eq!(fires[2], utc(2026, 3, 29, 13, 0));
    }

    // ─── Daily ───

    #[test]
    fn daily_at_0200_utc() {
        let sched = Schedule::Daily {
            time: NaiveTime::from_hms_opt(2, 0, 0).unwrap(),
        };
        // After 01:00 → fires at 02:00 same day
        let next = sched
            .next_fire_after(utc(2026, 3, 29, 1, 0), &tz_utc())
            .unwrap();
        assert_eq!(next, utc(2026, 3, 29, 2, 0));
    }

    #[test]
    fn daily_past_time_goes_to_tomorrow() {
        let sched = Schedule::Daily {
            time: NaiveTime::from_hms_opt(2, 0, 0).unwrap(),
        };
        // After 03:00 → fires at 02:00 next day
        let next = sched
            .next_fire_after(utc(2026, 3, 29, 3, 0), &tz_utc())
            .unwrap();
        assert_eq!(next, utc(2026, 3, 30, 2, 0));
    }

    // ─── DST spring-forward (issue #249) ───
    //
    // Europe/Berlin springs forward on 2026-03-29: the clock jumps
    // 02:00 CET → 03:00 CEST, so wall-clock times in [02:00, 03:00) do
    // not exist that day. A clock-time schedule landing in the gap must
    // roll forward to the transition instant instead of returning None
    // (which previously exhausted the trigger forever).

    fn tz_berlin() -> Tz {
        "Europe/Berlin".parse().unwrap()
    }

    #[test]
    fn daily_dst_spring_forward_gap_rolls_forward() {
        let sched = Schedule::Daily {
            time: NaiveTime::from_hms_opt(2, 30, 0).unwrap(),
        };
        let tz = tz_berlin();
        // 00:00 UTC = 01:00 CET, just before the 02:00→03:00 jump.
        let after = utc(2026, 3, 29, 0, 0);
        let next = sched.next_fire_after(after, &tz);
        // 02:30 local doesn't exist → rolls to 03:00 CEST = 01:00 UTC.
        assert_eq!(next, Some(utc(2026, 3, 29, 1, 0)));
    }

    #[test]
    fn daily_dst_gap_does_not_exhaust_across_fires() {
        // The follow-up fire (next day) must be the normal 02:30 CEST,
        // not a duplicate of the rolled-forward instant.
        let sched = Schedule::Daily {
            time: NaiveTime::from_hms_opt(2, 30, 0).unwrap(),
        };
        let tz = tz_berlin();
        let fires = sched.next_n_fires(utc(2026, 3, 29, 0, 0), &tz, 2);
        assert_eq!(fires.len(), 2);
        assert_eq!(fires[0], utc(2026, 3, 29, 1, 0)); // gap → 03:00 CEST
        // 2026-03-30 02:30 CEST = 00:30 UTC
        assert_eq!(fires[1], utc(2026, 3, 30, 0, 30));
    }

    #[test]
    fn weekdays_dst_spring_forward_gap_rolls_forward() {
        // 2026-03-29 is a Sunday.
        let sched = Schedule::Weekdays {
            days: vec![chrono::Weekday::Sun],
            time: NaiveTime::from_hms_opt(2, 30, 0).unwrap(),
        };
        let tz = tz_berlin();
        let next = sched.next_fire_after(utc(2026, 3, 29, 0, 0), &tz);
        assert_eq!(next, Some(utc(2026, 3, 29, 1, 0)));
    }

    #[test]
    fn monthly_dst_spring_forward_gap_rolls_forward() {
        // 29th of the month at 02:30 — lands in the Berlin gap on 2026-03-29.
        let sched = Schedule::Monthly {
            ordinals: vec![MonthDay::Day(29)],
            time: NaiveTime::from_hms_opt(2, 30, 0).unwrap(),
        };
        let tz = tz_berlin();
        let next = sched.next_fire_after(utc(2026, 3, 28, 0, 0), &tz);
        assert_eq!(next, Some(utc(2026, 3, 29, 1, 0)));
    }

    #[test]
    fn daily_dst_fall_back_picks_earliest() {
        // Autumn fall-back 2026-10-25: 03:00 CEST → 02:00 CET, so 02:30
        // occurs twice. We deliberately pick the earliest (CEST) instance.
        let sched = Schedule::Daily {
            time: NaiveTime::from_hms_opt(2, 30, 0).unwrap(),
        };
        let tz = tz_berlin();
        // 00:00 UTC = 02:00 CEST, before the first 02:30 occurrence.
        let after = utc(2026, 10, 25, 0, 0);
        let next = sched.next_fire_after(after, &tz).unwrap();
        // Earliest 02:30 is CEST (UTC+2) = 00:30 UTC.
        assert_eq!(next, utc(2026, 10, 25, 0, 30));
    }

    #[test]
    fn daily_vienna_timezone() {
        let sched = Schedule::Daily {
            time: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        };
        let tz = tz_vienna();
        // Vienna is UTC+1 (winter) or UTC+2 (summer)
        // March 29, 2026 is during summer time (CEST, UTC+2)
        // So 09:00 Vienna = 07:00 UTC
        let after = utc(2026, 3, 29, 0, 0);
        let next = sched.next_fire_after(after, &tz).unwrap();
        assert_eq!(next, utc(2026, 3, 29, 7, 0));
    }

    // ─── Weekdays ───

    #[test]
    fn weekday_schedule_skips_weekend() {
        let sched = Schedule::Weekdays {
            days: vec![
                chrono::Weekday::Mon,
                chrono::Weekday::Tue,
                chrono::Weekday::Wed,
                chrono::Weekday::Thu,
                chrono::Weekday::Fri,
            ],
            time: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        };
        // March 29, 2026 is a Sunday
        let after = utc(2026, 3, 29, 10, 0);
        let next = sched.next_fire_after(after, &tz_utc()).unwrap();
        // Should skip to Monday March 30
        assert_eq!(next, utc(2026, 3, 30, 9, 0));
    }

    #[test]
    fn weekday_fires_today_if_before_time() {
        let sched = Schedule::Weekdays {
            days: vec![chrono::Weekday::Mon],
            time: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        };
        // March 30, 2026 is a Monday, before 09:00
        let after = utc(2026, 3, 30, 7, 0);
        let next = sched.next_fire_after(after, &tz_utc()).unwrap();
        assert_eq!(next, utc(2026, 3, 30, 9, 0));
    }

    // ─── Monthly ───

    #[test]
    fn monthly_1st_of_month() {
        let sched = Schedule::Monthly {
            ordinals: vec![MonthDay::Day(1)],
            time: NaiveTime::from_hms_opt(6, 0, 0).unwrap(),
        };
        // After March 29 → April 1
        let after = utc(2026, 3, 29, 10, 0);
        let next = sched.next_fire_after(after, &tz_utc()).unwrap();
        assert_eq!(next, utc(2026, 4, 1, 6, 0));
    }

    #[test]
    fn monthly_last_day() {
        let sched = Schedule::Monthly {
            ordinals: vec![MonthDay::Last],
            time: NaiveTime::from_hms_opt(23, 59, 0).unwrap(),
        };
        // After March 29 → March 31 (last day of March)
        let after = utc(2026, 3, 29, 0, 0);
        let next = sched.next_fire_after(after, &tz_utc()).unwrap();
        assert_eq!(next, utc(2026, 3, 31, 23, 59));
    }

    #[test]
    fn monthly_1st_and_15th() {
        let sched = Schedule::Monthly {
            ordinals: vec![MonthDay::Day(1), MonthDay::Day(15)],
            time: NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
        };
        let after = utc(2026, 3, 2, 0, 0);
        let next = sched.next_fire_after(after, &tz_utc()).unwrap();
        // Should be March 15
        assert_eq!(next, utc(2026, 3, 15, 10, 0));
    }

    #[test]
    fn monthly_february_last() {
        let sched = Schedule::Monthly {
            ordinals: vec![MonthDay::Last],
            time: NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
        };
        let after = utc(2026, 2, 1, 0, 0);
        let next = sched.next_fire_after(after, &tz_utc()).unwrap();
        // Feb 2026 has 28 days
        assert_eq!(next, utc(2026, 2, 28, 0, 0));
    }

    // ─── Once ───

    #[test]
    fn once_in_future() {
        let at = utc(2026, 4, 1, 3, 0);
        let sched = Schedule::Once { at };
        let next = sched
            .next_fire_after(utc(2026, 3, 29, 0, 0), &tz_utc())
            .unwrap();
        assert_eq!(next, at);
    }

    #[test]
    fn once_in_past() {
        let at = utc(2026, 1, 1, 0, 0);
        let sched = Schedule::Once { at };
        let next = sched.next_fire_after(utc(2026, 3, 29, 0, 0), &tz_utc());
        assert!(next.is_none());
    }

    // ─── Disabled ───

    #[test]
    fn disabled_never_fires() {
        let sched = Schedule::Disabled;
        assert!(
            sched
                .next_fire_after(utc(2026, 3, 29, 0, 0), &tz_utc())
                .is_none()
        );
    }

    // ─── next_n_fires ───

    #[test]
    fn daily_next_5() {
        let sched = Schedule::Daily {
            time: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        };
        let fires = sched.next_n_fires(utc(2026, 3, 29, 0, 0), &tz_utc(), 5);
        assert_eq!(fires.len(), 5);
        assert_eq!(fires[0], utc(2026, 3, 29, 9, 0));
        assert_eq!(fires[4], utc(2026, 4, 2, 9, 0));
    }

    #[test]
    fn once_next_n_returns_at_most_one() {
        let sched = Schedule::Once {
            at: utc(2026, 4, 1, 3, 0),
        };
        let fires = sched.next_n_fires(utc(2026, 3, 29, 0, 0), &tz_utc(), 5);
        assert_eq!(fires.len(), 1);
    }

    // ─── Summary ───

    #[test]
    fn summary_interval() {
        let sched = Schedule::Interval { seconds: 900 };
        assert_eq!(sched.summary(), "every 15 minutes");
    }

    #[test]
    fn summary_interval_singular() {
        // Pin the n=1 fix — previously rendered "every 1 minutes".
        assert_eq!(
            Schedule::Interval { seconds: 60 }.summary(),
            "every 1 minute"
        );
        assert_eq!(
            Schedule::Interval { seconds: 3600 }.summary(),
            "every 1 hour"
        );
        assert_eq!(
            Schedule::Interval { seconds: 1 }.summary(),
            "every 1 second"
        );
    }

    #[test]
    fn summary_weekdays() {
        let sched = Schedule::Weekdays {
            days: vec![chrono::Weekday::Mon, chrono::Weekday::Fri],
            time: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        };
        assert_eq!(sched.summary(), "every monday, friday at 09:00");
    }

    // ─── Parse datetime ───

    #[test]
    fn parse_rfc3339() {
        let dt = parse_datetime("2026-04-01T03:00:00Z").unwrap();
        assert_eq!(dt, utc(2026, 4, 1, 3, 0));
    }

    #[test]
    fn parse_with_offset() {
        let dt = parse_datetime("2026-04-01T03:00:00+02:00").unwrap();
        // 03:00 +02:00 = 01:00 UTC
        assert_eq!(dt, utc(2026, 4, 1, 1, 0));
    }
}

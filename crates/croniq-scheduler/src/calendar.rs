//! Calendar evaluation: include/exclude rules for fire time filtering.
//!
//! A calendar determines whether a given datetime is "allowed" for job execution.
//! Rules: `(all includes match) AND (no exclude matches)`. No includes = everything allowed.

use crate::schedule::{ast_weekday_to_chrono, resolve_local_at_or_after};
use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, NaiveTime, Timelike, Utc};
use chrono_tz::Tz;
use croniq_config::calendar_args::{self, AnnualArg};

/// A compiled calendar with evaluated rules.
#[derive(Debug, Clone)]
pub struct Calendar {
    pub name: String,
    /// The zone this calendar's rules are evaluated in (issue #450) — already
    /// resolved, so there is no "unset" state to re-interpret at every gate
    /// check. `CalendarConfig::timezone` folded in `defaults { timezone … }`;
    /// nothing declared anywhere lands on UTC here.
    ///
    /// Deliberately *not* the consulting job's zone: a calendar is a named,
    /// shared resource, and "this holiday calendar is Austrian" must hold for
    /// every job that references it.
    pub tz: Tz,
    pub includes: Vec<CalendarRule>,
    pub excludes: Vec<CalendarRule>,
}

/// A single calendar rule.
#[derive(Debug, Clone)]
pub enum CalendarRule {
    /// Match specific weekdays (e.g., monday..friday).
    Weekly(Vec<chrono::Weekday>),

    /// Match a daily time window (e.g., 08:00..18:00).
    Window(NaiveTime, NaiveTime),

    /// Match specific days of month (e.g., 1, 15).
    Monthly(Vec<u32>),

    /// Match recurring annual dates (e.g., 01-01, 12-25).
    /// Stored as (month, day) pairs.
    Annual(Vec<(u32, u32)>),

    /// Match specific full dates (e.g., 2026-04-06).
    Dates(Vec<NaiveDate>),
}

impl Calendar {
    /// Compile a calendar from config AST.
    pub fn from_config(
        cfg: &croniq_config::compile::CalendarConfig,
    ) -> Result<Self, CalendarError> {
        let mut includes = Vec::new();
        let mut excludes = Vec::new();

        for rule in &cfg.rules {
            let compiled = compile_rule(&rule.rule_type, &rule.args)?;
            match rule.kind.as_str() {
                "include" => includes.push(compiled),
                "exclude" => excludes.push(compiled),
                other => {
                    return Err(CalendarError::UnknownRuleKind(other.to_string()));
                }
            }
        }

        // A zone that does not parse falls back to UTC instead of failing the
        // calendar, and is reported by whoever supplied it rather than here —
        // this crate stays free of a logging dependency. Both suppliers screen
        // the value first: a Croniqfile fails `validate` and aborts the load
        // (#426), and a `calendar_definitions.timezone` row is warned about and
        // cleared in `calendar_config_from_definition`. Pausing every job that
        // consults a calendar with a legacy-bad zone would be a worse upgrade
        // than running it in UTC (issue #450).
        let tz = cfg
            .timezone
            .as_deref()
            .filter(|t| !t.is_empty())
            .and_then(|name| croniq_config::timezone::parse(name).ok())
            .unwrap_or(chrono_tz::UTC);

        Ok(Calendar {
            name: cfg.name.clone(),
            tz,
            includes,
            excludes,
        })
    }

    /// Check if a datetime is allowed by this calendar.
    ///
    /// Logic:
    /// - If there are include rules, ALL must match.
    /// - If there are exclude rules, NONE must match.
    /// - Result: `includes_pass AND NOT excludes_match`
    pub fn is_allowed(&self, date: NaiveDate, time: NaiveTime) -> bool {
        let includes_pass = if self.includes.is_empty() {
            true
        } else {
            self.includes.iter().all(|r| r.matches(date, time))
        };

        let excludes_match = self.excludes.iter().any(|r| r.matches(date, time));

        includes_pass && !excludes_match
    }

    /// Allowed second-of-day intervals on `date`.
    ///
    /// This factors `is_allowed` exactly: every rule kind is either
    /// time-independent (Weekly/Monthly/Annual/Dates — gates the whole day)
    /// or date-independent (Window — contributes the same daily time set on
    /// every day), so "allowed at (date, t)" is equivalent to "all date-rules
    /// pass on `date`" AND "t lies in (∩ include windows) ∖ (∪ exclude
    /// windows)". Pinned by the `allowed_intervals_factor_is_allowed` test.
    pub(crate) fn allowed_intervals_on(&self, date: NaiveDate) -> DaySet {
        let mut set: DaySet = vec![(0, DAY_SECS)];
        for rule in &self.includes {
            match rule {
                CalendarRule::Window(s, e) => {
                    set = intersect_intervals(&set, &window_intervals(*s, *e));
                }
                date_rule => {
                    if !date_rule.matches(date, NaiveTime::MIN) {
                        return Vec::new();
                    }
                }
            }
        }
        for rule in &self.excludes {
            match rule {
                CalendarRule::Window(s, e) => {
                    set = subtract_intervals(&set, &window_intervals(*s, *e));
                }
                date_rule => {
                    if date_rule.matches(date, NaiveTime::MIN) {
                        return Vec::new();
                    }
                }
            }
        }
        set
    }

    /// Earliest local datetime `>= from` allowed by this calendar, or `None`
    /// when nothing is allowed within [`MAX_SCAN_DAYS`] (genuinely exhausted,
    /// e.g. a `dates` calendar entirely in the past). Unlike walking raw
    /// schedule ticks through `is_allowed`, this jumps straight to the next
    /// open window, so the cost is O(days-to-window) regardless of how
    /// frequent the schedule is (#391).
    pub fn next_allowed_after(&self, from: NaiveDateTime) -> Option<NaiveDateTime> {
        next_instant_in(from, |d| self.allowed_intervals_on(d))
    }

    /// Whether the calendar is open at the UTC instant `at`.
    ///
    /// The only correct way to ask: the date and time the rules are compared
    /// against are the calendar's own local ones (issue #450), never the
    /// consulting job's.
    pub fn is_allowed_at(&self, at: DateTime<Utc>) -> bool {
        let local = at.with_timezone(&self.tz);
        self.is_allowed(local.date_naive(), local.time())
    }

    /// Earliest UTC instant `>= from` at which the calendar is open, or `None`
    /// when nothing opens within [`MAX_SCAN_DAYS`].
    ///
    /// Scans in the calendar's own zone, which is what keeps
    /// [`Self::allowed_intervals_on`]'s date/time factoring valid: seen from
    /// here a `window` really does contribute the same second-of-day set on
    /// every day. (Projected into some *other* zone it would not — that is why
    /// the trigger intersects the two gates by advancing instants rather than
    /// by merging interval sets; see `Trigger::next_gate_open`.)
    pub fn next_open_at_or_after(&self, from: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let local = from.with_timezone(&self.tz).naive_local();
        let open_local = next_instant_in(local, |d| self.allowed_intervals_on(d))?;
        resolve_local_at_or_after(&self.tz, open_local, from)
    }
}

/// Sorted, disjoint, half-open `[start, end)` second-of-day intervals
/// (endpoints in `0..=86400`) describing when a single day is open.
pub(crate) type DaySet = Vec<(u32, u32)>;

const DAY_SECS: u32 = 86_400;

/// How far [`next_instant_in`] scans before declaring the gate permanently
/// closed: 4 years + 2 days covers an `annual` rule that only matches Feb 29
/// across a full leap cycle.
pub(crate) const MAX_SCAN_DAYS: u64 = 4 * 365 + 2;

/// Interval form of the shared window semantics (`CalendarRule::Window` /
/// `TimeWindow::contains`): half-open `[start, end)`, `start > end` wraps
/// overnight at midnight, `start == end` matches nothing.
pub(crate) fn window_intervals(start: NaiveTime, end: NaiveTime) -> DaySet {
    let s = start.num_seconds_from_midnight();
    let e = end.num_seconds_from_midnight();
    match s.cmp(&e) {
        std::cmp::Ordering::Less => vec![(s, e)],
        std::cmp::Ordering::Equal => Vec::new(),
        std::cmp::Ordering::Greater => {
            let mut set = DaySet::new();
            if e > 0 {
                set.push((0, e));
            }
            set.push((s, DAY_SECS));
            set
        }
    }
}

/// Intersection of two interval sets (two-pointer merge).
pub(crate) fn intersect_intervals(a: &DaySet, b: &DaySet) -> DaySet {
    let mut out = DaySet::new();
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        let lo = a[i].0.max(b[j].0);
        let hi = a[i].1.min(b[j].1);
        if lo < hi {
            out.push((lo, hi));
        }
        if a[i].1 <= b[j].1 {
            i += 1;
        } else {
            j += 1;
        }
    }
    out
}

/// `a` minus `b`, via intersecting `a` with the complement of `b`.
pub(crate) fn subtract_intervals(a: &DaySet, b: &DaySet) -> DaySet {
    let mut complement = DaySet::new();
    let mut cursor = 0;
    for &(s, e) in b {
        if s > cursor {
            complement.push((cursor, s));
        }
        cursor = cursor.max(e);
    }
    if cursor < DAY_SECS {
        complement.push((cursor, DAY_SECS));
    }
    intersect_intervals(a, &complement)
}

/// Earliest local instant `>= from` inside the daily interval sets produced
/// by `intervals_on`. `None` = nothing within [`MAX_SCAN_DAYS`].
pub(crate) fn next_instant_in(
    from: NaiveDateTime,
    mut intervals_on: impl FnMut(NaiveDate) -> DaySet,
) -> Option<NaiveDateTime> {
    // Ceil sub-second precision to whole seconds so the returned instant is
    // never before `from`. May yield 86400 (past midnight) — day 0 then
    // simply finds no interval and the scan moves on.
    let mut from_secs = from.time().num_seconds_from_midnight();
    if from.time().nanosecond() > 0 {
        from_secs += 1;
    }
    for offset in 0..=MAX_SCAN_DAYS {
        let date = from.date().checked_add_days(chrono::Days::new(offset))?;
        for (s, e) in intervals_on(date) {
            let lower = if offset == 0 { from_secs.max(s) } else { s };
            if lower < e {
                return Some(
                    date.and_time(NaiveTime::from_num_seconds_from_midnight_opt(lower, 0)?),
                );
            }
        }
    }
    None
}

impl CalendarRule {
    /// Check if this rule matches the given date/time.
    fn matches(&self, date: NaiveDate, time: NaiveTime) -> bool {
        match self {
            CalendarRule::Weekly(days) => days.contains(&date.weekday()),

            CalendarRule::Window(start, end) => {
                if start <= end {
                    // Normal window: 08:00..18:00
                    time >= *start && time < *end
                } else {
                    // Overnight window: 22:00..06:00
                    time >= *start || time < *end
                }
            }

            CalendarRule::Monthly(days) => days.contains(&date.day()),

            CalendarRule::Annual(dates) => dates
                .iter()
                .any(|(m, d)| date.month() == *m && date.day() == *d),

            CalendarRule::Dates(dates) => dates.contains(&date),
        }
    }
}

// Argument parsing is shared with croniq-config's `validate` via
// `calendar_args`, so offline validation and runtime compilation cannot
// diverge (#356).
fn compile_rule(rule_type: &str, args: &[String]) -> Result<CalendarRule, CalendarError> {
    match rule_type {
        "weekly" => {
            let mut days: Vec<chrono::Weekday> = Vec::new();
            for arg in args {
                let parsed =
                    calendar_args::parse_weekly_arg(arg).map_err(CalendarError::InvalidRule)?;
                days.extend(parsed.iter().map(ast_weekday_to_chrono));
            }
            Ok(CalendarRule::Weekly(days))
        }
        "window" => {
            // Args might be ["08:00..18:00"] (range) or ["08:00", "18:00"]
            let (start, end) =
                calendar_args::parse_window_args(args).map_err(CalendarError::InvalidRule)?;
            Ok(CalendarRule::Window(start, end))
        }
        "monthly" => {
            let days: Vec<u32> = args
                .iter()
                .map(|s| calendar_args::parse_monthly_arg(s).map_err(CalendarError::InvalidRule))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(CalendarRule::Monthly(days))
        }
        "annual" | "yearly" => {
            let mut annual_dates = Vec::new();
            let mut specific_dates = Vec::new();

            for arg in args {
                match calendar_args::parse_annual_arg(arg).map_err(CalendarError::InvalidRule)? {
                    AnnualArg::MonthDay(month, day) => annual_dates.push((month, day)),
                    AnnualArg::Date(date) => specific_dates.push(date),
                    AnnualArg::Ignored => continue,
                }
            }

            // If we have a mix, combine. Annual dates match every year.
            // Specific dates match only in their year.
            // For simplicity, if ALL are annual (MM-DD), return Annual rule.
            // If ALL are specific (YYYY-MM-DD), return Dates rule.
            // Mixed: return both in a single rule isn't possible, so we prefer Annual
            // and add specific dates as Annual entries using their month/day.
            if specific_dates.is_empty() {
                Ok(CalendarRule::Annual(annual_dates))
            } else if annual_dates.is_empty() {
                Ok(CalendarRule::Dates(specific_dates))
            } else {
                // Mixed: treat specific dates as annual (recurring) for simplicity
                for date in &specific_dates {
                    annual_dates.push((date.month(), date.day()));
                }
                Ok(CalendarRule::Annual(annual_dates))
            }
        }
        other => Err(CalendarError::UnknownRuleType(other.to_string())),
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum CalendarError {
    #[error("unknown rule kind: {0}")]
    UnknownRuleKind(String),

    #[error("unknown rule type: {0}")]
    UnknownRuleType(String),

    #[error("invalid rule: {0}")]
    InvalidRule(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn time(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    // ─── Weekly ───

    #[test]
    fn weekly_weekdays() {
        let rule = CalendarRule::Weekly(vec![
            chrono::Weekday::Mon,
            chrono::Weekday::Tue,
            chrono::Weekday::Wed,
            chrono::Weekday::Thu,
            chrono::Weekday::Fri,
        ]);
        // March 30, 2026 = Monday
        assert!(rule.matches(date(2026, 3, 30), time(9, 0)));
        // March 29, 2026 = Sunday
        assert!(!rule.matches(date(2026, 3, 29), time(9, 0)));
    }

    // ─── Window ───

    #[test]
    fn window_normal() {
        let rule = CalendarRule::Window(time(8, 0), time(18, 0));
        assert!(rule.matches(date(2026, 3, 30), time(9, 0)));
        assert!(rule.matches(date(2026, 3, 30), time(8, 0)));
        assert!(!rule.matches(date(2026, 3, 30), time(18, 0)));
        assert!(!rule.matches(date(2026, 3, 30), time(7, 59)));
    }

    #[test]
    fn window_overnight() {
        let rule = CalendarRule::Window(time(22, 0), time(6, 0));
        assert!(rule.matches(date(2026, 3, 30), time(23, 0)));
        assert!(rule.matches(date(2026, 3, 30), time(3, 0)));
        assert!(!rule.matches(date(2026, 3, 30), time(12, 0)));
    }

    // ─── Annual ───

    #[test]
    fn annual_holidays() {
        let rule = CalendarRule::Annual(vec![(1, 1), (12, 25), (12, 26)]);
        assert!(rule.matches(date(2026, 1, 1), time(0, 0)));
        assert!(rule.matches(date(2026, 12, 25), time(0, 0)));
        assert!(!rule.matches(date(2026, 3, 15), time(0, 0)));
    }

    // ─── Specific dates ───

    #[test]
    fn specific_dates() {
        let rule = CalendarRule::Dates(vec![date(2026, 4, 6), date(2026, 4, 7)]);
        assert!(rule.matches(date(2026, 4, 6), time(0, 0)));
        assert!(!rule.matches(date(2027, 4, 6), time(0, 0))); // different year
    }

    // ─── Calendar composite ───

    #[test]
    fn business_days_calendar() {
        let cal = Calendar {
            name: "business-days".into(),
            tz: chrono_tz::Europe::Vienna,
            includes: vec![CalendarRule::Weekly(vec![
                chrono::Weekday::Mon,
                chrono::Weekday::Tue,
                chrono::Weekday::Wed,
                chrono::Weekday::Thu,
                chrono::Weekday::Fri,
            ])],
            excludes: vec![CalendarRule::Annual(vec![(1, 1), (12, 25), (12, 26)])],
        };

        // Monday, regular day
        assert!(cal.is_allowed(date(2026, 3, 30), time(9, 0)));
        // Sunday
        assert!(!cal.is_allowed(date(2026, 3, 29), time(9, 0)));
        // Christmas (Thursday)
        assert!(!cal.is_allowed(date(2026, 12, 25), time(9, 0)));
        // New Year (Thursday)
        assert!(!cal.is_allowed(date(2026, 1, 1), time(9, 0)));
    }

    #[test]
    fn maintenance_window_calendar() {
        let cal = Calendar {
            name: "maintenance".into(),
            tz: chrono_tz::UTC,
            includes: vec![
                CalendarRule::Weekly(vec![chrono::Weekday::Sun]),
                CalendarRule::Window(time(2, 0), time(6, 0)),
            ],
            excludes: vec![],
        };

        // Sunday 03:00 — allowed
        assert!(cal.is_allowed(date(2026, 3, 29), time(3, 0)));
        // Sunday 10:00 — outside window
        assert!(!cal.is_allowed(date(2026, 3, 29), time(10, 0)));
        // Monday 03:00 — wrong day
        assert!(!cal.is_allowed(date(2026, 3, 30), time(3, 0)));
    }

    #[test]
    fn no_includes_means_everything_allowed() {
        let cal = Calendar {
            name: "holidays-only".into(),
            tz: chrono_tz::UTC,
            includes: vec![],
            excludes: vec![CalendarRule::Annual(vec![(1, 1)])],
        };

        assert!(cal.is_allowed(date(2026, 6, 15), time(12, 0)));
        assert!(!cal.is_allowed(date(2026, 1, 1), time(12, 0)));
    }

    // ─── Compile from config ───

    #[test]
    fn compile_from_config() {
        use croniq_config::compile::{CalendarConfig, CalendarRuleConfig};

        let cfg = CalendarConfig {
            name: "biz".into(),
            timezone: Some("Europe/Vienna".into()),
            rules: vec![
                CalendarRuleConfig {
                    kind: "include".into(),
                    rule_type: "weekly".into(),
                    args: vec![
                        "monday".into(),
                        "tuesday".into(),
                        "wednesday".into(),
                        "thursday".into(),
                        "friday".into(),
                    ],
                },
                CalendarRuleConfig {
                    kind: "exclude".into(),
                    rule_type: "annual".into(),
                    args: vec!["01-01".into(), "12-25".into()],
                },
            ],
        };

        let cal = Calendar::from_config(&cfg).unwrap();
        assert_eq!(cal.includes.len(), 1);
        assert_eq!(cal.excludes.len(), 1);
        assert!(cal.is_allowed(date(2026, 3, 30), time(9, 0)));
        assert!(!cal.is_allowed(date(2026, 1, 1), time(9, 0)));
    }

    fn weekly_config(args: Vec<String>) -> croniq_config::compile::CalendarConfig {
        croniq_config::compile::CalendarConfig {
            name: "biz".into(),
            timezone: Some("UTC".into()),
            rules: vec![croniq_config::compile::CalendarRuleConfig {
                kind: "include".into(),
                rule_type: "weekly".into(),
                args,
            }],
        }
    }

    // #356: the `weekday`/`weekend` group aliases that `fmt` and the cron
    // converter emit must compile. This is also the shape a compile-time
    // resolved variable (`include weekly {days}` with `days = "weekday"`)
    // produces, which bypasses the parser-side expansion.
    #[test]
    fn compile_weekly_weekday_alias() {
        let cal = Calendar::from_config(&weekly_config(vec!["weekday".into()])).unwrap();
        // 2026-03-30 is a Monday, 2026-03-29 is a Sunday.
        assert!(cal.is_allowed(date(2026, 3, 30), time(9, 0)));
        assert!(!cal.is_allowed(date(2026, 3, 29), time(9, 0)));
    }

    #[test]
    fn compile_weekly_weekend_alias() {
        let cal = Calendar::from_config(&weekly_config(vec!["weekend".into()])).unwrap();
        assert!(cal.is_allowed(date(2026, 3, 29), time(9, 0)));
        assert!(!cal.is_allowed(date(2026, 3, 30), time(9, 0)));
    }

    #[test]
    fn compile_weekly_alias_mixed_with_day() {
        let cal =
            Calendar::from_config(&weekly_config(vec!["weekday".into(), "sunday".into()])).unwrap();
        assert!(cal.is_allowed(date(2026, 3, 29), time(9, 0))); // Sunday
        assert!(cal.is_allowed(date(2026, 3, 30), time(9, 0))); // Monday
        assert!(!cal.is_allowed(date(2026, 3, 28), time(9, 0))); // Saturday
    }

    #[test]
    fn compile_weekly_unknown_day_errors() {
        let err = Calendar::from_config(&weekly_config(vec!["funday".into()])).unwrap_err();
        assert_eq!(err.to_string(), "invalid rule: unknown weekday: funday");
    }

    #[test]
    fn compile_window_from_config() {
        let rule = compile_rule("window", &["08:00..18:00".to_string()]).unwrap();
        assert!(matches!(rule, CalendarRule::Window(_, _)));
        assert!(rule.matches(date(2026, 3, 30), time(9, 0)));
        assert!(!rule.matches(date(2026, 3, 30), time(19, 0)));
    }

    #[test]
    fn compile_monthly_from_config() {
        let rule = compile_rule("monthly", &["1".to_string(), "15".to_string()]).unwrap();
        assert!(rule.matches(date(2026, 3, 15), time(9, 0)));
        assert!(!rule.matches(date(2026, 3, 16), time(9, 0)));
        // Out-of-range days compile and are simply inert (runtime parity).
        assert!(compile_rule("monthly", &["45".to_string()]).is_ok());
    }

    #[test]
    fn compile_annual_ignored_arg_skipped() {
        // A len-5 arg with too many dashes has always been silently
        // skipped — the refactor must not make the loader stricter.
        let rule = compile_rule("annual", &["1-2-3".to_string(), "12-25".to_string()]).unwrap();
        match rule {
            CalendarRule::Annual(dates) => assert_eq!(dates, vec![(12, 25)]),
            other => panic!("expected Annual, got {other:?}"),
        }
    }

    // ─── DaySet primitives + next_allowed_after (#391) ───

    fn dt(y: i32, m: u32, d: u32, h: u32, min: u32) -> chrono::NaiveDateTime {
        date(y, m, d).and_time(time(h, min))
    }

    fn weekdays() -> CalendarRule {
        CalendarRule::Weekly(vec![
            chrono::Weekday::Mon,
            chrono::Weekday::Tue,
            chrono::Weekday::Wed,
            chrono::Weekday::Thu,
            chrono::Weekday::Fri,
        ])
    }

    /// Mon–Fri 08:00..18:00 — the business-hours shape from issue #391.
    fn business_hours() -> Calendar {
        Calendar {
            name: "business-hours".into(),
            tz: chrono_tz::UTC,
            includes: vec![weekdays(), CalendarRule::Window(time(8, 0), time(18, 0))],
            excludes: vec![],
        }
    }

    #[test]
    fn window_intervals_shapes() {
        assert_eq!(
            window_intervals(time(8, 0), time(18, 0)),
            vec![(8 * 3600, 18 * 3600)]
        );
        // Overnight wraps at midnight.
        assert_eq!(
            window_intervals(time(22, 0), time(6, 0)),
            vec![(0, 6 * 3600), (22 * 3600, 86_400)]
        );
        // Overnight degenerating to a pure suffix: no empty [0, 0) part.
        assert_eq!(
            window_intervals(time(22, 0), time(0, 0)),
            vec![(22 * 3600, 86_400)]
        );
        // start == end matches nothing (half-open semantics, parity with
        // CalendarRule::Window::matches).
        assert_eq!(
            window_intervals(time(9, 0), time(9, 0)),
            Vec::<(u32, u32)>::new()
        );
    }

    #[test]
    fn intersect_and_subtract() {
        let a = vec![(0u32, 10u32), (20, 30)];
        let b = vec![(5u32, 25u32)];
        assert_eq!(intersect_intervals(&a, &b), vec![(5, 10), (20, 25)]);
        assert_eq!(subtract_intervals(&a, &b), vec![(0, 5), (25, 30)]);
        assert_eq!(subtract_intervals(&a, &Vec::new()), a);
        assert_eq!(
            intersect_intervals(&a, &Vec::new()),
            Vec::<(u32, u32)>::new()
        );
        // Subtraction consuming a whole interval.
        assert_eq!(subtract_intervals(&a, &vec![(0, 15)]), vec![(20, 30)]);
    }

    /// Pin the factoring argument on `allowed_intervals_on`: interval
    /// membership must agree with `is_allowed` on a 15-minute grid over a
    /// composite calendar exercising every rule shape.
    #[test]
    fn allowed_intervals_factor_is_allowed() {
        let cal = Calendar {
            name: "composite".into(),
            tz: chrono_tz::UTC,
            includes: vec![weekdays(), CalendarRule::Window(time(8, 0), time(18, 0))],
            excludes: vec![
                CalendarRule::Annual(vec![(1, 1)]),
                CalendarRule::Window(time(12, 0), time(12, 45)),
            ],
        };
        // A week spanning New Year (hits the annual exclude) + a plain week.
        for start in [date(2025, 12, 29), date(2026, 3, 30)] {
            for day in 0..7 {
                let d = start.checked_add_days(chrono::Days::new(day)).unwrap();
                let set = cal.allowed_intervals_on(d);
                for quarter in 0..96u32 {
                    let secs = quarter * 900;
                    let t = NaiveTime::from_num_seconds_from_midnight_opt(secs, 0).unwrap();
                    let in_set = set.iter().any(|&(s, e)| secs >= s && secs < e);
                    assert_eq!(cal.is_allowed(d, t), in_set, "mismatch at {d} {t}");
                }
            }
        }
    }

    #[test]
    fn next_allowed_inside_window_returns_from() {
        // 2026-03-30 is a Monday.
        let from = dt(2026, 3, 30, 9, 30);
        assert_eq!(business_hours().next_allowed_after(from), Some(from));
    }

    #[test]
    fn next_allowed_start_edge_inclusive_end_edge_exclusive() {
        let cal = business_hours();
        let open = dt(2026, 3, 30, 8, 0);
        assert_eq!(cal.next_allowed_after(open), Some(open));
        // 18:00 itself is outside (half-open) → next day 08:00.
        assert_eq!(
            cal.next_allowed_after(dt(2026, 3, 30, 18, 0)),
            Some(dt(2026, 3, 31, 8, 0))
        );
    }

    #[test]
    fn next_allowed_before_open_same_day() {
        assert_eq!(
            business_hours().next_allowed_after(dt(2026, 3, 30, 6, 30)),
            Some(dt(2026, 3, 30, 8, 0))
        );
    }

    #[test]
    fn next_allowed_weekend_gap() {
        // 2026-04-03 is a Friday; 18:30 → Monday 08:00.
        assert_eq!(
            business_hours().next_allowed_after(dt(2026, 4, 3, 18, 30)),
            Some(dt(2026, 4, 6, 8, 0))
        );
    }

    #[test]
    fn next_allowed_overnight_include() {
        let cal = Calendar {
            name: "nightly".into(),
            tz: chrono_tz::UTC,
            includes: vec![CalendarRule::Window(time(22, 0), time(6, 0))],
            excludes: vec![],
        };
        assert_eq!(
            cal.next_allowed_after(dt(2026, 3, 30, 12, 0)),
            Some(dt(2026, 3, 30, 22, 0))
        );
        let inside = dt(2026, 3, 30, 23, 0);
        assert_eq!(cal.next_allowed_after(inside), Some(inside));
    }

    #[test]
    fn next_allowed_annual_exclude_skips_whole_day() {
        let mut cal = business_hours();
        cal.excludes.push(CalendarRule::Annual(vec![(1, 1)]));
        // 2026-01-01 is a Thursday; 07:00 that day → Friday 08:00.
        assert_eq!(
            cal.next_allowed_after(dt(2026, 1, 1, 7, 0)),
            Some(dt(2026, 1, 2, 8, 0))
        );
    }

    #[test]
    fn next_allowed_year_wrap() {
        let cal = Calendar {
            name: "new-year-only".into(),
            tz: chrono_tz::UTC,
            includes: vec![CalendarRule::Annual(vec![(1, 1)])],
            excludes: vec![],
        };
        assert_eq!(
            cal.next_allowed_after(dt(2026, 12, 15, 10, 0)),
            Some(dt(2027, 1, 1, 0, 0))
        );
    }

    #[test]
    fn next_allowed_monthly_31_skips_short_months() {
        let cal = Calendar {
            name: "day-31".into(),
            tz: chrono_tz::UTC,
            includes: vec![CalendarRule::Monthly(vec![31])],
            excludes: vec![],
        };
        // No Feb 31 → next match is Mar 31.
        assert_eq!(
            cal.next_allowed_after(dt(2026, 2, 1, 0, 0)),
            Some(dt(2026, 3, 31, 0, 0))
        );
    }

    #[test]
    fn next_allowed_feb29_crosses_leap_years() {
        let cal = Calendar {
            name: "leap-day".into(),
            tz: chrono_tz::UTC,
            includes: vec![CalendarRule::Annual(vec![(2, 29)])],
            excludes: vec![],
        };
        assert_eq!(
            cal.next_allowed_after(dt(2026, 3, 1, 0, 0)),
            Some(dt(2028, 2, 29, 0, 0))
        );
    }

    #[test]
    fn next_allowed_dates_all_past_is_none() {
        let cal = Calendar {
            name: "one-off".into(),
            tz: chrono_tz::UTC,
            includes: vec![CalendarRule::Dates(vec![date(2026, 4, 6)])],
            excludes: vec![],
        };
        assert_eq!(cal.next_allowed_after(dt(2026, 5, 1, 0, 0)), None);
    }

    #[test]
    fn next_allowed_ceils_subseconds() {
        // 17:59:59.5 must not resolve to an instant before itself: the last
        // whole second inside the window is 17:59:59, so ceil pushes past
        // the window edge → next day 08:00.
        let from =
            date(2026, 3, 30).and_time(NaiveTime::from_hms_milli_opt(17, 59, 59, 500).unwrap());
        assert_eq!(
            business_hours().next_allowed_after(from),
            Some(dt(2026, 3, 31, 8, 0))
        );
    }
}

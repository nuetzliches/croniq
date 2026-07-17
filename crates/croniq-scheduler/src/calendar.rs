//! Calendar evaluation: include/exclude rules for fire time filtering.
//!
//! A calendar determines whether a given datetime is "allowed" for job execution.
//! Rules: `(all includes match) AND (no exclude matches)`. No includes = everything allowed.

use crate::schedule::ast_weekday_to_chrono;
use chrono::{Datelike, NaiveDate, NaiveTime};
use croniq_config::calendar_args::{self, AnnualArg};

/// A compiled calendar with evaluated rules.
#[derive(Debug, Clone)]
pub struct Calendar {
    pub name: String,
    pub timezone: Option<String>,
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

        Ok(Calendar {
            name: cfg.name.clone(),
            timezone: cfg.timezone.clone(),
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
            timezone: Some("Europe/Vienna".into()),
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
            timezone: Some("UTC".into()),
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
            timezone: None,
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
            timezone: None,
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
}

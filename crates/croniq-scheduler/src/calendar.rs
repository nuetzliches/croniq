//! Calendar evaluation: include/exclude rules for fire time filtering.
//!
//! A calendar determines whether a given datetime is "allowed" for job execution.
//! Rules: `(all includes match) AND (no exclude matches)`. No includes = everything allowed.

use chrono::{Datelike, NaiveDate, NaiveTime, Timelike};

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
    pub fn from_config(cfg: &croniq_config::compile::CalendarConfig) -> Result<Self, CalendarError> {
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

            CalendarRule::Annual(dates) => {
                dates.iter().any(|(m, d)| date.month() == *m && date.day() == *d)
            }

            CalendarRule::Dates(dates) => dates.contains(&date),
        }
    }
}

fn compile_rule(rule_type: &str, args: &[String]) -> Result<CalendarRule, CalendarError> {
    match rule_type {
        "weekly" => {
            let days: Vec<chrono::Weekday> = args
                .iter()
                .map(|s| parse_weekday(s))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(CalendarRule::Weekly(days))
        }
        "window" => {
            if args.is_empty() {
                return Err(CalendarError::InvalidRule(
                    "window requires start..end times".into(),
                ));
            }
            // Args might be ["08:00..18:00"] (range) or ["08:00", "18:00"]
            let (start_str, end_str) = if args.len() == 1 {
                // Single arg with ".." separator
                args[0]
                    .split_once("..")
                    .ok_or_else(|| CalendarError::InvalidRule(format!("invalid window: {}", args[0])))?
            } else if args.len() == 2 {
                (args[0].as_str(), args[1].as_str())
            } else {
                return Err(CalendarError::InvalidRule(
                    "window expects start..end or start end".into(),
                ));
            };
            let start = parse_time(start_str)?;
            let end = parse_time(end_str)?;
            Ok(CalendarRule::Window(start, end))
        }
        "monthly" => {
            let days: Vec<u32> = args
                .iter()
                .map(|s| {
                    s.parse::<u32>()
                        .map_err(|_| CalendarError::InvalidRule(format!("invalid day: {s}")))
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(CalendarRule::Monthly(days))
        }
        "annual" | "yearly" => {
            let mut annual_dates = Vec::new();
            let mut specific_dates = Vec::new();

            for arg in args {
                if arg.len() == 5 && arg.contains('-') {
                    // MM-DD format
                    let parts: Vec<&str> = arg.split('-').collect();
                    if parts.len() == 2 {
                        let month: u32 = parts[0]
                            .parse()
                            .map_err(|_| CalendarError::InvalidRule(format!("invalid date: {arg}")))?;
                        let day: u32 = parts[1]
                            .parse()
                            .map_err(|_| CalendarError::InvalidRule(format!("invalid date: {arg}")))?;
                        annual_dates.push((month, day));
                    }
                } else if arg.len() == 10 && arg.chars().filter(|c| *c == '-').count() == 2 {
                    // YYYY-MM-DD format — specific date
                    let date = NaiveDate::parse_from_str(arg, "%Y-%m-%d")
                        .map_err(|_| CalendarError::InvalidRule(format!("invalid date: {arg}")))?;
                    specific_dates.push(date);
                } else {
                    return Err(CalendarError::InvalidRule(format!(
                        "invalid annual date format: {arg} (expected MM-DD or YYYY-MM-DD)"
                    )));
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

fn parse_weekday(s: &str) -> Result<chrono::Weekday, CalendarError> {
    match s.to_lowercase().as_str() {
        "monday" | "mon" => Ok(chrono::Weekday::Mon),
        "tuesday" | "tue" => Ok(chrono::Weekday::Tue),
        "wednesday" | "wed" => Ok(chrono::Weekday::Wed),
        "thursday" | "thu" => Ok(chrono::Weekday::Thu),
        "friday" | "fri" => Ok(chrono::Weekday::Fri),
        "saturday" | "sat" => Ok(chrono::Weekday::Sat),
        "sunday" | "sun" => Ok(chrono::Weekday::Sun),
        other => Err(CalendarError::InvalidRule(format!("unknown weekday: {other}"))),
    }
}

fn parse_time(s: &str) -> Result<NaiveTime, CalendarError> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 {
        return Err(CalendarError::InvalidRule(format!("invalid time: {s}")));
    }
    let hour: u32 = parts[0]
        .parse()
        .map_err(|_| CalendarError::InvalidRule(format!("invalid time: {s}")))?;
    let minute: u32 = parts[1]
        .parse()
        .map_err(|_| CalendarError::InvalidRule(format!("invalid time: {s}")))?;
    NaiveTime::from_hms_opt(hour, minute, 0)
        .ok_or_else(|| CalendarError::InvalidRule(format!("invalid time: {s}")))
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
            excludes: vec![
                CalendarRule::Annual(vec![(1, 1), (12, 25), (12, 26)]),
            ],
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
}

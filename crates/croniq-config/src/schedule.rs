//! Schedule evaluation: computes next fire times from human-readable schedule AST nodes.
//! This module will be expanded in Phase 2 (croniq-scheduler).

use crate::ast::{IntervalUnit, MonthOrdinal, ScheduleKind, Weekday};

/// A compiled schedule that can compute next fire times.
#[derive(Debug, Clone, serde::Serialize)]
pub enum CompiledSchedule {
    Interval {
        seconds: u64,
    },
    Daily {
        hour: u8,
        minute: u8,
    },
    Weekdays {
        days: Vec<Weekday>,
        hour: u8,
        minute: u8,
    },
    Monthly {
        ordinals: Vec<MonthOrdinal>,
        hour: u8,
        minute: u8,
    },
    Once {
        at: String,
    },
    Disabled,
}

impl CompiledSchedule {
    pub fn from_ast(kind: &ScheduleKind) -> Self {
        match kind {
            ScheduleKind::Interval { count, unit } => {
                let seconds = match unit {
                    IntervalUnit::Seconds => *count as u64,
                    IntervalUnit::Minutes => *count as u64 * 60,
                    IntervalUnit::Hours => *count as u64 * 3600,
                };
                CompiledSchedule::Interval { seconds }
            }
            ScheduleKind::Daily { time } => CompiledSchedule::Daily {
                hour: time.hour,
                minute: time.minute,
            },
            ScheduleKind::Weekdays { days, time } => CompiledSchedule::Weekdays {
                days: days.clone(),
                hour: time.hour,
                minute: time.minute,
            },
            ScheduleKind::Monthly { ordinals, time } => CompiledSchedule::Monthly {
                ordinals: ordinals.clone(),
                hour: time.hour,
                minute: time.minute,
            },
            ScheduleKind::Once { at } => CompiledSchedule::Once {
                at: at.value.clone(),
            },
            ScheduleKind::Disabled => CompiledSchedule::Disabled,
        }
    }

    /// Human-readable summary of this schedule.
    pub fn summary(&self) -> String {
        match self {
            CompiledSchedule::Interval { seconds } => {
                // Grammatical number lives in `plural` so this emitter can't
                // drift from `format` / `convert` on the count-of-1 rule.
                if *seconds >= 3600 && seconds % 3600 == 0 {
                    format!(
                        "every {}",
                        crate::plural::interval_phrase(seconds / 3600, "hour")
                    )
                } else if *seconds >= 60 && seconds % 60 == 0 {
                    format!(
                        "every {}",
                        crate::plural::interval_phrase(seconds / 60, "minute")
                    )
                } else {
                    format!(
                        "every {}",
                        crate::plural::interval_phrase(*seconds, "second")
                    )
                }
            }
            CompiledSchedule::Daily { hour, minute } => {
                format!("every day at {hour:02}:{minute:02}")
            }
            CompiledSchedule::Weekdays { days, hour, minute } => {
                let day_names: Vec<&str> = days.iter().map(|d| d.as_str()).collect();
                format!("every {} at {hour:02}:{minute:02}", day_names.join(", "))
            }
            CompiledSchedule::Monthly {
                ordinals,
                hour,
                minute,
            } => {
                let ord_names: Vec<String> = ordinals
                    .iter()
                    .map(|o| match o {
                        MonthOrdinal::Day(d) => format!("{d}"),
                        MonthOrdinal::Last => "last".to_string(),
                    })
                    .collect();
                format!(
                    "every {} of month at {hour:02}:{minute:02}",
                    ord_names.join(", ")
                )
            }
            CompiledSchedule::Once { at } => format!("once at {at}"),
            CompiledSchedule::Disabled => "disabled".to_string(),
        }
    }

    /// Emit a canonical DSL schedule line that round-trips through
    /// [`crate::parser::parse_schedule_expr`].
    ///
    /// Unlike [`Self::summary`], the weekday/monthly forms use the
    /// space-separated syntax the parser accepts — the comma-joined summary
    /// (`every monday, friday at 09:00`) does not re-parse. This is what gets
    /// persisted into a `cron_expression` for DSL/adopted jobs so the scheduler
    /// can rebuild the trigger on reload. Delegates to the shared canonical
    /// emitter so it can never drift from the formatter.
    pub fn to_dsl(&self) -> String {
        crate::format::format_schedule_line(&self.to_schedule_kind())
    }

    /// Rebuild the AST [`ScheduleKind`] this schedule compiled from, so the
    /// canonical formatter can emit it. The spans are synthetic (this value
    /// was never lexed) but the formatter ignores them. `Once` is re-quoted so
    /// datetimes with offsets survive re-lexing.
    fn to_schedule_kind(&self) -> ScheduleKind {
        use crate::ast::{StringValue, TimeValue};
        use crate::lexer::Span;

        let time = |hour: u8, minute: u8| TimeValue {
            hour,
            minute,
            raw: format!("{hour:02}:{minute:02}"),
            span: Span::empty(0),
        };

        match self {
            CompiledSchedule::Interval { seconds } => {
                // Invert `from_ast`: pick the coarsest unit that divides
                // evenly, matching how `summary`/`format_schedule_line` choose.
                let (count, unit) = if *seconds >= 3600 && seconds % 3600 == 0 {
                    ((seconds / 3600) as u32, IntervalUnit::Hours)
                } else if *seconds >= 60 && seconds % 60 == 0 {
                    ((seconds / 60) as u32, IntervalUnit::Minutes)
                } else {
                    (*seconds as u32, IntervalUnit::Seconds)
                };
                ScheduleKind::Interval { count, unit }
            }
            CompiledSchedule::Daily { hour, minute } => ScheduleKind::Daily {
                time: time(*hour, *minute),
            },
            CompiledSchedule::Weekdays { days, hour, minute } => ScheduleKind::Weekdays {
                days: days.clone(),
                time: time(*hour, *minute),
            },
            CompiledSchedule::Monthly {
                ordinals,
                hour,
                minute,
            } => ScheduleKind::Monthly {
                ordinals: ordinals.clone(),
                time: time(*hour, *minute),
            },
            CompiledSchedule::Once { at } => ScheduleKind::Once {
                at: StringValue {
                    value: at.clone(),
                    quoted: true,
                    is_placeholder: false,
                    span: Span::empty(0),
                },
            },
            CompiledSchedule::Disabled => ScheduleKind::Disabled,
        }
    }
}

#[cfg(test)]
mod summary_tests {
    use super::*;

    #[test]
    fn interval_singular_pluralisation() {
        // Pin the n=1 fix — previously rendered "every 1 minutes".
        assert_eq!(
            CompiledSchedule::Interval { seconds: 60 }.summary(),
            "every 1 minute"
        );
        assert_eq!(
            CompiledSchedule::Interval { seconds: 3600 }.summary(),
            "every 1 hour"
        );
        assert_eq!(
            CompiledSchedule::Interval { seconds: 1 }.summary(),
            "every 1 second"
        );
    }

    #[test]
    fn interval_plural_unchanged() {
        assert_eq!(
            CompiledSchedule::Interval { seconds: 300 }.summary(),
            "every 5 minutes"
        );
        assert_eq!(
            CompiledSchedule::Interval { seconds: 7200 }.summary(),
            "every 2 hours"
        );
    }

    #[test]
    fn to_dsl_roundtrips_through_parser() {
        use crate::parser::parse_schedule_expr;

        // One case per schedule shape, including the ones whose `summary()`
        // form (comma-joined weekdays/ordinals) does NOT re-parse.
        let cases = [
            CompiledSchedule::Interval { seconds: 300 },
            CompiledSchedule::Interval { seconds: 60 },
            CompiledSchedule::Interval { seconds: 90 },
            CompiledSchedule::Interval { seconds: 3600 },
            CompiledSchedule::Daily { hour: 2, minute: 0 },
            CompiledSchedule::Weekdays {
                days: vec![Weekday::Monday, Weekday::Friday],
                hour: 9,
                minute: 0,
            },
            CompiledSchedule::Monthly {
                ordinals: vec![MonthOrdinal::Day(1), MonthOrdinal::Day(15)],
                hour: 10,
                minute: 0,
            },
            CompiledSchedule::Monthly {
                ordinals: vec![MonthOrdinal::Last],
                hour: 23,
                minute: 59,
            },
            CompiledSchedule::Once {
                at: "2026-04-01T03:00:00+00:00".to_string(),
            },
        ];

        for sched in cases {
            let dsl = sched.to_dsl();
            let kind = parse_schedule_expr(&dsl)
                .unwrap_or_else(|e| panic!("to_dsl output {dsl:?} did not re-parse: {e}"));
            // Re-emitting the re-parsed schedule must be a fixed point.
            let round = CompiledSchedule::from_ast(&kind).to_dsl();
            assert_eq!(round, dsl, "round-trip changed the canonical form");
        }
    }
}

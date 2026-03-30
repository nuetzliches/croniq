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
                if *seconds >= 3600 && seconds % 3600 == 0 {
                    format!("every {} hours", seconds / 3600)
                } else if *seconds >= 60 && seconds % 60 == 0 {
                    format!("every {} minutes", seconds / 60)
                } else {
                    format!("every {seconds} seconds")
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
}

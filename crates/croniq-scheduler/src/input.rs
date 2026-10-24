//! Write-time checks for schedule and calendar rows that arrive through an API
//! rather than through the Croniqfile.
//!
//! The Croniqfile gets its values checked by the compiler. A row written by
//! `POST /v1/schedules`, `PUT /v1/calendars/{id}` or one of the MCP tools that
//! mirror them only reaches the loader later, and the loader fails such a row
//! closed rather than refusing it: an unparseable `cron_expression` became "no
//! trigger" and the job silently left the scheduler, a typo in `window` became
//! "no window" (review 2026-10-24, P-6). The HTTP handlers and the MCP tools
//! each grew their own subset of checks, so the same row was refused on one
//! surface and accepted on the other (P-8).
//!
//! These are the stateless halves, in one place for both surfaces. The checks
//! that need server state (does the Croniqfile own this name, is this calendar
//! known) live with the handlers in `croniq-server` and reach the MCP through
//! its host hook. Each function treats `None` and an empty string as "unset",
//! the convention the update endpoints use to clear a field.

use crate::schedule::Schedule;
use crate::trigger::TimeWindow;

/// Why a value was refused: a stable machine-readable `code` (the `error`
/// field of the HTTP body) and a message for the person who sent it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputError {
    pub code: &'static str,
    pub message: String,
}

impl InputError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for InputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

/// Build a runtime [`Schedule`] from a persisted `cron_expression`.
///
/// API-created triggers store interval shorthand (`"5m"`, `"300"`,
/// `"*/5 * * * *"`); DSL-synthesized and adopted triggers store a canonical DSL
/// schedule line (`"every day at 02:00"`). The shorthand is tried first, then
/// the full DSL grammar. Malformed expressions yield `None`.
///
/// This is what the loader builds a stored trigger from, so [`check_schedule`]
/// accepting a value means the loader can schedule it.
pub fn schedule_from_expr(expr: &str) -> Option<Schedule> {
    if let Some(seconds) = parse_interval_seconds(expr) {
        return Some(Schedule::Interval { seconds });
    }
    let kind = croniq_config::parser::parse_schedule_expr(expr).ok()?;
    Schedule::from_ast(&kind).ok()
}

/// The interval shorthand an API trigger may carry: `"300"`, `"30s"`, `"5m"`,
/// `"1h"` or `"*/N * * * *"` (every N minutes).
pub fn parse_interval_seconds(s: &str) -> Option<u64> {
    let s = s.trim();
    if let Ok(n) = s.parse::<u64>() {
        return Some(n);
    }
    if let Some(n) = s.strip_suffix('s') {
        return n.parse().ok();
    }
    if let Some(n) = s.strip_suffix('m') {
        return n.parse::<u64>().ok().map(|v| v * 60);
    }
    if let Some(n) = s.strip_suffix('h') {
        return n.parse::<u64>().ok().map(|v| v * 3600);
    }
    if let Some(rest) = s.strip_prefix("*/")
        && let Some(n) = rest
            .split_whitespace()
            .next()
            .and_then(|n| n.parse::<u64>().ok())
    {
        return Some(n * 60);
    }
    None
}

/// Refuse a `cron_expression` the loader could not turn into a schedule
/// (P-6). Absent is allowed — a trigger row without an expression is how a
/// schedule is parked — but a blank one is not: it is the same "never fires"
/// as a typo, reached by a different keystroke.
pub fn check_schedule(expr: Option<&str>) -> Result<(), InputError> {
    let Some(expr) = expr else { return Ok(()) };
    let refuse = || {
        InputError::new(
            "invalid_schedule",
            format!(
                "schedule '{expr}' does not parse: use interval shorthand (\"30s\", \"5m\", \"1h\", \"*/5 * * * *\") or a Croniqfile schedule line (\"every day at 02:00\")"
            ),
        )
    };
    match schedule_from_expr(expr) {
        // An interval of zero would fire on every tick.
        Some(Schedule::Interval { seconds: 0 }) => Err(InputError::new(
            "invalid_schedule",
            "an interval schedule must be longer than zero seconds",
        )),
        Some(_) => Ok(()),
        None => Err(refuse()),
    }
}

/// Refuse a daily `window` that does not parse as `HH:MM..HH:MM` (P-6).
pub fn check_window(window: Option<&str>) -> Result<(), InputError> {
    let Some(window) = window.filter(|w| !w.is_empty()) else {
        return Ok(());
    };
    if TimeWindow::parse(window).is_some() {
        Ok(())
    } else {
        Err(InputError::new(
            "invalid_window",
            format!("window '{window}' does not parse: expected HH:MM..HH:MM, e.g. 02:00..06:00"),
        ))
    }
}

/// Refuse a `timezone` that is not an IANA zone name (issue #426), with the
/// did-you-mean from `croniq_config::timezone`.
pub fn check_timezone(timezone: Option<&str>) -> Result<(), InputError> {
    let Some(name) = timezone.filter(|t| !t.is_empty()) else {
        return Ok(());
    };
    croniq_config::timezone::parse(name)
        .map(|_| ())
        .map_err(|e| InputError::new("unknown_timezone", e.to_string()))
}

/// Validate free-form calendar rules with the calendar-body parser plus
/// semantic validation (#356).
///
/// The MCP tools used to run the parser only, so a rule the loader rejects
/// (`include weekly funday`) was refused over HTTP and stored over `/mcp`
/// (P-8).
pub fn check_calendar_rules(rules: &str) -> Result<(), InputError> {
    if rules.trim().is_empty() {
        return Ok(());
    }
    // A dedicated entry point rather than splicing the text into a synthetic
    // `calendar { … }` block: it refuses braces and placeholders, which a
    // stored calendar would otherwise resolve against the server's own
    // environment and files (review 2026-10-24, F-2/F-11).
    let ast = croniq_config::parser::Parser::parse_calendar_rules("__validate__", rules)
        .map_err(|e| InputError::new("invalid_rules", e.to_string()))?;
    let errors: Vec<String> = croniq_config::validate::validate(&ast)
        .into_iter()
        .filter(|d| d.severity == croniq_config::validate::Severity::Error)
        .map(|d| d.message)
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(InputError::new("invalid_rules", errors.join("; ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_accepts_shorthand_and_dsl_lines() {
        for ok in ["5m", "300", "1h", "*/5 * * * *", "every day at 02:00"] {
            assert_eq!(check_schedule(Some(ok)), Ok(()), "{ok}");
        }
        assert_eq!(check_schedule(None), Ok(()));
    }

    #[test]
    fn schedule_refuses_what_the_loader_cannot_build() {
        for bad in ["jeden Tag", "", "every day at 25:00", "0", "0m"] {
            let err = check_schedule(Some(bad)).unwrap_err();
            assert_eq!(err.code, "invalid_schedule", "{bad}");
        }
    }

    #[test]
    fn window_must_be_a_range_of_times() {
        assert_eq!(check_window(Some("02:00..06:00")), Ok(()));
        assert_eq!(check_window(Some("")), Ok(()));
        assert_eq!(check_window(None), Ok(()));
        assert_eq!(
            check_window(Some("02:00-06:00")).unwrap_err().code,
            "invalid_window"
        );
    }

    #[test]
    fn timezone_must_be_iana() {
        assert_eq!(check_timezone(Some("Europe/Berlin")), Ok(()));
        assert_eq!(check_timezone(Some("")), Ok(()));
        assert_eq!(
            check_timezone(Some("Europe/Berln")).unwrap_err().code,
            "unknown_timezone"
        );
    }

    #[test]
    fn calendar_rules_get_semantic_validation() {
        assert_eq!(check_calendar_rules("include weekly weekday"), Ok(()));
        assert_eq!(check_calendar_rules("   "), Ok(()));
        assert_eq!(
            check_calendar_rules("include weekly funday")
                .unwrap_err()
                .code,
            "invalid_rules"
        );
        assert_eq!(
            check_calendar_rules("include {").unwrap_err().code,
            "invalid_rules"
        );
    }
}

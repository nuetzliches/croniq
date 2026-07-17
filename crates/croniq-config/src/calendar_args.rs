//! Shared parsing of calendar rule arguments.
//!
//! Used by `validate` (offline) and by croniq-scheduler's `compile_rule`
//! (runtime) so the two cannot diverge again (#356). Error strings are
//! load-bearing: the scheduler wraps them verbatim in
//! `CalendarError::InvalidRule`, so changing them changes user-visible
//! loader diagnostics.

use crate::ast::Weekday;
use chrono::{NaiveDate, NaiveTime};

/// Parse a single `weekly` argument into one or more days.
///
/// Accepts the seven day names (full or 3-letter, case-insensitive) and
/// the `weekday`/`weekend` group aliases that `format` and the cron
/// converter emit.
pub fn parse_weekly_arg(s: &str) -> Result<&'static [Weekday], String> {
    Weekday::parse_token(s).ok_or_else(|| format!("unknown weekday: {s}"))
}

/// Parse `window` arguments into a `(start, end)` pair.
///
/// Accepts either a single `"HH:MM..HH:MM"` range argument or two
/// separate `"HH:MM"` arguments.
pub fn parse_window_args(args: &[String]) -> Result<(NaiveTime, NaiveTime), String> {
    if args.is_empty() {
        return Err("window requires start..end times".into());
    }
    let (start_str, end_str) = if args.len() == 1 {
        args[0]
            .split_once("..")
            .ok_or_else(|| format!("invalid window: {}", args[0]))?
    } else if args.len() == 2 {
        (args[0].as_str(), args[1].as_str())
    } else {
        return Err("window expects start..end or start end".into());
    };
    Ok((parse_time(start_str)?, parse_time(end_str)?))
}

/// Parse a single `monthly` argument into a day number.
///
/// Deliberately accepts ANY `u32` — the runtime always has (an
/// out-of-range day simply never matches), and validation must not be
/// stricter than the loader. Range plausibility is the caller's concern.
pub fn parse_monthly_arg(s: &str) -> Result<u32, String> {
    s.parse::<u32>().map_err(|_| format!("invalid day: {s}"))
}

/// A parsed `annual`/`yearly` argument.
#[derive(Debug)]
pub enum AnnualArg {
    /// `MM-DD`, recurring every year. No month/day range check —
    /// runtime parity (an implausible date simply never matches).
    MonthDay(u32, u32),
    /// `YYYY-MM-DD`, a specific date (chrono-validated).
    Date(NaiveDate),
    /// A len-5 argument containing `-` that doesn't split into exactly
    /// two parts (e.g. `1-2-3`). The runtime has always silently
    /// skipped these; modelled explicitly so the loader keeps accepting
    /// configs it accepted before #356.
    Ignored,
}

/// Parse a single `annual`/`yearly` argument.
pub fn parse_annual_arg(s: &str) -> Result<AnnualArg, String> {
    if s.len() == 5 && s.contains('-') {
        // MM-DD format
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() != 2 {
            return Ok(AnnualArg::Ignored);
        }
        let month: u32 = parts[0].parse().map_err(|_| format!("invalid date: {s}"))?;
        let day: u32 = parts[1].parse().map_err(|_| format!("invalid date: {s}"))?;
        Ok(AnnualArg::MonthDay(month, day))
    } else if s.len() == 10 && s.chars().filter(|c| *c == '-').count() == 2 {
        // YYYY-MM-DD format — specific date
        NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map(AnnualArg::Date)
            .map_err(|_| format!("invalid date: {s}"))
    } else {
        Err(format!(
            "invalid annual date format: {s} (expected MM-DD or YYYY-MM-DD)"
        ))
    }
}

fn parse_time(s: &str) -> Result<NaiveTime, String> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 {
        return Err(format!("invalid time: {s}"));
    }
    let hour: u32 = parts[0].parse().map_err(|_| format!("invalid time: {s}"))?;
    let minute: u32 = parts[1].parse().map_err(|_| format!("invalid time: {s}"))?;
    NaiveTime::from_hms_opt(hour, minute, 0).ok_or_else(|| format!("invalid time: {s}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weekly_arg_aliases_and_days() {
        assert_eq!(parse_weekly_arg("weekday").unwrap().len(), 5);
        assert_eq!(parse_weekly_arg("weekend").unwrap().len(), 2);
        assert_eq!(parse_weekly_arg("Sun").unwrap(), &[Weekday::Sunday]);
        assert_eq!(
            parse_weekly_arg("funday").unwrap_err(),
            "unknown weekday: funday"
        );
    }

    #[test]
    fn window_args_range_and_pair_forms() {
        let range = parse_window_args(&["08:00..18:00".into()]).unwrap();
        let pair = parse_window_args(&["08:00".into(), "18:00".into()]).unwrap();
        assert_eq!(range, pair);

        assert_eq!(
            parse_window_args(&[]).unwrap_err(),
            "window requires start..end times"
        );
        assert_eq!(
            parse_window_args(&["08:00".into()]).unwrap_err(),
            "invalid window: 08:00"
        );
        assert_eq!(
            parse_window_args(&["a".into(), "b".into(), "c".into()]).unwrap_err(),
            "window expects start..end or start end"
        );
        assert_eq!(
            parse_window_args(&["25:00..26:00".into()]).unwrap_err(),
            "invalid time: 25:00"
        );
    }

    #[test]
    fn monthly_arg_numeric_only() {
        assert_eq!(parse_monthly_arg("15").unwrap(), 15);
        // Out-of-range days parse — runtime parity, the rule is inert.
        assert_eq!(parse_monthly_arg("45").unwrap(), 45);
        assert_eq!(parse_monthly_arg("foo").unwrap_err(), "invalid day: foo");
    }

    #[test]
    fn annual_arg_shapes() {
        assert!(matches!(
            parse_annual_arg("12-25").unwrap(),
            AnnualArg::MonthDay(12, 25)
        ));
        assert!(matches!(
            parse_annual_arg("2026-04-06").unwrap(),
            AnnualArg::Date(_)
        ));
        // Implausible MM-DD still parses — runtime parity.
        assert!(matches!(
            parse_annual_arg("13-45").unwrap(),
            AnnualArg::MonthDay(13, 45)
        ));
        // Len-5 with too many dashes is silently skipped by the runtime.
        assert!(matches!(
            parse_annual_arg("1-2-3").unwrap(),
            AnnualArg::Ignored
        ));
        assert_eq!(
            parse_annual_arg("2026-02-30").unwrap_err(),
            "invalid date: 2026-02-30"
        );
        assert_eq!(
            parse_annual_arg("aa-bb").unwrap_err(),
            "invalid date: aa-bb"
        );
        assert_eq!(
            parse_annual_arg("junk").unwrap_err(),
            "invalid annual date format: junk (expected MM-DD or YYYY-MM-DD)"
        );
        // "12-5" is len 4 — falls through to the format error.
        assert_eq!(
            parse_annual_arg("12-5").unwrap_err(),
            "invalid annual date format: 12-5 (expected MM-DD or YYYY-MM-DD)"
        );
    }
}

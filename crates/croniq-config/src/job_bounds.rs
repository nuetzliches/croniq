//! Shared parsing of a job's time constraints: the job-level `window`, the
//! `not_before` / `not_after` schedule options and the `once at` datetime.
//!
//! Used by `validate` (offline) and by the server's loader (runtime) so that
//! what `croniq validate` accepts is exactly what the server can apply — the
//! same arrangement as [`crate::calendar_args`] (#356). Before this the loader
//! parsed all four with `.ok()` and dropped what did not parse: a
//! `not_after "2026-12-31"` ran forever and `window 8:00-18:00` ran around the
//! clock, both with a green `validate` (review 2026-10-24, D-8 / F-5).

use chrono::{DateTime, NaiveDateTime, NaiveTime, TimeZone, Utc};

/// Parse a job-level `window "HH:MM..HH:MM"` value.
///
/// The job gate takes one range argument; the two-argument form is a calendar
/// rule spelling only. A start after the end wraps past midnight.
pub fn parse_job_window(s: &str) -> Result<(NaiveTime, NaiveTime), String> {
    let (start, end) = s
        .split_once("..")
        .ok_or_else(|| format!("invalid window `{s}`: write it as `HH:MM..HH:MM`"))?;
    let time = |t: &str| {
        crate::calendar_args::parse_time(t)
            .map_err(|_| format!("invalid window `{s}`: `{t}` is not a time of day (HH:MM)"))
    };
    Ok((time(start)?, time(end)?))
}

/// Parse a `not_before` / `not_after` bound. RFC 3339 only — the bound is an
/// absolute instant, so it must carry its offset.
pub fn parse_bound(s: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|_| {
            format!(
                "invalid bound `{s}`: write an RFC 3339 timestamp with an offset, \
                 e.g. `2026-12-31T23:59:59Z`"
            )
        })
}

/// Parse a `once at` datetime: RFC 3339, or `YYYY-MM-DDTHH:MM:SS` read as UTC.
///
/// Mirrors `croniq_scheduler::schedule::parse_datetime`; the server's loader
/// tests pin the two against each other.
pub fn parse_once_at(s: &str) -> Result<DateTime<Utc>, String> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Ok(Utc.from_utc_datetime(&dt));
    }
    Err(format!(
        "invalid `once at` datetime `{s}`: write an existing date and time, \
         e.g. `2026-12-31T23:59:59Z`"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_accepts_ranges_and_wraparound() {
        assert!(parse_job_window("08:00..18:00").is_ok());
        assert!(parse_job_window("8:00..18:00").is_ok());
        assert!(parse_job_window("22:00..06:00").is_ok());
    }

    #[test]
    fn window_rejects_other_spellings() {
        for bad in ["8:00-18:00", "08:00..25:00", "08:00", "08:00..", "x..y"] {
            assert!(parse_job_window(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn bound_requires_rfc3339() {
        assert!(parse_bound("2026-12-31T23:59:59Z").is_ok());
        assert!(parse_bound("2026-12-31T23:59:59+01:00").is_ok());
        assert!(parse_bound("2026-12-31").is_err());
        assert!(parse_bound("2026-12-31T23:59:59").is_err());
    }

    #[test]
    fn once_at_rejects_impossible_dates() {
        assert!(parse_once_at("2026-04-30T03:00:00Z").is_ok());
        assert!(parse_once_at("2026-04-30T03:00:00").is_ok());
        assert!(parse_once_at("2026-04-31T03:00:00Z").is_err());
        assert!(parse_once_at("tomorrow").is_err());
    }
}

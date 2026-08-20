//! Seconds-granularity duration parsing for the server's config and env knobs.
//!
//! The grammar itself lives in [`croniq_execution::retry::parse_duration_checked`]
//! — the workspace's single duration parser. This module only adapts it for the
//! settings measured in whole seconds (lease TTLs, dedup windows, the API-key
//! rotation grace), which is also why it rejects sub-second values instead of
//! truncating `500ms` into a zero-second lease nobody asked for.

/// Parse a duration string like `"60s"`, `"2m"`, `"1h"`, `"7d"`, or a bare
/// integer (interpreted as seconds) into seconds. Returns an error string on
/// malformed input rather than silently falling back, so that bad config
/// surfaces at boot instead of becoming a 2-minute lease nobody asked for.
pub fn parse_duration_secs(s: &str) -> Result<u64, String> {
    let parsed = croniq_execution::retry::parse_duration_checked(s)?;
    if parsed.subsec_nanos() != 0 {
        return Err(format!(
            "duration {s:?} is not a whole number of seconds — use '<n>[s|m|h|d]'"
        ));
    }
    Ok(parsed.as_secs())
}

#[cfg(test)]
mod tests {
    use super::parse_duration_secs;

    #[test]
    fn parses_units_and_bare_seconds() {
        assert_eq!(parse_duration_secs("30s").unwrap(), 30);
        assert_eq!(parse_duration_secs("2m").unwrap(), 120);
        assert_eq!(parse_duration_secs("1h").unwrap(), 3600);
        assert_eq!(parse_duration_secs("45").unwrap(), 45);
        assert_eq!(parse_duration_secs("  10s  ").unwrap(), 10);
    }

    #[test]
    fn accepts_days_like_the_retention_knobs_do() {
        // The whole point of #486: `1d` used to be an error here while
        // `execution_retention 30d` was accepted a few lines away in main.rs.
        assert_eq!(parse_duration_secs("1d").unwrap(), 86_400);
        assert_eq!(parse_duration_secs("30d").unwrap(), 2_592_000);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_duration_secs("abc").is_err());
        assert!(parse_duration_secs("10x").is_err());
        assert!(parse_duration_secs("").is_err());
    }

    #[test]
    fn rejects_sub_second_values_instead_of_truncating_to_zero() {
        // The shared grammar understands `ms`, but a lease TTL or rotation
        // grace of "500ms" would truncate to 0 seconds — i.e. an immediately
        // expired lease / instantly revoked key. Say so instead.
        let err = parse_duration_secs("500ms").unwrap_err();
        assert!(err.contains("whole number of seconds"), "got: {err}");
        assert!(parse_duration_secs("1ms").is_err());
        // A whole-second value expressed in milliseconds is still fine.
        assert_eq!(parse_duration_secs("2000ms").unwrap(), 2);
    }
}

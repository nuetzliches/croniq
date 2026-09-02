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

/// Last-resort execution timeout: applies only when neither the execution, nor
/// its job, nor the caller declared one.
pub const DEFAULT_EXECUTION_TIMEOUT: &str = "5m";

/// The timeout in force for a persisted execution.
///
/// Precedence is [`TIMEOUT_METADATA_KEY`](croniq_config::compile::TIMEOUT_METADATA_KEY)
/// on the row → the job's configured `timeout` → [`DEFAULT_EXECUTION_TIMEOUT`].
/// The row wins because the timeout can be overridden per fire
/// (`POST /v1/trigger`, the MCP fire tools), so the job config describes future
/// fires rather than this one; it stays the fallback for rows written before the
/// stamp existed (issue #558).
///
/// A blank or unparseable value falls through to the next source rather than
/// being honoured, matching how the trigger path treats a blank `timeout`
/// (issue #553). Without that, a malformed stamp would shadow a good job config
/// and the reaper would judge a 2h job by the 5m default — reaping live work.
pub fn effective_timeout(
    metadata: &std::collections::HashMap<String, String>,
    job_timeout: Option<&str>,
) -> String {
    metadata
        .get(croniq_config::compile::TIMEOUT_METADATA_KEY)
        .map(String::as_str)
        .into_iter()
        .chain(job_timeout)
        .map(str::trim)
        .find(|t| croniq_execution::retry::parse_duration(t).is_some())
        .unwrap_or(DEFAULT_EXECUTION_TIMEOUT)
        .to_string()
}

/// [`effective_timeout`] in whole seconds, for the reaper's age threshold.
pub fn effective_timeout_secs(
    metadata: &std::collections::HashMap<String, String>,
    job_timeout: Option<&str>,
) -> u64 {
    croniq_execution::retry::parse_duration(&effective_timeout(metadata, job_timeout))
        .map(|d| d.as_secs())
        // Unreachable while DEFAULT_EXECUTION_TIMEOUT parses; kept explicit so a
        // typo in that constant degrades to 5m rather than to zero, which would
        // make the reaper treat every live claim as wedged.
        .unwrap_or(300)
}

#[cfg(test)]
mod tests {
    use super::parse_duration_secs;
    use super::{DEFAULT_EXECUTION_TIMEOUT, effective_timeout, effective_timeout_secs};
    use croniq_config::compile::TIMEOUT_METADATA_KEY;
    use std::collections::HashMap;

    fn meta(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn stamped_timeout_wins_over_job_config() {
        // The whole point of the stamp (#558): the row records what was in force
        // for THIS execution, which a per-fire override can make differ from the
        // job's configured value.
        let m = meta(&[(TIMEOUT_METADATA_KEY, "4h")]);
        assert_eq!(effective_timeout(&m, Some("30s")), "4h");
        assert_eq!(effective_timeout_secs(&m, Some("30s")), 14_400);
    }

    #[test]
    fn falls_back_to_job_config_without_a_stamp() {
        // Rows written before the stamp existed must keep reaping on the job's
        // timeout rather than collapsing to the 5m default.
        let m = meta(&[]);
        assert_eq!(effective_timeout(&m, Some("2h")), "2h");
        assert_eq!(effective_timeout_secs(&m, Some("2h")), 7_200);
    }

    #[test]
    fn falls_back_to_default_without_either() {
        assert_eq!(
            effective_timeout(&meta(&[]), None),
            DEFAULT_EXECUTION_TIMEOUT
        );
        assert_eq!(effective_timeout_secs(&meta(&[]), None), 300);
    }

    #[test]
    fn unusable_stamp_falls_through_instead_of_being_honoured() {
        // A blank or malformed stamp must not shadow a good job config — the
        // reaper would otherwise judge a 2h job by 5m and reap live work.
        for bad in ["", "   ", "5min", "abc"] {
            let m = meta(&[(TIMEOUT_METADATA_KEY, bad)]);
            assert_eq!(
                effective_timeout(&m, Some("2h")),
                "2h",
                "stamp {bad:?} must fall through to the job config"
            );
        }
    }

    #[test]
    fn unusable_job_config_falls_through_to_default() {
        assert_eq!(
            effective_timeout(&meta(&[]), Some("5min")),
            DEFAULT_EXECUTION_TIMEOUT
        );
    }

    #[test]
    fn stamp_is_trimmed() {
        let m = meta(&[(TIMEOUT_METADATA_KEY, "  90s  ")]);
        assert_eq!(effective_timeout(&m, None), "90s");
    }

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

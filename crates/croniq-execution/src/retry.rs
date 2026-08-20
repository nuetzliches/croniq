//! Retry strategies: exponential, linear, fixed backoff with jitter.

use std::time::Duration;

/// Retry policy configuration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RetryPolicy {
    pub strategy: RetryStrategy,
    pub max_attempts: u32,
    /// Jitter factor 0.0..1.0 — randomizes delay to prevent thundering herd.
    pub jitter: f64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum RetryStrategy {
    /// Exponential backoff: delay = base * 2^(attempt-1), capped at `cap`.
    Exponential { base: Duration, cap: Duration },
    /// Fixed delay between retries.
    Fixed { delay: Duration },
    /// Linear backoff: delay = base + step * (attempt-1), capped at `cap`.
    Linear {
        base: Duration,
        step: Duration,
        cap: Duration,
    },
}

/// Result of evaluating a retry decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryDecision {
    /// Retry after the given delay.
    RetryAfter(Duration),
    /// No more retries — move to dead letter.
    Exhausted,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            strategy: RetryStrategy::Exponential {
                base: Duration::from_secs(2),
                cap: Duration::from_secs(30),
            },
            max_attempts: 3,
            jitter: 0.25,
        }
    }
}

impl RetryPolicy {
    /// Decide whether to retry given the current attempt number.
    /// `attempt` is 1-based (first execution = attempt 1, first retry = attempt 2).
    pub fn evaluate(&self, attempt: u32) -> RetryDecision {
        if attempt >= self.max_attempts {
            return RetryDecision::Exhausted;
        }

        let base_delay = self.compute_delay(attempt);
        let jittered = apply_jitter(base_delay, self.jitter);
        RetryDecision::RetryAfter(jittered)
    }

    /// Compute the delay without jitter (deterministic, for testing).
    pub fn compute_delay(&self, attempt: u32) -> Duration {
        let retry_num = attempt; // attempt 1 = first retry
        match &self.strategy {
            RetryStrategy::Exponential { base, cap } => {
                let multiplier = 2u64.saturating_pow(retry_num - 1);
                let delay = base.saturating_mul(multiplier as u32);
                std::cmp::min(delay, *cap)
            }
            RetryStrategy::Fixed { delay } => *delay,
            RetryStrategy::Linear { base, step, cap } => {
                let additional = step.saturating_mul(retry_num - 1);
                let delay = base.saturating_add(additional);
                std::cmp::min(delay, *cap)
            }
        }
    }

    /// Create an exponential retry policy.
    pub fn exponential(max_attempts: u32, base: Duration, cap: Duration, jitter: f64) -> Self {
        Self {
            strategy: RetryStrategy::Exponential { base, cap },
            max_attempts,
            jitter,
        }
    }

    /// Create a fixed-delay retry policy.
    pub fn fixed(max_attempts: u32, delay: Duration) -> Self {
        Self {
            strategy: RetryStrategy::Fixed { delay },
            max_attempts,
            jitter: 0.0,
        }
    }

    /// Create a linear backoff retry policy.
    pub fn linear(max_attempts: u32, base: Duration, step: Duration, cap: Duration) -> Self {
        Self {
            strategy: RetryStrategy::Linear { base, step, cap },
            max_attempts,
            jitter: 0.0,
        }
    }

    /// No retries.
    pub fn none() -> Self {
        Self {
            strategy: RetryStrategy::Fixed {
                delay: Duration::ZERO,
            },
            max_attempts: 1,
            jitter: 0.0,
        }
    }
}

/// Apply jitter to a duration. Jitter factor 0.0 = no jitter, 1.0 = ±100%.
fn apply_jitter(duration: Duration, jitter: f64) -> Duration {
    if jitter <= 0.0 || duration.is_zero() {
        return duration;
    }

    let jitter = jitter.clamp(0.0, 1.0);
    let millis = duration.as_millis() as f64;
    let variance = millis * jitter;
    let random: f64 = rand::random();
    let offset = variance * (2.0 * random - 1.0); // -variance..+variance
    let jittered = (millis + offset).max(0.0);
    Duration::from_millis(jittered as u64)
}

/// Parse a duration string like "2s", "500ms", "5m", "1h", "30d", or a bare
/// integer (seconds).
///
/// This is the one duration grammar for the whole workspace: config
/// directives, env vars and API payloads all resolve through here, so an
/// operator never has to remember which knob accepts which units.
/// [`parse_duration`] is the `Option` flavour for callers that fall back to a
/// default; this one reports *why* the value was rejected, for the paths that
/// must fail loudly instead (bad config at boot).
pub fn parse_duration_checked(s: &str) -> Result<Duration, String> {
    let trimmed = s.trim();
    // Longest suffix first: "ms" must win over "s".
    let (digits, unit, millis_per_unit) = if let Some(v) = trimmed.strip_suffix("ms") {
        (v, "ms", 1)
    } else if let Some(v) = trimmed.strip_suffix('s') {
        (v, "s", 1_000)
    } else if let Some(v) = trimmed.strip_suffix('m') {
        (v, "m", 60 * 1_000)
    } else if let Some(v) = trimmed.strip_suffix('h') {
        (v, "h", 3_600 * 1_000)
    } else if let Some(v) = trimmed.strip_suffix('d') {
        (v, "d", 86_400 * 1_000)
    } else {
        (trimmed, "", 1_000)
    };
    if digits.is_empty() {
        return Err(format!(
            "invalid duration {s:?}: expected '<n>[ms|s|m|h|d]' or bare seconds"
        ));
    }
    let value: u64 = digits.parse().map_err(|_| {
        if unit.is_empty() {
            format!("invalid duration {s:?}: expected '<n>[ms|s|m|h|d]' or bare seconds")
        } else {
            format!("invalid duration {s:?}: cannot parse number before '{unit}'")
        }
    })?;
    value
        .checked_mul(millis_per_unit)
        .map(Duration::from_millis)
        .ok_or_else(|| format!("duration {s:?} overflows the representable range"))
}

/// Parse a duration string like "2s", "500ms", "5m", "1h", returning `None`
/// on malformed input. Thin wrapper over [`parse_duration_checked`] for the
/// call sites that fall back to a default instead of surfacing the reason.
pub fn parse_duration(s: &str) -> Option<Duration> {
    parse_duration_checked(s).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exponential_backoff_delays() {
        let policy = RetryPolicy {
            strategy: RetryStrategy::Exponential {
                base: Duration::from_secs(2),
                cap: Duration::from_secs(30),
            },
            max_attempts: 5,
            jitter: 0.0, // no jitter for deterministic test
        };

        assert_eq!(policy.compute_delay(1), Duration::from_secs(2)); // 2 * 2^0
        assert_eq!(policy.compute_delay(2), Duration::from_secs(4)); // 2 * 2^1
        assert_eq!(policy.compute_delay(3), Duration::from_secs(8)); // 2 * 2^2
        assert_eq!(policy.compute_delay(4), Duration::from_secs(16)); // 2 * 2^3
        assert_eq!(policy.compute_delay(5), Duration::from_secs(30)); // capped
    }

    #[test]
    fn exponential_evaluate() {
        let policy =
            RetryPolicy::exponential(3, Duration::from_secs(2), Duration::from_secs(30), 0.0);

        assert_eq!(
            policy.evaluate(1),
            RetryDecision::RetryAfter(Duration::from_secs(2))
        );
        assert_eq!(
            policy.evaluate(2),
            RetryDecision::RetryAfter(Duration::from_secs(4))
        );
        assert_eq!(policy.evaluate(3), RetryDecision::Exhausted);
    }

    #[test]
    fn fixed_delay() {
        let policy = RetryPolicy::fixed(3, Duration::from_secs(10));

        assert_eq!(policy.compute_delay(1), Duration::from_secs(10));
        assert_eq!(policy.compute_delay(2), Duration::from_secs(10));
        assert_eq!(policy.compute_delay(3), Duration::from_secs(10));
    }

    #[test]
    fn linear_backoff() {
        let policy = RetryPolicy::linear(
            5,
            Duration::from_secs(5),
            Duration::from_secs(5),
            Duration::from_secs(60),
        );

        assert_eq!(policy.compute_delay(1), Duration::from_secs(5)); // 5 + 5*0
        assert_eq!(policy.compute_delay(2), Duration::from_secs(10)); // 5 + 5*1
        assert_eq!(policy.compute_delay(3), Duration::from_secs(15)); // 5 + 5*2
    }

    #[test]
    fn linear_cap() {
        let policy = RetryPolicy::linear(
            100,
            Duration::from_secs(5),
            Duration::from_secs(10),
            Duration::from_secs(25),
        );

        assert_eq!(policy.compute_delay(1), Duration::from_secs(5));
        assert_eq!(policy.compute_delay(2), Duration::from_secs(15));
        assert_eq!(policy.compute_delay(3), Duration::from_secs(25)); // capped
        assert_eq!(policy.compute_delay(4), Duration::from_secs(25)); // still capped
    }

    #[test]
    fn no_retries() {
        let policy = RetryPolicy::none();
        assert_eq!(policy.evaluate(1), RetryDecision::Exhausted);
    }

    #[test]
    fn jitter_stays_in_range() {
        let policy =
            RetryPolicy::exponential(5, Duration::from_secs(10), Duration::from_secs(60), 0.5);

        // Run many times to check jitter range
        for _ in 0..100 {
            if let RetryDecision::RetryAfter(delay) = policy.evaluate(1) {
                // Base is 10s, jitter 0.5 → range 5s..15s
                assert!(delay >= Duration::from_secs(5));
                assert!(delay <= Duration::from_secs(15));
            }
        }
    }

    #[test]
    fn zero_jitter_is_deterministic() {
        let d = apply_jitter(Duration::from_secs(10), 0.0);
        assert_eq!(d, Duration::from_secs(10));
    }

    // ─── parse_duration ───

    #[test]
    fn parse_durations() {
        assert_eq!(parse_duration("500ms"), Some(Duration::from_millis(500)));
        assert_eq!(parse_duration("2s"), Some(Duration::from_secs(2)));
        assert_eq!(parse_duration("5m"), Some(Duration::from_secs(300)));
        assert_eq!(parse_duration("1h"), Some(Duration::from_secs(3600)));
        assert_eq!(parse_duration("30d"), Some(Duration::from_secs(2592000)));
    }

    #[test]
    fn parse_duration_invalid() {
        assert_eq!(parse_duration("abc"), None);
        assert_eq!(parse_duration(""), None);
    }

    #[test]
    fn checked_parse_reports_why_it_failed() {
        assert!(
            parse_duration_checked("10x")
                .unwrap_err()
                .contains("expected")
        );
        assert!(
            parse_duration_checked("abcm")
                .unwrap_err()
                .contains("before 'm'")
        );
        // A unit with no number is not a zero-length duration.
        assert!(parse_duration_checked("s").is_err());
        assert!(parse_duration_checked("ms").is_err());
        assert!(parse_duration_checked("  ").is_err());
    }

    #[test]
    fn checked_parse_rejects_overflow_instead_of_wrapping() {
        // u64 seconds still fit, but the millisecond conversion does not:
        // the pre-#486 parser multiplied unchecked and wrapped (or panicked
        // in a debug build) instead of reporting the bad value.
        let err = parse_duration_checked("999999999999999999d").unwrap_err();
        assert!(err.contains("overflows"), "unexpected error: {err}");
    }

    #[test]
    fn checked_parse_accepts_the_full_grammar() {
        assert_eq!(
            parse_duration_checked("500ms").unwrap(),
            Duration::from_millis(500)
        );
        assert_eq!(
            parse_duration_checked("45").unwrap(),
            Duration::from_secs(45)
        );
        assert_eq!(
            parse_duration_checked("  10s  ").unwrap(),
            Duration::from_secs(10)
        );
        assert_eq!(
            parse_duration_checked("30d").unwrap(),
            Duration::from_secs(2_592_000)
        );
    }
}

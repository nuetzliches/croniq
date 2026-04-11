//! Timeout policy for job executions.

use std::time::Duration;

use crate::retry::parse_duration;

/// Timeout configuration for a job execution.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TimeoutPolicy {
    /// Maximum execution duration before the job is considered timed out.
    pub duration: Duration,
}

impl Default for TimeoutPolicy {
    fn default() -> Self {
        Self {
            duration: Duration::from_secs(300), // 5 minutes
        }
    }
}

impl TimeoutPolicy {
    pub fn new(duration: Duration) -> Self {
        Self { duration }
    }

    /// Parse from a duration string like "5m", "30s", "1h".
    pub fn from_str(s: &str) -> Option<Self> {
        parse_duration(s).map(|d| Self { duration: d })
    }

    /// Check if an execution has exceeded the timeout.
    pub fn is_expired(&self, elapsed: Duration) -> bool {
        elapsed >= self.duration
    }

    /// Remaining time before timeout, or zero if already expired.
    pub fn remaining(&self, elapsed: Duration) -> Duration {
        self.duration.saturating_sub(elapsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_5_minutes() {
        let policy = TimeoutPolicy::default();
        assert_eq!(policy.duration, Duration::from_secs(300));
    }

    #[test]
    fn not_expired() {
        let policy = TimeoutPolicy::new(Duration::from_secs(60));
        assert!(!policy.is_expired(Duration::from_secs(30)));
    }

    #[test]
    fn expired() {
        let policy = TimeoutPolicy::new(Duration::from_secs(60));
        assert!(policy.is_expired(Duration::from_secs(60)));
        assert!(policy.is_expired(Duration::from_secs(120)));
    }

    #[test]
    fn remaining_time() {
        let policy = TimeoutPolicy::new(Duration::from_secs(60));
        assert_eq!(policy.remaining(Duration::from_secs(20)), Duration::from_secs(40));
        assert_eq!(policy.remaining(Duration::from_secs(60)), Duration::ZERO);
        assert_eq!(policy.remaining(Duration::from_secs(90)), Duration::ZERO);
    }

    #[test]
    fn from_string() {
        let policy = TimeoutPolicy::from_str("15m").unwrap();
        assert_eq!(policy.duration, Duration::from_secs(900));
    }
}

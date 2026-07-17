//! Dead letter policy: what happens when retries are exhausted.

use std::time::Duration;

use crate::retry::parse_duration;

/// Dead letter configuration for a job.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeadLetterPolicy {
    /// Whether dead-lettering is enabled.
    pub enabled: bool,
    /// How long to retain dead letters before auto-purge.
    pub retention: Duration,
    /// Human-readable hint for the operator on how to resolve.
    pub operator_hint: Option<String>,
    /// Opt-in staleness guard: reject replaying a dead letter whose original
    /// `scheduled_for` is older than this (unless forced). `None` = no guard.
    pub replay_max_age: Option<Duration>,
}

impl Default for DeadLetterPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            retention: Duration::from_secs(30 * 86400), // 30 days
            operator_hint: None,
            replay_max_age: None,
        }
    }
}

impl DeadLetterPolicy {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            retention: Duration::ZERO,
            operator_hint: None,
            replay_max_age: None,
        }
    }

    pub fn with_retention(mut self, retention: &str) -> Self {
        if let Some(d) = parse_duration(retention) {
            self.retention = d;
        }
        self
    }

    pub fn with_hint(mut self, hint: &str) -> Self {
        self.operator_hint = Some(hint.to_string());
        self
    }

    /// Set the replay staleness guard from a duration string (e.g. `"7d"`).
    /// A string that fails to parse leaves the guard unset.
    pub fn with_replay_max_age(mut self, max_age: &str) -> Self {
        self.replay_max_age = parse_duration(max_age);
        self
    }

    /// Compute the expiry time for a dead letter created at `now`.
    pub fn expires_at(
        &self,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Option<chrono::DateTime<chrono::Utc>> {
        if !self.enabled || self.retention.is_zero() {
            return None;
        }
        Some(now + self.retention)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    #[test]
    fn default_30_days() {
        let policy = DeadLetterPolicy::default();
        assert!(policy.enabled);
        assert_eq!(policy.retention, Duration::from_secs(30 * 86400));
    }

    #[test]
    fn disabled_policy() {
        let policy = DeadLetterPolicy::disabled();
        assert!(!policy.enabled);
    }

    #[test]
    fn expires_at_computation() {
        let policy = DeadLetterPolicy::default().with_retention("7d");
        let now = Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 0).unwrap();
        let expires = policy.expires_at(now).unwrap();
        let expected = Utc.with_ymd_and_hms(2026, 4, 6, 0, 0, 0).unwrap();
        assert_eq!(expires, expected);
    }

    #[test]
    fn disabled_no_expiry() {
        let policy = DeadLetterPolicy::disabled();
        let now = Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 0).unwrap();
        assert!(policy.expires_at(now).is_none());
    }

    #[test]
    fn with_hint() {
        let policy = DeadLetterPolicy::default().with_hint("Check DB connectivity");
        assert_eq!(
            policy.operator_hint.as_deref(),
            Some("Check DB connectivity")
        );
    }

    #[test]
    fn replay_max_age_default_none() {
        assert!(DeadLetterPolicy::default().replay_max_age.is_none());
    }

    #[test]
    fn with_replay_max_age_parses() {
        let policy = DeadLetterPolicy::default().with_replay_max_age("7d");
        assert_eq!(policy.replay_max_age, Some(Duration::from_secs(7 * 86400)));
    }

    #[test]
    fn with_replay_max_age_invalid_stays_none() {
        let policy = DeadLetterPolicy::default().with_replay_max_age("not-a-duration");
        assert!(policy.replay_max_age.is_none());
    }
}

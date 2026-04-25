//! Execution pipeline: combines retry, timeout, and dead-letter policies
//! into a single decision engine.
//!
//! The pipeline evaluates what should happen after a job execution completes
//! (or times out) — retry, dead-letter, or done.

use std::time::Duration;

use crate::dead_letter::DeadLetterPolicy;
use crate::retry::{RetryDecision, RetryPolicy};
use crate::timeout::TimeoutPolicy;

/// Complete execution policy set for a job.
#[derive(Debug, Clone, Default)]
pub struct ExecutionPolicy {
    pub retry: RetryPolicy,
    pub timeout: TimeoutPolicy,
    pub dead_letter: DeadLetterPolicy,
}

/// The outcome of a completed execution.
#[derive(Debug, Clone, PartialEq)]
pub enum ExecutionOutcome {
    /// Job completed successfully.
    Success,

    /// Job failed, will be retried after the given delay.
    Retry { next_attempt: u32, delay: Duration },

    /// Job failed, all retries exhausted — moved to dead letter queue.
    DeadLetter {
        reason: String,
        operator_hint: Option<String>,
        expires_after: Option<Duration>,
    },

    /// Job failed, dead-lettering disabled — execution is simply dropped.
    Dropped { reason: String },

    /// Job was cancelled.
    Cancelled,
}

/// Result reported by a runner.
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub success: bool,
    pub error: Option<String>,
    pub duration: Duration,
    pub attempt: u32,
}

impl ExecutionPolicy {
    /// Evaluate the outcome of an execution result.
    pub fn evaluate(&self, result: &ExecutionResult) -> ExecutionOutcome {
        // Success → done
        if result.success {
            return ExecutionOutcome::Success;
        }

        // Check if timed out
        let timed_out = self.timeout.is_expired(result.duration);
        let error = if timed_out {
            result
                .error
                .clone()
                .unwrap_or_else(|| "execution timed out".to_string())
        } else {
            result
                .error
                .clone()
                .unwrap_or_else(|| "unknown error".to_string())
        };

        // Check retry
        match self.retry.evaluate(result.attempt) {
            RetryDecision::RetryAfter(delay) => ExecutionOutcome::Retry {
                next_attempt: result.attempt + 1,
                delay,
            },
            RetryDecision::Exhausted => {
                let reason = format!("exhausted after {} attempts: {}", result.attempt, error);

                if self.dead_letter.enabled {
                    ExecutionOutcome::DeadLetter {
                        reason,
                        operator_hint: self.dead_letter.operator_hint.clone(),
                        expires_after: Some(self.dead_letter.retention),
                    }
                } else {
                    ExecutionOutcome::Dropped { reason }
                }
            }
        }
    }

    /// Check if a running execution has exceeded its timeout.
    pub fn check_timeout(&self, elapsed: Duration) -> bool {
        self.timeout.is_expired(elapsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_policy() -> ExecutionPolicy {
        ExecutionPolicy::default()
    }

    fn policy_with_retries(max: u32) -> ExecutionPolicy {
        ExecutionPolicy {
            retry: RetryPolicy::fixed(max, Duration::from_secs(5)),
            ..Default::default()
        }
    }

    #[test]
    fn success_outcome() {
        let policy = default_policy();
        let result = ExecutionResult {
            success: true,
            error: None,
            duration: Duration::from_secs(10),
            attempt: 1,
        };
        assert_eq!(policy.evaluate(&result), ExecutionOutcome::Success);
    }

    #[test]
    fn failure_triggers_retry() {
        let policy = policy_with_retries(3);
        let result = ExecutionResult {
            success: false,
            error: Some("connection refused".into()),
            duration: Duration::from_secs(1),
            attempt: 1,
        };

        match policy.evaluate(&result) {
            ExecutionOutcome::Retry {
                next_attempt,
                delay,
            } => {
                assert_eq!(next_attempt, 2);
                assert_eq!(delay, Duration::from_secs(5));
            }
            other => panic!("expected Retry, got {other:?}"),
        }
    }

    #[test]
    fn second_retry() {
        let policy = policy_with_retries(3);
        let result = ExecutionResult {
            success: false,
            error: Some("timeout".into()),
            duration: Duration::from_secs(1),
            attempt: 2,
        };

        match policy.evaluate(&result) {
            ExecutionOutcome::Retry {
                next_attempt,
                delay,
            } => {
                assert_eq!(next_attempt, 3);
                assert_eq!(delay, Duration::from_secs(5));
            }
            other => panic!("expected Retry, got {other:?}"),
        }
    }

    #[test]
    fn exhausted_goes_to_dead_letter() {
        let policy = policy_with_retries(3);
        let result = ExecutionResult {
            success: false,
            error: Some("persistent failure".into()),
            duration: Duration::from_secs(1),
            attempt: 3, // last attempt
        };

        match policy.evaluate(&result) {
            ExecutionOutcome::DeadLetter { reason, .. } => {
                assert!(reason.contains("exhausted after 3 attempts"));
                assert!(reason.contains("persistent failure"));
            }
            other => panic!("expected DeadLetter, got {other:?}"),
        }
    }

    #[test]
    fn dead_letter_disabled_drops() {
        let policy = ExecutionPolicy {
            retry: RetryPolicy::none(),
            dead_letter: DeadLetterPolicy::disabled(),
            ..Default::default()
        };

        let result = ExecutionResult {
            success: false,
            error: Some("fail".into()),
            duration: Duration::from_secs(1),
            attempt: 1,
        };

        match policy.evaluate(&result) {
            ExecutionOutcome::Dropped { reason } => {
                assert!(reason.contains("exhausted"));
            }
            other => panic!("expected Dropped, got {other:?}"),
        }
    }

    #[test]
    fn timeout_detection() {
        let policy = ExecutionPolicy {
            timeout: TimeoutPolicy::new(Duration::from_secs(60)),
            retry: RetryPolicy::fixed(2, Duration::from_secs(5)),
            ..Default::default()
        };

        // Within timeout → retry
        let result = ExecutionResult {
            success: false,
            error: None,
            duration: Duration::from_secs(30),
            attempt: 1,
        };
        assert!(matches!(
            policy.evaluate(&result),
            ExecutionOutcome::Retry { .. }
        ));

        // Timed out, last attempt → dead letter
        let result = ExecutionResult {
            success: false,
            error: None,
            duration: Duration::from_secs(120),
            attempt: 2,
        };
        match policy.evaluate(&result) {
            ExecutionOutcome::DeadLetter { reason, .. } => {
                assert!(reason.contains("execution timed out"));
            }
            other => panic!("expected DeadLetter, got {other:?}"),
        }
    }

    #[test]
    fn check_timeout_method() {
        let policy = ExecutionPolicy {
            timeout: TimeoutPolicy::new(Duration::from_secs(60)),
            ..Default::default()
        };

        assert!(!policy.check_timeout(Duration::from_secs(30)));
        assert!(policy.check_timeout(Duration::from_secs(60)));
        assert!(policy.check_timeout(Duration::from_secs(90)));
    }

    #[test]
    fn dead_letter_with_hint_and_retention() {
        let policy = ExecutionPolicy {
            retry: RetryPolicy::none(),
            dead_letter: DeadLetterPolicy::default()
                .with_retention("60d")
                .with_hint("Check billing DB"),
            ..Default::default()
        };

        let result = ExecutionResult {
            success: false,
            error: Some("db down".into()),
            duration: Duration::from_secs(1),
            attempt: 1,
        };

        match policy.evaluate(&result) {
            ExecutionOutcome::DeadLetter {
                operator_hint,
                expires_after,
                ..
            } => {
                assert_eq!(operator_hint.as_deref(), Some("Check billing DB"));
                assert_eq!(expires_after, Some(Duration::from_secs(60 * 86400)));
            }
            other => panic!("expected DeadLetter, got {other:?}"),
        }
    }

    #[test]
    fn exponential_retry_increasing_delays() {
        let policy = ExecutionPolicy {
            retry: RetryPolicy::exponential(
                5,
                Duration::from_secs(2),
                Duration::from_secs(30),
                0.0,
            ),
            ..Default::default()
        };

        let mut delays = Vec::new();
        for attempt in 1..5 {
            let result = ExecutionResult {
                success: false,
                error: Some("fail".into()),
                duration: Duration::from_secs(1),
                attempt,
            };
            if let ExecutionOutcome::Retry { delay, .. } = policy.evaluate(&result) {
                delays.push(delay);
            }
        }

        assert_eq!(delays.len(), 4);
        assert_eq!(delays[0], Duration::from_secs(2));
        assert_eq!(delays[1], Duration::from_secs(4));
        assert_eq!(delays[2], Duration::from_secs(8));
        assert_eq!(delays[3], Duration::from_secs(16));
    }
}

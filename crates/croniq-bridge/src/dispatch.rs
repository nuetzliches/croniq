//! Convert DSL job definitions into dispatchable runtime artefacts.
//!
//! This module is the "firing decision" layer: given a compiled `JobConfig`
//! and a trigger moment, it produces the types that `croniq-runner` and
//! `croniq-execution` need to actually execute the job.

use chrono::{DateTime, Utc};
use croniq_config::compile::JobConfig;
use croniq_execution::pipeline::ExecutionPolicy;
use croniq_runner::WorkItem;

use crate::policy::{dead_letter_to_policy, retry_config_to_policy, timeout_to_policy};

// ─── WorkItem ─────────────────────────────────────────────────────────────────

/// Build a `WorkItem` from a `JobConfig` and a fire instant.
///
/// `execution_id` must be unique per firing. The caller is responsible for
/// generating it (e.g. a UUID from `croniq-store`).
///
/// `attempt` is 1 for the first execution, 2 for the first retry, and so on.
pub fn job_to_work_item(
    job: &JobConfig,
    execution_id: impl Into<String>,
    fire_at: DateTime<Utc>,
    attempt: u32,
) -> WorkItem {
    WorkItem {
        execution_id: execution_id.into(),
        job_key: job.key.clone(),
        fire_at,
        attempt,
        require: job.runner.require.clone(),
        prefer: job.runner.prefer.clone(),
        metadata: metadata_to_json(&job.metadata),
        timeout: job
            .timeout
            .clone()
            .unwrap_or_else(|| "5m".to_string()),
    }
}

fn metadata_to_json(meta: &std::collections::HashMap<String, String>) -> serde_json::Value {
    serde_json::Value::Object(
        meta.iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect(),
    )
}

// ─── ExecutionPolicy ──────────────────────────────────────────────────────────

/// Build an `ExecutionPolicy` from the retry / timeout / dead-letter settings
/// of a `JobConfig`.
///
/// This policy governs what happens after an execution completes: whether to
/// retry, when to give up and dead-letter, and how long a single attempt may
/// run.
pub fn job_to_execution_policy(job: &JobConfig) -> ExecutionPolicy {
    ExecutionPolicy {
        retry: retry_config_to_policy(&job.retry),
        timeout: timeout_to_policy(job.timeout.as_deref()),
        dead_letter: dead_letter_to_policy(&job.dead_letter),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use croniq_config::{
        compile::{DeadLetterConfig, RetryConfig, RunnerConfig},
        schedule::CompiledSchedule,
    };
    use croniq_execution::{
        pipeline::ExecutionOutcome,
        retry::RetryStrategy,
    };
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;
    use std::time::Duration;

    fn base_job() -> JobConfig {
        JobConfig {
            key: "billing:invoice".into(),
            namespace: "billing".into(),
            name: "invoice".into(),
            variant: None,
            description: None,
            schedule: CompiledSchedule::Disabled,
            schedule_summary: "disabled".into(),
            timezone: None,
            calendar: None,
            window: None,
            not_before: None,
            not_after: None,
            runner: RunnerConfig::default(),
            retry: RetryConfig::default(),
            timeout: Some("15m".into()),
            dead_letter: DeadLetterConfig::default(),
            metadata: HashMap::new(),
            execution_mode: croniq_config::compile::ExecutionMode::default(),
            catch_up: croniq_config::compile::CatchUpPolicy::default(),
            queue_ttl: None,
            max_queue_depth: None,
        }
    }

    // ─── job_to_work_item ─────────────────────────────────────────────────────

    #[test]
    fn work_item_has_correct_key() {
        let job = base_job();
        let item = job_to_work_item(&job, "exec-1", Utc::now(), 1);
        assert_eq!(item.job_key, "billing:invoice");
        assert_eq!(item.execution_id, "exec-1");
    }

    #[test]
    fn work_item_carries_attempt_number() {
        let job = base_job();
        assert_eq!(job_to_work_item(&job, "exec-1", Utc::now(), 1).attempt, 1);
        assert_eq!(job_to_work_item(&job, "exec-2", Utc::now(), 2).attempt, 2);
        assert_eq!(job_to_work_item(&job, "exec-3", Utc::now(), 5).attempt, 5);
    }

    #[test]
    fn work_item_propagates_timeout() {
        let job = base_job();
        let item = job_to_work_item(&job, "exec-1", Utc::now(), 1);
        assert_eq!(item.timeout, "15m");
    }

    #[test]
    fn work_item_default_timeout_when_none() {
        let mut job = base_job();
        job.timeout = None;
        let item = job_to_work_item(&job, "exec-1", Utc::now(), 1);
        assert_eq!(item.timeout, "5m");
    }

    #[test]
    fn work_item_propagates_runner_require_prefer() {
        let mut job = base_job();
        job.runner.require = vec!["billing".into(), "eu-central".into()];
        job.runner.prefer = vec!["priority".into()];

        let item = job_to_work_item(&job, "exec-1", Utc::now(), 1);
        assert_eq!(item.require, vec!["billing", "eu-central"]);
        assert_eq!(item.prefer, vec!["priority"]);
    }

    #[test]
    fn work_item_fire_at_preserved() {
        let job = base_job();
        let now = Utc::now();
        let item = job_to_work_item(&job, "exec-1", now, 1);
        assert_eq!(item.fire_at, now);
    }

    #[test]
    fn work_item_metadata_serialised_as_json_object() {
        let mut job = base_job();
        job.metadata.insert("month".into(), "2026-03".into());
        job.metadata.insert("env".into(), "prod".into());

        let item = job_to_work_item(&job, "exec-1", Utc::now(), 1);
        assert_eq!(item.metadata["month"], "2026-03");
        assert_eq!(item.metadata["env"], "prod");
    }

    #[test]
    fn work_item_empty_metadata_is_empty_object() {
        let job = base_job();
        let item = job_to_work_item(&job, "exec-1", Utc::now(), 1);
        assert!(item.metadata.as_object().unwrap().is_empty());
    }

    // ─── job_to_execution_policy ──────────────────────────────────────────────

    #[test]
    fn execution_policy_timeout_from_job() {
        let job = base_job(); // timeout = "15m"
        let policy = job_to_execution_policy(&job);
        assert_eq!(policy.timeout.duration, Duration::from_secs(900));
    }

    #[test]
    fn execution_policy_retry_max_attempts() {
        let mut job = base_job();
        job.retry.max_attempts = 5;
        let policy = job_to_execution_policy(&job);
        assert_eq!(policy.retry.max_attempts, 5);
    }

    #[test]
    fn execution_policy_dead_letter_enabled() {
        let job = base_job();
        let policy = job_to_execution_policy(&job);
        assert!(policy.dead_letter.enabled);
    }

    #[test]
    fn execution_policy_dead_letter_disabled() {
        let mut job = base_job();
        job.dead_letter.enabled = false;
        let policy = job_to_execution_policy(&job);
        assert!(!policy.dead_letter.enabled);
    }

    // ─── Full pipeline round-trip ─────────────────────────────────────────────

    #[test]
    fn policy_evaluate_success_after_first_try() {
        use croniq_execution::pipeline::ExecutionResult;

        let job = base_job();
        let policy = job_to_execution_policy(&job);

        let result = ExecutionResult {
            success: true,
            error: None,
            duration: Duration::from_millis(800),
            attempt: 1,
        };

        assert!(matches!(
            policy.evaluate(&result),
            ExecutionOutcome::Success
        ));
    }

    #[test]
    fn policy_evaluate_retry_on_first_failure() {
        use croniq_execution::pipeline::ExecutionResult;

        let job = base_job(); // default retry: exponential, max_attempts=3
        let policy = job_to_execution_policy(&job);

        let result = ExecutionResult {
            success: false,
            error: Some("timeout".into()),
            duration: Duration::from_secs(901), // just over 15m
            attempt: 1,
        };

        assert!(matches!(
            policy.evaluate(&result),
            ExecutionOutcome::Retry { .. }
        ));
    }

    #[test]
    fn policy_evaluate_dead_letter_after_exhaustion() {
        use croniq_execution::pipeline::ExecutionResult;

        let mut job = base_job();
        job.retry.max_attempts = 1;
        let policy = job_to_execution_policy(&job);

        let result = ExecutionResult {
            success: false,
            error: Some("permanent failure".into()),
            duration: Duration::from_millis(100),
            attempt: 1, // = max_attempts → exhausted
        };

        assert!(matches!(
            policy.evaluate(&result),
            ExecutionOutcome::DeadLetter { .. }
        ));
    }

    #[test]
    fn dsl_to_runtime_roundtrip_fixed_strategy() {
        use croniq_execution::retry::RetryDecision;

        let mut job = base_job();
        job.retry = RetryConfig {
            strategy: "fixed".into(),
            max_attempts: 2,
            delay: Some("30s".into()),
            jitter: Some(0.0),
            ..RetryConfig::default()
        };

        let policy = job_to_execution_policy(&job);
        assert!(matches!(
            policy.retry.strategy,
            RetryStrategy::Fixed { delay } if delay == Duration::from_secs(30)
        ));

        // Verify the delay is actually 30s on retry
        if let RetryDecision::RetryAfter(d) = policy.retry.evaluate(1) {
            assert_eq!(d, Duration::from_secs(30));
        } else {
            panic!("expected RetryAfter");
        }
    }
}

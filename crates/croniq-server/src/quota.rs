//! Quota guard: per-job-key trigger-frequency rate limiting.
//!
//! Only a per-minute trigger rate is enforced. It is backed by a sliding
//! window of recent trigger timestamps, so it self-heals: once a job stops
//! firing for a minute the window empties and it is allowed again.
//!
//! An earlier version also enforced a `max_parallel` cap backed by a
//! monotonic `active` counter incremented on every fire and decremented by a
//! `release()` call. But `release()` was never wired up in production —
//! completions are processed in a separate task (`CompletionProcessor`) with
//! no handle to this guard — so `active` only ever grew and, after
//! `max_parallel` fires, permanently wedged the job `overdue` (the quota-guard
//! leak found alongside issue #263). That cap was also undocumented and not
//! configurable from the DSL/API, and it duplicated the per-job
//! `max_queue_depth` overflow guard in the scheduler, which already bounds
//! in-flight work from live queue state and self-heals as runners drain. It
//! was removed rather than papered over.

use std::collections::HashMap;
use std::time::Instant;

/// Per-job-key quota configuration.
#[derive(Debug, Clone)]
pub struct JobQuota {
    pub max_per_minute: u32,
}

impl Default for JobQuota {
    fn default() -> Self {
        Self { max_per_minute: 60 }
    }
}

/// In-memory quota guard tracking recent trigger counts per job key.
pub struct QuotaGuard {
    /// Sliding window of trigger timestamps per job key.
    recent_triggers: HashMap<String, Vec<Instant>>,
    /// Default quota for jobs without explicit configuration.
    default_quota: JobQuota,
    /// Per-job overrides.
    quotas: HashMap<String, JobQuota>,
}

impl Default for QuotaGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl QuotaGuard {
    pub fn new() -> Self {
        Self {
            recent_triggers: HashMap::new(),
            default_quota: JobQuota::default(),
            quotas: HashMap::new(),
        }
    }

    pub fn set_quota(&mut self, job_key: &str, quota: JobQuota) {
        self.quotas.insert(job_key.to_string(), quota);
    }

    /// Check if a job is allowed to fire. Returns `true` if within the
    /// per-minute trigger rate, recording the trigger when it is.
    pub fn allow(&mut self, job_key: &str) -> bool {
        let quota = self.quotas.get(job_key).unwrap_or(&self.default_quota);
        let now = Instant::now();

        // Check per-minute trigger rate over a sliding 60s window.
        let window = std::time::Duration::from_secs(60);
        let triggers = self.recent_triggers.entry(job_key.to_string()).or_default();
        triggers.retain(|t| now.duration_since(*t) < window);
        if triggers.len() as u32 >= quota.max_per_minute {
            return false;
        }

        // Record this trigger
        triggers.push(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_within_rate_limit() {
        let mut guard = QuotaGuard::new();
        guard.set_quota("test:job", JobQuota { max_per_minute: 2 });
        assert!(guard.allow("test:job"));
        assert!(guard.allow("test:job"));
        assert!(!guard.allow("test:job")); // 3rd within the window blocked
    }

    #[test]
    fn rate_limit_is_per_job_key() {
        let mut guard = QuotaGuard::new();
        guard.set_quota("a:job", JobQuota { max_per_minute: 1 });
        guard.set_quota("b:job", JobQuota { max_per_minute: 1 });
        assert!(guard.allow("a:job"));
        assert!(!guard.allow("a:job"));
        // A different job key has its own independent window.
        assert!(guard.allow("b:job"));
    }

    #[test]
    fn unconfigured_job_uses_default_quota() {
        let mut guard = QuotaGuard::new();
        // Default is 60/min — 60 fires allowed within the window, the 61st not.
        for _ in 0..60 {
            assert!(guard.allow("unconfigured:job"));
        }
        assert!(!guard.allow("unconfigured:job"));
    }
}

//! Quota guard: per-job-key rate limiting for execution concurrency and trigger frequency.

use std::collections::HashMap;
use std::time::Instant;

/// Per-job-key quota configuration.
#[derive(Debug, Clone)]
pub struct JobQuota {
    pub max_parallel: u32,
    pub max_per_minute: u32,
}

impl Default for JobQuota {
    fn default() -> Self {
        Self {
            max_parallel: 10,
            max_per_minute: 60,
        }
    }
}

/// In-memory quota guard tracking active executions and recent trigger counts.
pub struct QuotaGuard {
    /// Number of currently active (queued + claimed) executions per job key.
    active: HashMap<String, u32>,
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
            active: HashMap::new(),
            recent_triggers: HashMap::new(),
            default_quota: JobQuota::default(),
            quotas: HashMap::new(),
        }
    }

    pub fn set_quota(&mut self, job_key: &str, quota: JobQuota) {
        self.quotas.insert(job_key.to_string(), quota);
    }

    /// Check if a job is allowed to fire. Returns `true` if within quota.
    pub fn allow(&mut self, job_key: &str) -> bool {
        let quota = self.quotas.get(job_key).unwrap_or(&self.default_quota);
        let now = Instant::now();

        // Check parallel execution limit
        let active = self.active.get(job_key).copied().unwrap_or(0);
        if active >= quota.max_parallel {
            return false;
        }

        // Check per-minute trigger rate
        let window = std::time::Duration::from_secs(60);
        let triggers = self.recent_triggers.entry(job_key.to_string()).or_default();
        triggers.retain(|t| now.duration_since(*t) < window);
        if triggers.len() as u32 >= quota.max_per_minute {
            return false;
        }

        // Record this trigger
        triggers.push(now);
        *self.active.entry(job_key.to_string()).or_insert(0) += 1;
        true
    }

    /// Mark an execution as completed (decrement active count).
    pub fn release(&mut self, job_key: &str) {
        if let Some(count) = self.active.get_mut(job_key) {
            *count = count.saturating_sub(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_within_quota() {
        let mut guard = QuotaGuard::new();
        guard.set_quota(
            "test:job",
            JobQuota {
                max_parallel: 2,
                max_per_minute: 10,
            },
        );
        assert!(guard.allow("test:job"));
        assert!(guard.allow("test:job"));
        assert!(!guard.allow("test:job")); // 3rd parallel blocked
    }

    #[test]
    fn release_frees_slot() {
        let mut guard = QuotaGuard::new();
        guard.set_quota(
            "test:job",
            JobQuota {
                max_parallel: 1,
                max_per_minute: 100,
            },
        );
        assert!(guard.allow("test:job"));
        assert!(!guard.allow("test:job"));
        guard.release("test:job");
        assert!(guard.allow("test:job"));
    }
}

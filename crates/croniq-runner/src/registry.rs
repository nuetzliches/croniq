//! Runner registry: tracks connected runners and their liveness.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::types::{Runner, RunnerStatus};

/// Default dead-threshold used by `register_or_update`. Matches the default
/// `lease_ttl_secs` on `AppState` (see `api.rs`). Production code that knows
/// the configured threshold should call `register_or_update_with_ttl`.
const DEFAULT_DEAD_THRESHOLD_SECS: u64 = 120;

/// Result of a successful `register_or_update` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterOutcome {
    /// First-ever registration for this runner_id.
    New,
    /// Existing entry was refreshed (heartbeat/inflight/capabilities updated).
    Updated,
    /// The previous instance was past the dead-threshold and has been replaced
    /// by this new instance. The caller should treat any executions still
    /// claimed by this `runner_id` in the persistent store as abandoned and
    /// requeue them — otherwise they remain orphaned (issue #190).
    TookOver { previous_instance_id: String },
}

/// In-memory registry of all runners that have ever polled.
///
/// All access is synchronous; callers should wrap this in `Arc<RwLock<_>>`
/// for shared async use.
#[derive(Debug, Default)]
pub struct RunnerRegistry {
    runners: HashMap<String, Runner>,
}

impl RunnerRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a runner or refresh its heartbeat. Uses a default 120 s
    /// dead-threshold for stale-instance takeover (see issue #190).
    /// Production paths that know the configured `lease_ttl_secs` should
    /// call [`Self::register_or_update_with_ttl`] instead.
    pub fn register_or_update(
        &mut self,
        runner_id: impl Into<String>,
        capabilities: Vec<String>,
        max_inflight: u32,
        inflight: Vec<String>,
        instance_id: Option<String>,
        tags: Vec<String>,
    ) -> Result<RegisterOutcome, String> {
        self.register_or_update_with_ttl(
            runner_id,
            capabilities,
            max_inflight,
            inflight,
            instance_id,
            tags,
            DEFAULT_DEAD_THRESHOLD_SECS,
        )
    }

    /// Like [`Self::register_or_update`] but lets the caller specify the
    /// dead-threshold used for stale-instance takeover. When a new
    /// `instance_id` arrives for an existing `runner_id` and the existing
    /// entry's `last_poll_at` is older than `dead_threshold_secs`, the old
    /// session is evicted inline and the new one accepted — without this,
    /// the new instance is locked out until the watchdog sweep runs
    /// (up to ~10 minutes; issue #190).
    #[allow(clippy::too_many_arguments)]
    pub fn register_or_update_with_ttl(
        &mut self,
        runner_id: impl Into<String>,
        capabilities: Vec<String>,
        max_inflight: u32,
        inflight: Vec<String>,
        instance_id: Option<String>,
        tags: Vec<String>,
        dead_threshold_secs: u64,
    ) -> Result<RegisterOutcome, String> {
        let id = runner_id.into();
        let now = Utc::now();
        let mut took_over: Option<String> = None;

        // Instance guard: a new instance_id arriving for an existing runner_id
        // is either a real conflict (two processes racing for the same id) or
        // a takeover (the old process died and a replacement is reconnecting).
        // We disambiguate by checking liveness.
        if let Some(ref new_iid) = instance_id
            && let Some(existing) = self.runners.get(&id)
            && let Some(ref old_iid) = existing.instance_id
            && old_iid != new_iid
        {
            if existing.status_at_with_ttl(now, dead_threshold_secs) == RunnerStatus::Dead {
                took_over = Some(old_iid.clone());
                self.runners.remove(&id);
            } else {
                return Err(old_iid.clone());
            }
        }

        let outcome = match &took_over {
            Some(prev) => RegisterOutcome::TookOver {
                previous_instance_id: prev.clone(),
            },
            None if self.runners.contains_key(&id) => RegisterOutcome::Updated,
            None => RegisterOutcome::New,
        };

        let runner = self.runners.entry(id.clone()).or_insert_with(|| Runner {
            runner_id: id,
            capabilities: capabilities.clone(),
            max_inflight,
            last_poll_at: now,
            inflight: inflight.clone(),
            instance_id: instance_id.clone(),
            tags: tags.clone(),
        });

        // Always update heartbeat + inflight; capabilities + tags may change too.
        runner.capabilities = capabilities;
        runner.max_inflight = max_inflight;
        runner.last_poll_at = now;
        runner.inflight = inflight;
        runner.tags = tags;
        if instance_id.is_some() {
            runner.instance_id = instance_id;
        }

        Ok(outcome)
    }

    /// Remove a runner from the registry.
    pub fn remove(&mut self, runner_id: &str) -> Option<Runner> {
        self.runners.remove(runner_id)
    }

    pub fn get(&self, runner_id: &str) -> Option<&Runner> {
        self.runners.get(runner_id)
    }

    pub fn get_mut(&mut self, runner_id: &str) -> Option<&mut Runner> {
        self.runners.get_mut(runner_id)
    }

    /// All registered runners regardless of status.
    pub fn all(&self) -> impl Iterator<Item = &Runner> {
        self.runners.values()
    }

    /// Runners whose status matches `filter` at the given instant.
    pub fn by_status(&self, status: RunnerStatus, now: DateTime<Utc>) -> Vec<&Runner> {
        self.runners
            .values()
            .filter(|r| r.status_at(now) == status)
            .collect()
    }

    /// Like `by_status` but with a custom dead threshold in seconds.
    pub fn by_status_with_ttl(
        &self,
        status: RunnerStatus,
        now: DateTime<Utc>,
        dead_threshold_secs: u64,
    ) -> Vec<&Runner> {
        self.runners
            .values()
            .filter(|r| r.status_at_with_ttl(now, dead_threshold_secs) == status)
            .collect()
    }

    /// Runner IDs that are considered dead. Their inflight work should be
    /// reassigned by the scheduler.
    pub fn dead_ids(&self, now: DateTime<Utc>) -> Vec<String> {
        self.by_status(RunnerStatus::Dead, now)
            .into_iter()
            .map(|r| r.runner_id.clone())
            .collect()
    }

    /// Add `execution_id` to the runner's inflight list.
    ///
    /// Returns `false` if the runner is unknown.
    pub fn claim(&mut self, runner_id: &str, execution_id: impl Into<String>) -> bool {
        match self.runners.get_mut(runner_id) {
            Some(r) => {
                r.inflight.push(execution_id.into());
                true
            }
            None => false,
        }
    }

    /// Remove `execution_id` from the runner's inflight list.
    ///
    /// Returns `false` if the runner is unknown or the ID wasn't in the list.
    pub fn release(&mut self, runner_id: &str, execution_id: &str) -> bool {
        match self.runners.get_mut(runner_id) {
            Some(r) => {
                let before = r.inflight.len();
                r.inflight.retain(|id| id != execution_id);
                r.inflight.len() < before
            }
            None => false,
        }
    }

    pub fn len(&self) -> usize {
        self.runners.len()
    }

    pub fn is_empty(&self) -> bool {
        self.runners.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use pretty_assertions::assert_eq;

    fn now() -> DateTime<Utc> {
        Utc::now()
    }

    #[test]
    fn register_new_runner() {
        let mut reg = RunnerRegistry::new();
        let outcome = reg.register_or_update("r1", vec!["billing".into()], 3, vec![], None, vec![]);
        assert_eq!(outcome.unwrap(), RegisterOutcome::New);
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn re_register_updates_heartbeat() {
        let mut reg = RunnerRegistry::new();
        let _ = reg.register_or_update("r1", vec!["billing".into()], 3, vec![], None, vec![]);
        let first_poll = reg.get("r1").unwrap().last_poll_at;

        // Simulate time passing by sleeping 1ms (not ideal but cheap)
        std::thread::sleep(std::time::Duration::from_millis(5));

        let outcome = reg.register_or_update("r1", vec!["billing".into()], 3, vec![], None, vec![]);
        assert_eq!(outcome.unwrap(), RegisterOutcome::Updated);
        let second_poll = reg.get("r1").unwrap().last_poll_at;
        assert!(second_poll > first_poll);
    }

    #[test]
    fn capabilities_updated_on_re_register() {
        let mut reg = RunnerRegistry::new();
        let _ = reg.register_or_update("r1", vec!["billing".into()], 3, vec![], None, vec![]);
        let _ = reg.register_or_update(
            "r1",
            vec!["billing".into(), "eu-central".into()],
            3,
            vec![],
            None,
            vec![],
        );

        let r = reg.get("r1").unwrap();
        assert_eq!(r.capabilities.len(), 2);
    }

    #[test]
    fn inflight_reflected_in_registry() {
        let mut reg = RunnerRegistry::new();
        let _ = reg.register_or_update(
            "r1",
            vec![],
            3,
            vec!["exec-1".into(), "exec-2".into()],
            None,
            vec![],
        );

        let r = reg.get("r1").unwrap();
        assert_eq!(r.inflight, vec!["exec-1", "exec-2"]);
    }

    #[test]
    fn claim_and_release() {
        let mut reg = RunnerRegistry::new();
        let _ = reg.register_or_update("r1", vec![], 3, vec![], None, vec![]);

        assert!(reg.claim("r1", "exec-42"));
        assert_eq!(reg.get("r1").unwrap().inflight, vec!["exec-42"]);

        assert!(reg.release("r1", "exec-42"));
        assert!(reg.get("r1").unwrap().inflight.is_empty());
    }

    #[test]
    fn claim_unknown_runner_returns_false() {
        let mut reg = RunnerRegistry::new();
        assert!(!reg.claim("unknown", "exec-1"));
    }

    #[test]
    fn release_unknown_execution_returns_false() {
        let mut reg = RunnerRegistry::new();
        let _ = reg.register_or_update("r1", vec![], 3, vec![], None, vec![]);
        assert!(!reg.release("r1", "exec-does-not-exist"));
    }

    #[test]
    fn remove_runner() {
        let mut reg = RunnerRegistry::new();
        let _ = reg.register_or_update("r1", vec![], 3, vec![], None, vec![]);
        let removed = reg.remove("r1");
        assert!(removed.is_some());
        assert!(reg.is_empty());
    }

    #[test]
    fn by_status_filters_correctly() {
        let mut reg = RunnerRegistry::new();
        let _ = reg.register_or_update("online", vec![], 1, vec![], None, vec![]);

        // Manually set a stale/dead runner by inserting with old timestamp
        reg.runners.insert(
            "dead".into(),
            Runner {
                runner_id: "dead".into(),
                capabilities: vec![],
                max_inflight: 1,
                last_poll_at: now() - Duration::seconds(200),
                inflight: vec![],
                instance_id: None,
                tags: vec![],
            },
        );

        let online = reg.by_status(RunnerStatus::Online, now());
        let dead = reg.by_status(RunnerStatus::Dead, now());

        assert_eq!(online.len(), 1);
        assert_eq!(online[0].runner_id, "online");
        assert_eq!(dead.len(), 1);
        assert_eq!(dead[0].runner_id, "dead");
    }

    #[test]
    fn fresh_instance_conflict_is_rejected() {
        // Two processes racing for the same runner_id, both alive — the
        // second one must be told to back off.
        let mut reg = RunnerRegistry::new();
        let _ = reg
            .register_or_update("r1", vec![], 3, vec![], Some("instance-A".into()), vec![])
            .unwrap();

        let err = reg
            .register_or_update("r1", vec![], 3, vec![], Some("instance-B".into()), vec![])
            .unwrap_err();

        assert_eq!(err, "instance-A");
        assert_eq!(
            reg.get("r1").unwrap().instance_id.as_deref(),
            Some("instance-A")
        );
    }

    #[test]
    fn stale_instance_is_evicted_on_takeover() {
        // Issue #190: a new instance polling for an existing runner_id must
        // not be locked out for the full watchdog sweep when the old session
        // is past dead-threshold. The conflict path should evict the dead
        // entry inline and accept the new instance.
        let mut reg = RunnerRegistry::new();
        reg.runners.insert(
            "r1".into(),
            Runner {
                runner_id: "r1".into(),
                capabilities: vec!["backup".into()],
                max_inflight: 1,
                last_poll_at: now() - Duration::seconds(600), // well past 120 s dead-threshold
                inflight: vec!["exec-orphan".into()],
                instance_id: Some("instance-old".into()),
                tags: vec![],
            },
        );

        let outcome = reg
            .register_or_update_with_ttl(
                "r1",
                vec!["backup".into()],
                1,
                vec![],
                Some("instance-new".into()),
                vec![],
                120,
            )
            .unwrap();

        assert_eq!(
            outcome,
            RegisterOutcome::TookOver {
                previous_instance_id: "instance-old".into()
            }
        );

        let r = reg.get("r1").unwrap();
        assert_eq!(r.instance_id.as_deref(), Some("instance-new"));
        // The old inflight list is gone — the caller is responsible for
        // requeuing those executions from the store.
        assert!(r.inflight.is_empty());
    }

    #[test]
    fn dead_ids_returns_dead_runner_ids() {
        let mut reg = RunnerRegistry::new();
        reg.runners.insert(
            "zombie".into(),
            Runner {
                runner_id: "zombie".into(),
                capabilities: vec![],
                max_inflight: 1,
                last_poll_at: now() - Duration::seconds(300),
                inflight: vec!["exec-1".into()],
                instance_id: None,
                tags: vec![],
            },
        );

        let ids = reg.dead_ids(now());
        assert_eq!(ids, vec!["zombie"]);
    }
}

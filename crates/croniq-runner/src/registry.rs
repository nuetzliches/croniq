//! Runner registry: tracks connected runners and their liveness.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::types::{Runner, RunnerStatus};

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

    /// Register a new runner or update an existing one's heartbeat and state.
    ///
    /// Returns `true` if this was a new registration, `false` for an update.
    /// Register or update a runner. Returns:
    /// - `Ok(true)` if this is a new registration
    /// - `Ok(false)` if this is an update to an existing runner
    /// - `Err(conflict_instance_id)` if another instance already owns this runner_id
    pub fn register_or_update(
        &mut self,
        runner_id: impl Into<String>,
        capabilities: Vec<String>,
        max_inflight: u32,
        inflight: Vec<String>,
        instance_id: Option<String>,
    ) -> Result<bool, String> {
        let id = runner_id.into();
        let now = Utc::now();

        // Instance guard: check for conflicting instance_id
        if let Some(ref new_iid) = instance_id
            && let Some(existing) = self.runners.get(&id)
                && let Some(ref old_iid) = existing.instance_id
                    && old_iid != new_iid {
                        // Different instance trying to use the same runner_id
                        return Err(old_iid.clone());
                    }

        let is_new = !self.runners.contains_key(&id);

        let runner = self.runners.entry(id.clone()).or_insert_with(|| Runner {
            runner_id: id,
            capabilities: capabilities.clone(),
            max_inflight,
            last_poll_at: now,
            inflight: inflight.clone(),
            instance_id: instance_id.clone(),
        });

        // Always update heartbeat + inflight; capabilities may change too.
        runner.capabilities = capabilities;
        runner.max_inflight = max_inflight;
        runner.last_poll_at = now;
        runner.inflight = inflight;
        if instance_id.is_some() {
            runner.instance_id = instance_id;
        }

        Ok(is_new)
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
    pub fn by_status_with_ttl(&self, status: RunnerStatus, now: DateTime<Utc>, dead_threshold_secs: u64) -> Vec<&Runner> {
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
        let is_new = reg.register_or_update("r1", vec!["billing".into()], 3, vec![], None);
        assert!(is_new.unwrap());
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn re_register_updates_heartbeat() {
        let mut reg = RunnerRegistry::new();
        reg.register_or_update("r1", vec!["billing".into()], 3, vec![], None);
        let first_poll = reg.get("r1").unwrap().last_poll_at;

        // Simulate time passing by sleeping 1ms (not ideal but cheap)
        std::thread::sleep(std::time::Duration::from_millis(5));

        let is_new = reg.register_or_update("r1", vec!["billing".into()], 3, vec![], None);
        assert!(!is_new.unwrap());
        let second_poll = reg.get("r1").unwrap().last_poll_at;
        assert!(second_poll > first_poll);
    }

    #[test]
    fn capabilities_updated_on_re_register() {
        let mut reg = RunnerRegistry::new();
        reg.register_or_update("r1", vec!["billing".into()], 3, vec![], None);
        reg.register_or_update("r1", vec!["billing".into(), "eu-central".into()], 3, vec![], None);

        let r = reg.get("r1").unwrap();
        assert_eq!(r.capabilities.len(), 2);
    }

    #[test]
    fn inflight_reflected_in_registry() {
        let mut reg = RunnerRegistry::new();
        reg.register_or_update("r1", vec![], 3, vec!["exec-1".into(), "exec-2".into()], None);

        let r = reg.get("r1").unwrap();
        assert_eq!(r.inflight, vec!["exec-1", "exec-2"]);
    }

    #[test]
    fn claim_and_release() {
        let mut reg = RunnerRegistry::new();
        reg.register_or_update("r1", vec![], 3, vec![], None);

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
        reg.register_or_update("r1", vec![], 3, vec![], None);
        assert!(!reg.release("r1", "exec-does-not-exist"));
    }

    #[test]
    fn remove_runner() {
        let mut reg = RunnerRegistry::new();
        reg.register_or_update("r1", vec![], 3, vec![], None);
        let removed = reg.remove("r1");
        assert!(removed.is_some());
        assert!(reg.is_empty());
    }

    #[test]
    fn by_status_filters_correctly() {
        let mut reg = RunnerRegistry::new();
        reg.register_or_update("online", vec![], 1, vec![], None);

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
            },
        );

        let ids = reg.dead_ids(now());
        assert_eq!(ids, vec!["zombie"]);
    }
}

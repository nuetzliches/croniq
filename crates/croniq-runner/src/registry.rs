//! Runner registry: tracks connected runners and their liveness.

use std::collections::{HashMap, VecDeque};

use chrono::{DateTime, Utc};

use crate::types::{Runner, RunnerStatus};

/// Default dead-threshold used by the deprecated `by_status` / `dead_ids`
/// status helpers. Matches the default `lease_ttl_secs` on `AppState` (see
/// `api.rs`). Production code passes the configured threshold via
/// `by_status_with_ttl` instead. Instance takeover in `register_or_update`
/// does not use a dead-threshold (issues #190, #374).
const DEFAULT_DEAD_THRESHOLD_SECS: u64 = 120;

/// Sliding window for identity-flapping detection (issue #374 follow-up):
/// this many takeovers of the same `runner_id` within `FLAP_WINDOW_SECS`
/// means two (or more) live processes are almost certainly sharing the id —
/// each restart-loop iteration under a container restart policy produces a
/// fresh `instance_id` that legitimately takes the identity over, so the
/// per-takeover signal alone never reveals the ping-pong.
const FLAP_WINDOW_SECS: i64 = 600;
const FLAP_THRESHOLD: usize = 3;

/// Result of a successful `register_or_update` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterOutcome {
    /// First-ever registration for this runner_id.
    New,
    /// Existing entry was refreshed (heartbeat/inflight/capabilities updated).
    Updated,
    /// A new `instance_id` arrived for this `runner_id` and replaced the
    /// previous instance (issues #190, #374). The caller should treat any
    /// executions still claimed by this `runner_id` in the persistent store
    /// as abandoned and requeue them — otherwise they remain orphaned.
    TookOver { previous_instance_id: String },
}

/// In-memory registry of all runners that have ever polled.
///
/// All access is synchronous; callers should wrap this in `Arc<RwLock<_>>`
/// for shared async use.
#[derive(Debug, Default)]
pub struct RunnerRegistry {
    runners: HashMap<String, Runner>,
    /// Recent takeover timestamps per `runner_id`, pruned to
    /// `FLAP_WINDOW_SECS` on every insert. Keyed by runner_id (bounded by
    /// fleet size), never by instance_id (unbounded under a restart loop).
    takeover_history: HashMap<String, VecDeque<DateTime<Utc>>>,
    /// Last time a flapping signal was raised per `runner_id` — throttles
    /// `record_takeover` to one signal per window while the ping-pong lasts.
    last_flap_signal: HashMap<String, DateTime<Utc>>,
}

impl RunnerRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a runner or refresh its heartbeat. A new `instance_id`
    /// arriving for an existing `runner_id` evicts the old session inline
    /// and takes the identity over regardless of the old entry's liveness
    /// (issues #190, #374) — the caller must requeue the old session's
    /// claims on [`RegisterOutcome::TookOver`]. Only the most recently
    /// deposed instance is rejected with `Err` (fencing; see the instance
    /// guard below).
    pub fn register_or_update(
        &mut self,
        runner_id: impl Into<String>,
        capabilities: Vec<String>,
        max_inflight: u32,
        inflight: Vec<String>,
        instance_id: Option<String>,
        tags: Vec<String>,
    ) -> Result<RegisterOutcome, String> {
        let id = runner_id.into();
        let now = Utc::now();
        let mut took_over: Option<String> = None;

        // Instance guard (issues #190, #374): a new instance_id arriving for
        // an existing runner_id takes the identity over immediately — a
        // fresh instance_id from the same persisted runner_id is the restart
        // signal, and waiting for the old entry to go Dead strands its
        // claimed executions for up to the dead threshold (or forever, when
        // the new session keeps the id alive). The caller requeues the old
        // session's claims on `TookOver`.
        //
        // Duplicate-deployment protection (the reason #190 kept a conflict
        // path): the deposed instance_id is fenced — its further polls get
        // a conflict, so two live processes sharing a runner_id converge to
        // one winner (last writer) while the loser bails out per #134,
        // instead of endlessly taking the id over from each other.
        if let Some(ref new_iid) = instance_id
            && let Some(existing) = self.runners.get(&id)
            && let Some(ref old_iid) = existing.instance_id
            && old_iid != new_iid
        {
            if existing.deposed_instance_id.as_ref() == Some(new_iid) {
                return Err(old_iid.clone());
            }
            took_over = Some(old_iid.clone());
            self.runners.remove(&id);
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
            deposed_instance_id: None,
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
        // Fence the deposed instance so its next poll conflicts instead of
        // re-taking the id (see instance guard above). Never cleared on
        // plain updates: instance IDs are fresh UUIDs per process start, so
        // a stale fence can never lock out a legitimate new session.
        if took_over.is_some() {
            runner.deposed_instance_id = took_over.clone();
        }

        Ok(outcome)
    }

    /// Record a takeover of `runner_id` at `now` and report whether this
    /// crossed the identity-flapping threshold (≥ `FLAP_THRESHOLD` takeovers
    /// within `FLAP_WINDOW_SECS`). Returns `true` at most once per window
    /// per runner_id so the caller can warn/audit without spamming — while
    /// the flapping persists, the signal re-fires once the window elapses.
    pub fn record_takeover(&mut self, runner_id: &str, now: DateTime<Utc>) -> bool {
        let window_start = now - chrono::Duration::seconds(FLAP_WINDOW_SECS);
        let history = self
            .takeover_history
            .entry(runner_id.to_string())
            .or_default();
        history.push_back(now);
        while history.front().is_some_and(|t| *t < window_start) {
            history.pop_front();
        }

        if history.len() < FLAP_THRESHOLD {
            return false;
        }
        let throttled = self
            .last_flap_signal
            .get(runner_id)
            .is_some_and(|last| *last >= window_start);
        if throttled {
            return false;
        }
        self.last_flap_signal.insert(runner_id.to_string(), now);
        true
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

    /// Runners whose status matches `filter` at the given instant, using the
    /// default 120 s dead-threshold.
    #[deprecated(
        note = "hardcodes a 120 s dead-threshold; use `by_status_with_ttl` with the configured `lease_ttl_secs` so status matches the watchdog's assessment"
    )]
    pub fn by_status(&self, status: RunnerStatus, now: DateTime<Utc>) -> Vec<&Runner> {
        self.by_status_with_ttl(status, now, DEFAULT_DEAD_THRESHOLD_SECS)
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

    /// Runner IDs that are considered dead (default 120 s dead-threshold).
    /// Their inflight work should be reassigned by the scheduler.
    #[deprecated(
        note = "hardcodes a 120 s dead-threshold; use `by_status_with_ttl(RunnerStatus::Dead, …)` with the configured `lease_ttl_secs`"
    )]
    pub fn dead_ids(&self, now: DateTime<Utc>) -> Vec<String> {
        self.by_status_with_ttl(RunnerStatus::Dead, now, DEFAULT_DEAD_THRESHOLD_SECS)
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
                deposed_instance_id: None,
                tags: vec![],
            },
        );

        let online = reg.by_status_with_ttl(RunnerStatus::Online, now(), 120);
        let dead = reg.by_status_with_ttl(RunnerStatus::Dead, now(), 120);

        assert_eq!(online.len(), 1);
        assert_eq!(online[0].runner_id, "online");
        assert_eq!(dead.len(), 1);
        assert_eq!(dead[0].runner_id, "dead");
    }

    #[test]
    fn by_status_with_ttl_uses_configured_threshold() {
        // lease_ttl 300: a runner last seen 200 s ago is Stale (150 ≤ 200 < 300),
        // while the 120 s default would classify it as Dead.
        let mut reg = RunnerRegistry::new();
        reg.runners.insert(
            "lagging".into(),
            Runner {
                runner_id: "lagging".into(),
                capabilities: vec![],
                max_inflight: 1,
                last_poll_at: now() - Duration::seconds(200),
                inflight: vec![],
                instance_id: None,
                deposed_instance_id: None,
                tags: vec![],
            },
        );

        assert_eq!(
            reg.by_status_with_ttl(RunnerStatus::Stale, now(), 300)
                .len(),
            1
        );
        assert!(
            reg.by_status_with_ttl(RunnerStatus::Dead, now(), 300)
                .is_empty()
        );
        assert_eq!(
            reg.by_status_with_ttl(RunnerStatus::Dead, now(), 120).len(),
            1
        );
    }

    #[test]
    fn fresh_instance_takes_over_live_entry() {
        // Issue #374: a fresh instance_id under the same runner_id is the
        // restart signal — it must take over immediately (and the caller
        // requeues the old session's claims), even while the old entry is
        // still within the dead threshold.
        let mut reg = RunnerRegistry::new();
        let _ = reg
            .register_or_update("r1", vec![], 3, vec![], Some("instance-A".into()), vec![])
            .unwrap();

        let outcome = reg
            .register_or_update("r1", vec![], 3, vec![], Some("instance-B".into()), vec![])
            .unwrap();

        assert_eq!(
            outcome,
            RegisterOutcome::TookOver {
                previous_instance_id: "instance-A".into()
            }
        );
        let r = reg.get("r1").unwrap();
        assert_eq!(r.instance_id.as_deref(), Some("instance-B"));
        // The loser is fenced out.
        assert_eq!(r.deposed_instance_id.as_deref(), Some("instance-A"));
    }

    #[test]
    fn deposed_instance_is_fenced_with_conflict() {
        // Duplicate deployment: after B deposes A, A's next poll must NOT
        // re-take the id (that would thrash) — it gets a conflict and its
        // SDK bails out after the conflict streak (#134).
        let mut reg = RunnerRegistry::new();
        let _ = reg
            .register_or_update("r1", vec![], 3, vec![], Some("instance-A".into()), vec![])
            .unwrap();
        let _ = reg
            .register_or_update("r1", vec![], 3, vec![], Some("instance-B".into()), vec![])
            .unwrap();

        let err = reg
            .register_or_update("r1", vec![], 3, vec![], Some("instance-A".into()), vec![])
            .unwrap_err();

        assert_eq!(err, "instance-B");
        assert_eq!(
            reg.get("r1").unwrap().instance_id.as_deref(),
            Some("instance-B")
        );
    }

    #[test]
    fn winner_re_poll_preserves_fence() {
        let mut reg = RunnerRegistry::new();
        let _ = reg
            .register_or_update("r1", vec![], 3, vec![], Some("instance-A".into()), vec![])
            .unwrap();
        let _ = reg
            .register_or_update("r1", vec![], 3, vec![], Some("instance-B".into()), vec![])
            .unwrap();

        // Winner keeps polling: plain Updated, fence untouched.
        let outcome = reg
            .register_or_update("r1", vec![], 3, vec![], Some("instance-B".into()), vec![])
            .unwrap();
        assert_eq!(outcome, RegisterOutcome::Updated);
        assert_eq!(
            reg.get("r1").unwrap().deposed_instance_id.as_deref(),
            Some("instance-A")
        );
    }

    #[test]
    fn third_instance_overwrites_fence() {
        // Last-writer-wins: a genuinely new process (fresh UUID) always
        // gets in; the fence only ever holds the most recently deposed id.
        let mut reg = RunnerRegistry::new();
        let _ = reg
            .register_or_update("r1", vec![], 3, vec![], Some("instance-A".into()), vec![])
            .unwrap();
        let _ = reg
            .register_or_update("r1", vec![], 3, vec![], Some("instance-B".into()), vec![])
            .unwrap();

        let outcome = reg
            .register_or_update("r1", vec![], 3, vec![], Some("instance-C".into()), vec![])
            .unwrap();
        assert_eq!(
            outcome,
            RegisterOutcome::TookOver {
                previous_instance_id: "instance-B".into()
            }
        );
        let r = reg.get("r1").unwrap();
        assert_eq!(r.deposed_instance_id.as_deref(), Some("instance-B"));
        // A is no longer fenced — as a fresh process it could register again.
        let outcome = reg
            .register_or_update("r1", vec![], 3, vec![], Some("instance-A".into()), vec![])
            .unwrap();
        assert!(matches!(outcome, RegisterOutcome::TookOver { .. }));
    }

    #[test]
    fn stale_instance_is_evicted_on_takeover() {
        // Issues #190, #374: a new instance polling for an existing
        // runner_id evicts the old entry inline and takes the identity over
        // regardless of the old entry's liveness.
        let mut reg = RunnerRegistry::new();
        reg.runners.insert(
            "r1".into(),
            Runner {
                runner_id: "r1".into(),
                capabilities: vec!["backup".into()],
                max_inflight: 1,
                last_poll_at: now() - Duration::seconds(600),
                inflight: vec!["exec-orphan".into()],
                instance_id: Some("instance-old".into()),
                deposed_instance_id: None,
                tags: vec![],
            },
        );

        let outcome = reg
            .register_or_update(
                "r1",
                vec!["backup".into()],
                1,
                vec![],
                Some("instance-new".into()),
                vec![],
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
                deposed_instance_id: None,
                tags: vec![],
            },
        );

        #[allow(deprecated)]
        let ids = reg.dead_ids(now());
        assert_eq!(ids, vec!["zombie"]);
    }

    // ─── Identity-flapping detection (issue #374 follow-up) ─────────────────

    #[test]
    fn flapping_signals_once_at_third_takeover_in_window() {
        let mut reg = RunnerRegistry::new();
        let t0 = now();
        assert!(!reg.record_takeover("r1", t0));
        assert!(!reg.record_takeover("r1", t0 + Duration::seconds(60)));
        // Third takeover within 10 min crosses the threshold.
        assert!(reg.record_takeover("r1", t0 + Duration::seconds(120)));
        // Further takeovers in the same window are throttled.
        assert!(!reg.record_takeover("r1", t0 + Duration::seconds(180)));
        assert!(!reg.record_takeover("r1", t0 + Duration::seconds(240)));
    }

    #[test]
    fn flapping_resignals_after_throttle_window_elapses() {
        let mut reg = RunnerRegistry::new();
        let t0 = now();
        for i in 0..2 {
            assert!(!reg.record_takeover("r1", t0 + Duration::seconds(i * 60)));
        }
        assert!(reg.record_takeover("r1", t0 + Duration::seconds(120)));
        // Ping-pong continues past the throttle window: signal fires again.
        let later = t0 + Duration::seconds(700);
        assert!(!reg.record_takeover("r1", later));
        assert!(!reg.record_takeover("r1", later + Duration::seconds(60)));
        assert!(reg.record_takeover("r1", later + Duration::seconds(120)));
    }

    #[test]
    fn no_flapping_when_takeovers_are_spread_out() {
        let mut reg = RunnerRegistry::new();
        let t0 = now();
        // Three takeovers, each more than a window apart — ordinary
        // restarts, never flapping.
        assert!(!reg.record_takeover("r1", t0));
        assert!(!reg.record_takeover("r1", t0 + Duration::seconds(700)));
        assert!(!reg.record_takeover("r1", t0 + Duration::seconds(1400)));
    }

    #[test]
    fn flapping_is_tracked_per_runner_id() {
        let mut reg = RunnerRegistry::new();
        let t0 = now();
        assert!(!reg.record_takeover("r1", t0));
        assert!(!reg.record_takeover("r2", t0 + Duration::seconds(30)));
        assert!(!reg.record_takeover("r1", t0 + Duration::seconds(60)));
        assert!(!reg.record_takeover("r2", t0 + Duration::seconds(90)));
        assert!(reg.record_takeover("r1", t0 + Duration::seconds(120)));
        assert!(reg.record_takeover("r2", t0 + Duration::seconds(150)));
    }
}

//! Capability router: matches work items to eligible runners.

use crate::types::{Runner, WorkItem};

/// Stateless capability matcher.
///
/// Rules:
/// - A runner **must** have all capabilities listed in `work.require`.
/// - A runner **scores higher** for each capability it has from `work.prefer`.
/// - Among equally scored runners, the one with more available capacity wins.
pub struct CapabilityRouter;

impl CapabilityRouter {
    /// Returns `true` if `runner` satisfies all required capabilities.
    pub fn can_handle(runner: &Runner, work: &WorkItem) -> bool {
        work.require
            .iter()
            .all(|req| runner.capabilities.contains(req))
    }

    /// Score a runner's fit for a work item (higher = better).
    ///
    /// - +1 per matched preferred capability
    /// - +1 if runner has spare capacity (inflight < max_inflight)
    pub fn score(runner: &Runner, work: &WorkItem) -> u32 {
        let preferred_matches = work
            .prefer
            .iter()
            .filter(|pref| runner.capabilities.contains(pref))
            .count() as u32;

        let capacity_bonus = if runner.has_capacity() { 1 } else { 0 };

        preferred_matches + capacity_bonus
    }

    /// From a slice of candidate runners, pick the best one for the given work.
    ///
    /// Eligibility (required capabilities) is checked first; then the highest
    /// scorer wins. Returns `None` if no runner is eligible.
    pub fn best<'a>(runners: &[&'a Runner], work: &WorkItem) -> Option<&'a Runner> {
        runners
            .iter()
            .filter(|r| r.has_capacity() && Self::can_handle(r, work))
            .max_by_key(|r| Self::score(r, work))
            .copied()
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::types::Runner;

    fn runner(id: &str, caps: Vec<&str>, inflight: u32, max: u32) -> Runner {
        Runner {
            runner_id: id.into(),
            capabilities: caps.into_iter().map(String::from).collect(),
            max_inflight: max,
            last_poll_at: Utc::now(),
            inflight: (0..inflight).map(|i| format!("exec-{i}")).collect(),
            instance_id: None,
        }
    }

    fn work(require: Vec<&str>, prefer: Vec<&str>) -> WorkItem {
        WorkItem {
            execution_id: "exec-1".into(),
            job_key: "test:job".into(),
            fire_at: Utc::now(),
            attempt: 1,
            require: require.into_iter().map(String::from).collect(),
            prefer: prefer.into_iter().map(String::from).collect(),
            metadata: serde_json::Value::Null,
            timeout: "5m".into(),
        }
    }

    #[test]
    fn can_handle_all_required() {
        let r = runner("r1", vec!["billing", "eu-central"], 0, 3);
        let w = work(vec!["billing"], vec![]);
        assert!(CapabilityRouter::can_handle(&r, &w));
    }

    #[test]
    fn cannot_handle_missing_required() {
        let r = runner("r1", vec!["etl"], 0, 3);
        let w = work(vec!["billing"], vec![]);
        assert!(!CapabilityRouter::can_handle(&r, &w));
    }

    #[test]
    fn no_requirements_matches_any() {
        let r = runner("r1", vec![], 0, 3);
        let w = work(vec![], vec![]);
        assert!(CapabilityRouter::can_handle(&r, &w));
    }

    #[test]
    fn score_preferred_bonus() {
        let r_with_pref = runner("r1", vec!["billing", "eu-central"], 0, 3);
        let r_without_pref = runner("r2", vec!["billing"], 0, 3);
        let w = work(vec!["billing"], vec!["eu-central"]);

        assert!(
            CapabilityRouter::score(&r_with_pref, &w)
                > CapabilityRouter::score(&r_without_pref, &w)
        );
    }

    #[test]
    fn score_capacity_bonus() {
        let r_free = runner("r1", vec!["billing"], 0, 3);
        let r_full = runner("r2", vec!["billing"], 3, 3);
        let w = work(vec!["billing"], vec![]);

        assert!(CapabilityRouter::score(&r_free, &w) > CapabilityRouter::score(&r_full, &w));
    }

    #[test]
    fn best_picks_highest_scorer() {
        let r1 = runner("r1", vec!["billing"], 0, 3);
        let r2 = runner("r2", vec!["billing", "eu-central"], 0, 3);
        let w = work(vec!["billing"], vec!["eu-central"]);

        let refs: Vec<&Runner> = vec![&r1, &r2];
        let best = CapabilityRouter::best(&refs, &w).unwrap();
        assert_eq!(best.runner_id, "r2");
    }

    #[test]
    fn best_excludes_full_runners() {
        let r_full = runner("r1", vec!["billing"], 3, 3);
        let r_free = runner("r2", vec!["billing"], 0, 3);
        let w = work(vec!["billing"], vec![]);

        let refs: Vec<&Runner> = vec![&r_full, &r_free];
        let best = CapabilityRouter::best(&refs, &w).unwrap();
        assert_eq!(best.runner_id, "r2");
    }

    #[test]
    fn best_returns_none_when_no_eligible() {
        let r = runner("r1", vec!["etl"], 0, 3);
        let w = work(vec!["billing"], vec![]);

        let refs: Vec<&Runner> = vec![&r];
        assert!(CapabilityRouter::best(&refs, &w).is_none());
    }

    #[test]
    fn best_returns_none_for_empty_runner_list() {
        let w = work(vec![], vec![]);
        assert!(CapabilityRouter::best(&[], &w).is_none());
    }

    #[test]
    fn multiple_preferred_caps() {
        let r_two = runner("r1", vec!["billing", "eu-central", "priority"], 0, 3);
        let r_one = runner("r2", vec!["billing", "eu-central"], 0, 3);
        let w = work(vec!["billing"], vec!["eu-central", "priority"]);

        let refs: Vec<&Runner> = vec![&r_one, &r_two];
        let best = CapabilityRouter::best(&refs, &w).unwrap();
        assert_eq!(best.runner_id, "r1");
    }
}

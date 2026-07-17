//! Work queue: pending executions waiting to be dispatched to runners.

use std::collections::{HashMap, VecDeque};

use crate::types::WorkItem;

/// FIFO queue of pending work items.
///
/// `dequeue_for` respects required capabilities: it returns the first item
/// the requesting runner is eligible to execute. Items that don't match are
/// left in place (not re-ordered), preserving fairness across runners with
/// different capability sets.
///
/// Maintains an O(1) per-`job_key` counter (`per_job_count`) so the scheduler
/// can enforce `max_queue_depth` without scanning the queue on every tick.
#[derive(Debug, Default)]
pub struct WorkQueue {
    items: VecDeque<WorkItem>,
    /// Number of currently-queued items keyed by `job_key`. Updated in
    /// lockstep with `items` by every mutating method on this struct.
    /// Entries are removed when the count reaches zero so iteration stays
    /// proportional to active jobs, not total job_keys ever seen.
    per_job_count: HashMap<String, usize>,
}

impl WorkQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a work item at the back of the queue.
    pub fn enqueue(&mut self, item: WorkItem) {
        *self.per_job_count.entry(item.job_key.clone()).or_insert(0) += 1;
        self.items.push_back(item);
    }

    /// Remove and return the first item this runner can execute, based on
    /// required capabilities.
    ///
    /// Returns `None` if no eligible item exists.
    pub fn dequeue_for(&mut self, capabilities: &[String]) -> Option<WorkItem> {
        self.dequeue_for_where(capabilities, |_| true)
    }

    /// Like [`Self::dequeue_for`], but additionally requires `eligible` to
    /// return `true` for the item. Ineligible items are skipped in place —
    /// the same semantics as a capability mismatch — so a blocked item keeps
    /// its FIFO position without starving items queued behind it.
    ///
    /// Used by the per-job concurrency guard (issue #278): the server passes
    /// a predicate that rejects items whose job already has `max_concurrent`
    /// executions in flight, and the item simply stays queued.
    pub fn dequeue_for_where(
        &mut self,
        capabilities: &[String],
        mut eligible: impl FnMut(&WorkItem) -> bool,
    ) -> Option<WorkItem> {
        let pos = self.items.iter().position(|item| {
            item.require.iter().all(|req| capabilities.contains(req)) && eligible(item)
        });

        pos.map(|i| {
            let item = self.items.remove(i).expect("index just found");
            self.dec_count(&item.job_key);
            item
        })
    }

    /// Remove up to `limit` eligible items for a runner in one call.
    pub fn dequeue_many_for(&mut self, capabilities: &[String], limit: usize) -> Vec<WorkItem> {
        let mut result = Vec::with_capacity(limit);
        while result.len() < limit {
            match self.dequeue_for(capabilities) {
                Some(item) => result.push(item),
                None => break,
            }
        }
        result
    }

    /// Peek at the first item without removing it.
    pub fn peek(&self) -> Option<&WorkItem> {
        self.items.front()
    }

    /// Peek at the first `n` items without removing them.
    pub fn peek_n(&self, n: usize) -> Vec<&WorkItem> {
        self.items.iter().take(n).collect()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Number of currently-queued items for `job_key`. O(1) lookup;
    /// the scheduler tick uses this to enforce `max_queue_depth` instead
    /// of scanning the queue every second.
    pub fn count_for_job(&self, job_key: &str) -> usize {
        self.per_job_count.get(job_key).copied().unwrap_or(0)
    }

    /// Remove a specific execution by ID (e.g. when cancelled before dispatch).
    ///
    /// Returns `true` if an item was found and removed.
    pub fn remove(&mut self, execution_id: &str) -> bool {
        if let Some(idx) = self
            .items
            .iter()
            .position(|i| i.execution_id == execution_id)
        {
            let removed = self.items.remove(idx).expect("index just found");
            self.dec_count(&removed.job_key);
            true
        } else {
            false
        }
    }

    /// Remove every queued item for `job_key`, returning the removed
    /// execution IDs. Used by the scheduler to enforce "keep only the latest"
    /// semantics for ephemeral jobs (issue #263): a fresh ephemeral fire
    /// replaces any earlier, still-unclaimed one so non-persisted work can't
    /// pile up against `max_queue_depth` while no runner is draining.
    pub fn remove_job(&mut self, job_key: &str) -> Vec<String> {
        let mut removed = Vec::new();
        let mut i = 0;
        while i < self.items.len() {
            if self.items[i].job_key == job_key {
                let item = self.items.remove(i).expect("index in bounds");
                removed.push(item.execution_id);
            } else {
                i += 1;
            }
        }
        if !removed.is_empty() {
            self.per_job_count.remove(job_key);
        }
        removed
    }

    /// Drain all items from the queue (e.g. for shutdown).
    pub fn drain(&mut self) -> Vec<WorkItem> {
        self.per_job_count.clear();
        self.items.drain(..).collect()
    }

    fn dec_count(&mut self, job_key: &str) {
        if let Some(count) = self.per_job_count.get_mut(job_key) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.per_job_count.remove(job_key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use pretty_assertions::assert_eq;

    use super::*;

    fn item(id: &str, require: Vec<&str>) -> WorkItem {
        WorkItem {
            execution_id: id.into(),
            job_key: format!("job:{id}"),
            fire_at: Utc::now(),
            scheduled_for: Utc::now(),
            attempt: 1,
            require: require.into_iter().map(|s| s.to_string()).collect(),
            prefer: vec![],
            metadata: serde_json::Value::Null,
            timeout: "5m".into(),
        }
    }

    #[test]
    fn enqueue_and_dequeue() {
        let mut q = WorkQueue::new();
        q.enqueue(item("exec-1", vec![]));

        let result = q.dequeue_for(&["billing".to_string()]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().execution_id, "exec-1");
        assert!(q.is_empty());
    }

    #[test]
    fn empty_queue_returns_none() {
        let mut q = WorkQueue::new();
        assert!(q.dequeue_for(&[]).is_none());
    }

    #[test]
    fn required_cap_filters_out_ineligible() {
        let mut q = WorkQueue::new();
        q.enqueue(item("exec-billing", vec!["billing"]));
        q.enqueue(item("exec-etl", vec!["etl"]));

        // Runner only has "etl"
        let got = q.dequeue_for(&["etl".to_string()]).unwrap();
        assert_eq!(got.execution_id, "exec-etl");

        // "exec-billing" is still in queue
        assert_eq!(q.len(), 1);
        assert_eq!(q.peek().unwrap().execution_id, "exec-billing");
    }

    #[test]
    fn multiple_required_caps_must_all_match() {
        let mut q = WorkQueue::new();
        // Requires both billing AND eu-central
        q.enqueue(item("exec-1", vec!["billing", "eu-central"]));

        // Runner has only billing
        assert!(q.dequeue_for(&["billing".to_string()]).is_none());

        // Runner has both
        let got = q.dequeue_for(&["billing".to_string(), "eu-central".to_string()]);
        assert!(got.is_some());
    }

    #[test]
    fn no_required_caps_matches_any_runner() {
        let mut q = WorkQueue::new();
        q.enqueue(item("exec-open", vec![]));

        // Even an empty capabilities runner can claim it
        let got = q.dequeue_for(&[]);
        assert!(got.is_some());
    }

    #[test]
    fn dequeue_many_respects_limit() {
        let mut q = WorkQueue::new();
        for i in 0..5 {
            q.enqueue(item(&format!("exec-{i}"), vec![]));
        }

        let batch = q.dequeue_many_for(&[], 3);
        assert_eq!(batch.len(), 3);
        assert_eq!(q.len(), 2);
    }

    #[test]
    fn dequeue_many_stops_when_queue_exhausted() {
        let mut q = WorkQueue::new();
        q.enqueue(item("exec-1", vec![]));
        q.enqueue(item("exec-2", vec![]));

        let batch = q.dequeue_many_for(&[], 10);
        assert_eq!(batch.len(), 2);
        assert!(q.is_empty());
    }

    #[test]
    fn remove_by_id() {
        let mut q = WorkQueue::new();
        q.enqueue(item("exec-1", vec![]));
        q.enqueue(item("exec-2", vec![]));

        assert!(q.remove("exec-1"));
        assert_eq!(q.len(), 1);
        assert_eq!(q.peek().unwrap().execution_id, "exec-2");
    }

    #[test]
    fn remove_nonexistent_returns_false() {
        let mut q = WorkQueue::new();
        q.enqueue(item("exec-1", vec![]));
        assert!(!q.remove("exec-does-not-exist"));
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn fifo_ordering_preserved() {
        let mut q = WorkQueue::new();
        for id in ["a", "b", "c"] {
            q.enqueue(item(id, vec![]));
        }

        assert_eq!(q.dequeue_for(&[]).unwrap().execution_id, "a");
        assert_eq!(q.dequeue_for(&[]).unwrap().execution_id, "b");
        assert_eq!(q.dequeue_for(&[]).unwrap().execution_id, "c");
    }

    #[test]
    fn drain_empties_queue() {
        let mut q = WorkQueue::new();
        for i in 0..3 {
            q.enqueue(item(&format!("exec-{i}"), vec![]));
        }

        let drained = q.drain();
        assert_eq!(drained.len(), 3);
        assert!(q.is_empty());
    }

    fn item_with_job(execution_id: &str, job_key: &str) -> WorkItem {
        WorkItem {
            execution_id: execution_id.into(),
            job_key: job_key.into(),
            fire_at: Utc::now(),
            scheduled_for: Utc::now(),
            attempt: 1,
            require: vec![],
            prefer: vec![],
            metadata: serde_json::Value::Null,
            timeout: "5m".into(),
        }
    }

    #[test]
    fn count_for_job_tracks_enqueue_and_dequeue() {
        let mut q = WorkQueue::new();
        assert_eq!(q.count_for_job("billing:invoice"), 0);

        q.enqueue(item_with_job("e1", "billing:invoice"));
        q.enqueue(item_with_job("e2", "billing:invoice"));
        q.enqueue(item_with_job("e3", "etl:sync"));

        assert_eq!(q.count_for_job("billing:invoice"), 2);
        assert_eq!(q.count_for_job("etl:sync"), 1);
        assert_eq!(q.count_for_job("nonexistent"), 0);

        // Dequeue one billing:invoice → counter drops to 1
        let _ = q.dequeue_for(&[]);
        assert_eq!(
            q.count_for_job("billing:invoice") + q.count_for_job("etl:sync"),
            2
        );
    }

    #[test]
    fn count_for_job_handles_remove_by_id() {
        let mut q = WorkQueue::new();
        q.enqueue(item_with_job("e1", "billing:invoice"));
        q.enqueue(item_with_job("e2", "billing:invoice"));
        assert_eq!(q.count_for_job("billing:invoice"), 2);

        assert!(q.remove("e1"));
        assert_eq!(q.count_for_job("billing:invoice"), 1);

        assert!(q.remove("e2"));
        assert_eq!(q.count_for_job("billing:invoice"), 0);
        // No-op remove leaves counter alone
        assert!(!q.remove("nonexistent"));
        assert_eq!(q.count_for_job("billing:invoice"), 0);
    }

    #[test]
    fn remove_job_drops_all_items_for_key() {
        let mut q = WorkQueue::new();
        q.enqueue(item_with_job("e1", "beat:tick"));
        q.enqueue(item_with_job("e2", "etl:sync"));
        q.enqueue(item_with_job("e3", "beat:tick"));
        assert_eq!(q.count_for_job("beat:tick"), 2);

        let removed = q.remove_job("beat:tick");
        assert_eq!(removed, vec!["e1".to_string(), "e3".to_string()]);
        assert_eq!(q.count_for_job("beat:tick"), 0);
        // Unrelated job untouched
        assert_eq!(q.count_for_job("etl:sync"), 1);
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn remove_job_on_absent_key_is_noop() {
        let mut q = WorkQueue::new();
        q.enqueue(item_with_job("e1", "etl:sync"));
        assert!(q.remove_job("beat:tick").is_empty());
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn dequeue_for_where_skips_ineligible_items_in_place() {
        let mut q = WorkQueue::new();
        q.enqueue(item_with_job("e1", "guarded:job"));
        q.enqueue(item_with_job("e2", "other:job"));

        // Predicate blocks the guarded job — the next eligible item behind
        // it must still be dequeued (no starvation of other jobs).
        let got = q
            .dequeue_for_where(&[], |item| item.job_key != "guarded:job")
            .unwrap();
        assert_eq!(got.execution_id, "e2");

        // The blocked item keeps its position (and its per-job count).
        assert_eq!(q.len(), 1);
        assert_eq!(q.peek().unwrap().execution_id, "e1");
        assert_eq!(q.count_for_job("guarded:job"), 1);
    }

    #[test]
    fn dequeue_for_where_returns_none_when_all_blocked() {
        let mut q = WorkQueue::new();
        q.enqueue(item_with_job("e1", "guarded:job"));

        assert!(q.dequeue_for_where(&[], |_| false).is_none());
        // Item remains queued for a later attempt.
        assert_eq!(q.len(), 1);
        assert_eq!(q.count_for_job("guarded:job"), 1);
    }

    #[test]
    fn dequeue_for_where_still_respects_capabilities() {
        let mut q = WorkQueue::new();
        q.enqueue(item("exec-billing", vec!["billing"]));

        // Predicate says yes but the runner lacks the capability.
        assert!(q.dequeue_for_where(&["etl".into()], |_| true).is_none());
        let got = q.dequeue_for_where(&["billing".into()], |_| true);
        assert!(got.is_some());
    }

    #[test]
    fn count_for_job_resets_on_drain() {
        let mut q = WorkQueue::new();
        q.enqueue(item_with_job("e1", "billing:invoice"));
        q.enqueue(item_with_job("e2", "etl:sync"));
        assert_eq!(q.count_for_job("billing:invoice"), 1);

        let _ = q.drain();
        assert_eq!(q.count_for_job("billing:invoice"), 0);
        assert_eq!(q.count_for_job("etl:sync"), 0);
    }
}

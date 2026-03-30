//! Work queue: pending executions waiting to be dispatched to runners.

use std::collections::VecDeque;

use crate::types::WorkItem;

/// FIFO queue of pending work items.
///
/// `dequeue_for` respects required capabilities: it returns the first item
/// the requesting runner is eligible to execute. Items that don't match are
/// left in place (not re-ordered), preserving fairness across runners with
/// different capability sets.
#[derive(Debug, Default)]
pub struct WorkQueue {
    items: VecDeque<WorkItem>,
}

impl WorkQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a work item at the back of the queue.
    pub fn enqueue(&mut self, item: WorkItem) {
        self.items.push_back(item);
    }

    /// Remove and return the first item this runner can execute, based on
    /// required capabilities.
    ///
    /// Returns `None` if no eligible item exists.
    pub fn dequeue_for(&mut self, capabilities: &[String]) -> Option<WorkItem> {
        let pos = self
            .items
            .iter()
            .position(|item| item.require.iter().all(|req| capabilities.contains(req)));

        pos.map(|i| self.items.remove(i).expect("index just found"))
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

    /// Peek at the queue without removing items.
    pub fn peek(&self) -> Option<&WorkItem> {
        self.items.front()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Remove a specific execution by ID (e.g. when cancelled before dispatch).
    ///
    /// Returns `true` if an item was found and removed.
    pub fn remove(&mut self, execution_id: &str) -> bool {
        let before = self.items.len();
        self.items.retain(|i| i.execution_id != execution_id);
        self.items.len() < before
    }

    /// Drain all items from the queue (e.g. for shutdown).
    pub fn drain(&mut self) -> Vec<WorkItem> {
        self.items.drain(..).collect()
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
}

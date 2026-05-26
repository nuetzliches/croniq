//! Contract tests: verify store implementations satisfy the trait contracts.
//! These tests run against both SQLite and in-memory backends.

use crate::memory::create_memory_store;
use crate::models::*;
use crate::traits::*;
use chrono::{TimeZone, Utc};
use std::collections::HashMap;
use uuid::Uuid;

fn utc(y: i32, m: u32, d: u32, h: u32, min: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(y, m, d, h, min, 0).unwrap()
}

fn now() -> chrono::DateTime<Utc> {
    utc(2026, 3, 29, 12, 0)
}

fn make_execution(job_key: &str, fire_at: chrono::DateTime<Utc>) -> Execution {
    Execution {
        id: Uuid::new_v4(),
        job_key: job_key.to_string(),
        fire_at,
        attempt: 1,
        state: ExecutionState::Queued,
        runner_id: None,
        claimed_at: None,
        started_at: None,
        completed_at: None,
        duration_ms: None,
        error: None,
        dead_reason: None,
        metadata: HashMap::new(),
        created_at: now(),
    }
}

// ─── JobStore ───

#[test]
fn job_state_upsert_and_get() {
    let store = create_memory_store().unwrap();

    let state = JobState {
        job_key: "billing:invoice".into(),
        next_fire_at: Some(utc(2026, 3, 30, 2, 0)),
        last_fired_at: None,
        fire_count: 0,
        status: JobStatus::Active,
        updated_at: now(),
    };

    store.upsert_job_state(&state).unwrap();
    let loaded = store.get_job_state("billing:invoice").unwrap().unwrap();

    assert_eq!(loaded.job_key, "billing:invoice");
    assert_eq!(loaded.fire_count, 0);
    assert_eq!(loaded.status, JobStatus::Active);
}

#[test]
fn job_state_update() {
    let store = create_memory_store().unwrap();

    let mut state = JobState {
        job_key: "etl:sync".into(),
        next_fire_at: Some(utc(2026, 3, 29, 12, 15)),
        last_fired_at: None,
        fire_count: 0,
        status: JobStatus::Active,
        updated_at: now(),
    };

    store.upsert_job_state(&state).unwrap();

    state.fire_count = 42;
    state.last_fired_at = Some(now());
    store.upsert_job_state(&state).unwrap();

    let loaded = store.get_job_state("etl:sync").unwrap().unwrap();
    assert_eq!(loaded.fire_count, 42);
    assert!(loaded.last_fired_at.is_some());
}

#[test]
fn job_state_list() {
    let store = create_memory_store().unwrap();

    for key in ["a:one", "b:two", "c:three"] {
        store
            .upsert_job_state(&JobState {
                job_key: key.into(),
                next_fire_at: None,
                last_fired_at: None,
                fire_count: 0,
                status: JobStatus::Active,
                updated_at: now(),
            })
            .unwrap();
    }

    let list = store.list_job_states().unwrap();
    assert_eq!(list.len(), 3);
}

#[test]
fn job_state_delete() {
    let store = create_memory_store().unwrap();

    store
        .upsert_job_state(&JobState {
            job_key: "temp:job".into(),
            next_fire_at: None,
            last_fired_at: None,
            fire_count: 0,
            status: JobStatus::Active,
            updated_at: now(),
        })
        .unwrap();

    store.delete_job_state("temp:job").unwrap();
    assert!(store.get_job_state("temp:job").unwrap().is_none());
}

#[test]
fn job_state_not_found() {
    let store = create_memory_store().unwrap();
    assert!(store.get_job_state("nonexistent:job").unwrap().is_none());
}

#[test]
fn create_execution_and_advance_job_state_persists_both() {
    let store = create_memory_store().unwrap();

    let exec = make_execution("billing:invoice", utc(2026, 3, 29, 2, 0));
    let exec_id = exec.id;
    let job_state = JobState {
        job_key: "billing:invoice".into(),
        next_fire_at: Some(utc(2026, 3, 29, 3, 0)),
        last_fired_at: Some(utc(2026, 3, 29, 2, 0)),
        fire_count: 1,
        status: JobStatus::Active,
        updated_at: now(),
    };

    store
        .create_execution_and_advance_job_state(&exec, &job_state)
        .unwrap();

    // Both rows are committed.
    let loaded_exec = store.get_execution(exec_id).unwrap().unwrap();
    assert_eq!(loaded_exec.job_key, "billing:invoice");
    let loaded_state = store.get_job_state("billing:invoice").unwrap().unwrap();
    assert_eq!(loaded_state.fire_count, 1);
    assert_eq!(loaded_state.next_fire_at, Some(utc(2026, 3, 29, 3, 0)));
}

#[test]
fn create_execution_and_advance_job_state_atomically_rolls_back() {
    // Calling the method twice with the same execution id should fail
    // (PRIMARY KEY conflict on the second call) AND must not advance the
    // job_state on the failed second call. This proves the second
    // upsert_job_state is wrapped in the same transaction as the failing
    // execution insert — otherwise the state would update even though the
    // execution didn't, leaving the trigger desynced.
    let store = create_memory_store().unwrap();

    let exec = make_execution("etl:sync", utc(2026, 3, 29, 1, 0));
    let initial_state = JobState {
        job_key: "etl:sync".into(),
        next_fire_at: Some(utc(2026, 3, 29, 1, 15)),
        last_fired_at: Some(utc(2026, 3, 29, 1, 0)),
        fire_count: 1,
        status: JobStatus::Active,
        updated_at: now(),
    };

    store
        .create_execution_and_advance_job_state(&exec, &initial_state)
        .unwrap();

    // Second call with the same execution id and an *advanced* job_state.
    let advanced_state = JobState {
        next_fire_at: Some(utc(2026, 3, 29, 1, 30)),
        fire_count: 2,
        ..initial_state.clone()
    };
    let err = store
        .create_execution_and_advance_job_state(&exec, &advanced_state)
        .unwrap_err();
    let _ = err; // any DB error is fine — point is the call returned Err

    // job_state must NOT have advanced to fire_count=2 because the tx
    // rolled back.
    let loaded = store.get_job_state("etl:sync").unwrap().unwrap();
    assert_eq!(
        loaded.fire_count, 1,
        "job_state advanced even though execution insert failed"
    );
    assert_eq!(loaded.next_fire_at, Some(utc(2026, 3, 29, 1, 15)));
}

// ─── ExecutionStore ───

#[test]
fn execution_create_and_get() {
    let store = create_memory_store().unwrap();
    let exec = make_execution("billing:invoice", utc(2026, 3, 29, 2, 0));

    store.create_execution(&exec).unwrap();
    let loaded = store.get_execution(exec.id).unwrap().unwrap();

    assert_eq!(loaded.id, exec.id);
    assert_eq!(loaded.job_key, "billing:invoice");
    assert_eq!(loaded.state, ExecutionState::Queued);
}

#[test]
fn execution_claim() {
    let store = create_memory_store().unwrap();
    let exec = make_execution("billing:invoice", utc(2026, 3, 29, 2, 0));
    store.create_execution(&exec).unwrap();

    let claimed = store.claim_execution(exec.id, "runner-1", now()).unwrap();
    assert_eq!(claimed.state, ExecutionState::Claimed);
    assert_eq!(claimed.runner_id.as_deref(), Some("runner-1"));
    assert!(claimed.claimed_at.is_some());
}

#[test]
fn execution_claim_conflict() {
    let store = create_memory_store().unwrap();
    let exec = make_execution("billing:invoice", utc(2026, 3, 29, 2, 0));
    store.create_execution(&exec).unwrap();

    // First claim succeeds
    store.claim_execution(exec.id, "runner-1", now()).unwrap();

    // Second claim fails
    let result = store.claim_execution(exec.id, "runner-2", now());
    assert!(result.is_err());
}

#[test]
fn execution_complete_success() {
    let store = create_memory_store().unwrap();
    let exec = make_execution("etl:sync", utc(2026, 3, 29, 12, 0));
    store.create_execution(&exec).unwrap();
    store.claim_execution(exec.id, "runner-1", now()).unwrap();

    store
        .complete_execution(
            exec.id,
            ExecutionState::Completed,
            Some(4500),
            None,
            None,
            now(),
        )
        .unwrap();

    let loaded = store.get_execution(exec.id).unwrap().unwrap();
    assert_eq!(loaded.state, ExecutionState::Completed);
    assert_eq!(loaded.duration_ms, Some(4500));
}

#[test]
fn execution_complete_failure() {
    let store = create_memory_store().unwrap();
    let exec = make_execution("etl:sync", utc(2026, 3, 29, 12, 0));
    store.create_execution(&exec).unwrap();
    store.claim_execution(exec.id, "runner-1", now()).unwrap();

    store
        .complete_execution(
            exec.id,
            ExecutionState::Failed,
            Some(100),
            Some("connection refused"),
            None,
            now(),
        )
        .unwrap();

    let loaded = store.get_execution(exec.id).unwrap().unwrap();
    assert_eq!(loaded.state, ExecutionState::Failed);
    assert_eq!(loaded.error.as_deref(), Some("connection refused"));
}

#[test]
fn find_queued_executions() {
    let store = create_memory_store().unwrap();

    // Create 3 queued executions with different fire times
    for i in 0..3 {
        let mut exec = make_execution("etl:sync", utc(2026, 3, 29, 12, i));
        exec.id = Uuid::new_v4();
        store.create_execution(&exec).unwrap();
    }

    let queued = store.find_queued_executions(&[], 10).unwrap();
    assert_eq!(queued.len(), 3);
    // Should be ordered by fire_at
    assert!(queued[0].fire_at <= queued[1].fire_at);
}

#[test]
fn find_queued_executions_filters_by_capabilities() {
    let store = create_memory_store().unwrap();

    // Execution requiring "billing" capability
    let mut exec1 = make_execution("billing:invoice", utc(2026, 3, 29, 12, 0));
    exec1.metadata.insert(
        "__require".into(),
        serde_json::to_string(&vec!["billing"]).unwrap(),
    );
    store.create_execution(&exec1).unwrap();

    // Execution requiring "etl" capability
    let mut exec2 = make_execution("etl:sync", utc(2026, 3, 29, 12, 1));
    exec2.id = Uuid::new_v4();
    exec2.metadata.insert(
        "__require".into(),
        serde_json::to_string(&vec!["etl"]).unwrap(),
    );
    store.create_execution(&exec2).unwrap();

    // Execution with no requirements
    let mut exec3 = make_execution("reports:daily", utc(2026, 3, 29, 12, 2));
    exec3.id = Uuid::new_v4();
    store.create_execution(&exec3).unwrap();

    // Runner with "billing" capability should see exec1 + exec3
    let billing = store
        .find_queued_executions(&["billing".into()], 10)
        .unwrap();
    assert_eq!(billing.len(), 2);
    assert!(billing.iter().any(|e| e.job_key == "billing:invoice"));
    assert!(billing.iter().any(|e| e.job_key == "reports:daily"));

    // Runner with "etl" capability should see exec2 + exec3
    let etl = store.find_queued_executions(&["etl".into()], 10).unwrap();
    assert_eq!(etl.len(), 2);
    assert!(etl.iter().any(|e| e.job_key == "etl:sync"));
    assert!(etl.iter().any(|e| e.job_key == "reports:daily"));

    // Empty capabilities = all executions (no filtering)
    let all = store.find_queued_executions(&[], 10).unwrap();
    assert_eq!(all.len(), 3);
}

#[test]
fn requeue_abandoned() {
    let store = create_memory_store().unwrap();
    let exec = make_execution("billing:invoice", utc(2026, 3, 29, 2, 0));
    store.create_execution(&exec).unwrap();
    store
        .claim_execution(exec.id, "dead-runner", now())
        .unwrap();

    let requeued = store.requeue_abandoned("dead-runner", now()).unwrap();
    assert_eq!(requeued.len(), 1);
    assert_eq!(requeued[0], exec.id);

    let loaded = store.get_execution(exec.id).unwrap().unwrap();
    assert_eq!(loaded.state, ExecutionState::Queued);
    assert!(loaded.runner_id.is_none());
}

#[test]
fn cancel_execution() {
    let store = create_memory_store().unwrap();
    let exec = make_execution("etl:sync", utc(2026, 3, 29, 12, 0));
    store.create_execution(&exec).unwrap();

    store.cancel_execution(exec.id, now()).unwrap();

    let loaded = store.get_execution(exec.id).unwrap().unwrap();
    assert_eq!(loaded.state, ExecutionState::Cancelled);
}

#[test]
fn count_by_state() {
    let store = create_memory_store().unwrap();

    for _ in 0..3 {
        let exec = make_execution("a:job", utc(2026, 3, 29, 12, 0));
        store.create_execution(&exec).unwrap();
    }

    let exec = make_execution("b:job", utc(2026, 3, 29, 12, 0));
    store.create_execution(&exec).unwrap();
    store.claim_execution(exec.id, "r1", now()).unwrap();

    let counts = store.count_by_state().unwrap();
    assert_eq!(*counts.get(&ExecutionState::Queued).unwrap_or(&0), 3);
    assert_eq!(*counts.get(&ExecutionState::Claimed).unwrap_or(&0), 1);
}

#[test]
fn job_execution_metrics_aggregates_per_job() {
    let store = create_memory_store().unwrap();

    // Two completed runs (4.5s and 120s) + one dead run (0.2s) all record a
    // duration; a cancelled run never does.
    let complete = |dur_ms: i64, state: ExecutionState| {
        let exec = make_execution("metrics:job", utc(2026, 3, 29, 12, 0));
        store.create_execution(&exec).unwrap();
        store.claim_execution(exec.id, "r1", now()).unwrap();
        store
            .complete_execution(exec.id, state, Some(dur_ms), None, None, now())
            .unwrap();
    };
    complete(4_500, ExecutionState::Completed);
    complete(120_000, ExecutionState::Completed);
    complete(200, ExecutionState::Dead);

    let cancelled = make_execution("metrics:job", utc(2026, 3, 29, 12, 0));
    store.create_execution(&cancelled).unwrap();
    store.cancel_execution(cancelled.id, now()).unwrap();

    // A second job proves rows are grouped by job_key, not summed globally.
    let other = make_execution("other:job", utc(2026, 3, 29, 12, 0));
    store.create_execution(&other).unwrap();
    store.claim_execution(other.id, "r1", now()).unwrap();
    store
        .complete_execution(
            other.id,
            ExecutionState::Completed,
            Some(50),
            None,
            None,
            now(),
        )
        .unwrap();

    let all = store.job_execution_metrics().unwrap();
    let m = all
        .iter()
        .find(|m| m.job_key == "metrics:job")
        .expect("metrics:job aggregate row");

    assert_eq!(m.completed, 2);
    assert_eq!(m.failed, 0);
    assert_eq!(m.dead, 1);
    assert_eq!(m.cancelled, 1);
    assert_eq!(m.duration_count, 3);
    assert_eq!(m.duration_sum_ms, 124_700);
    assert_eq!(m.last_run_at, Some(now()));

    // Cumulative buckets over {0.2s, 4.5s, 120s} for the shared boundaries
    // [0.1, 0.5, 1, 5, 10, 30, 60, 300] seconds.
    assert_eq!(m.duration_buckets, vec![0, 1, 1, 2, 2, 2, 2, 3]);

    let other_m = all.iter().find(|m| m.job_key == "other:job").unwrap();
    assert_eq!(other_m.completed, 1);
    assert_eq!(other_m.duration_count, 1);
}

#[test]
fn list_executions_with_filter() {
    let store = create_memory_store().unwrap();

    let e1 = make_execution("billing:invoice", utc(2026, 3, 29, 2, 0));
    let e2 = make_execution("etl:sync", utc(2026, 3, 29, 12, 0));
    store.create_execution(&e1).unwrap();
    store.create_execution(&e2).unwrap();

    let results = store
        .list_executions(&ExecutionFilter {
            job_key: Some("billing:invoice".into()),
            ..Default::default()
        })
        .unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].job_key, "billing:invoice");
}

// ─── RunnerStore ───

#[test]
fn runner_register_and_get() {
    let store = create_memory_store().unwrap();

    let runner = Runner {
        runner_id: "billing-1".into(),
        capabilities: vec!["billing".into(), "eu-central".into()],
        max_inflight: 3,
        last_poll_at: now(),
        inflight: vec![],
        status: RunnerStatus::Online,
        registered_at: now(),
    };

    store.upsert_runner(&runner).unwrap();
    let loaded = store.get_runner("billing-1").unwrap().unwrap();

    assert_eq!(loaded.runner_id, "billing-1");
    assert_eq!(loaded.capabilities, vec!["billing", "eu-central"]);
    assert_eq!(loaded.max_inflight, 3);
}

#[test]
fn runner_update_poll() {
    let store = create_memory_store().unwrap();

    let runner = Runner {
        runner_id: "r1".into(),
        capabilities: vec![],
        max_inflight: 1,
        last_poll_at: now(),
        inflight: vec![],
        status: RunnerStatus::Online,
        registered_at: now(),
    };
    store.upsert_runner(&runner).unwrap();

    let exec_id = Uuid::new_v4();
    let later = utc(2026, 3, 29, 12, 5);
    store.update_poll("r1", &[exec_id], later).unwrap();

    let loaded = store.get_runner("r1").unwrap().unwrap();
    assert_eq!(loaded.inflight, vec![exec_id]);
    assert_eq!(loaded.last_poll_at, later);
}

#[test]
fn runner_list_and_remove() {
    let store = create_memory_store().unwrap();

    for id in ["r1", "r2", "r3"] {
        store
            .upsert_runner(&Runner {
                runner_id: id.into(),
                capabilities: vec![],
                max_inflight: 1,
                last_poll_at: now(),
                inflight: vec![],
                status: RunnerStatus::Online,
                registered_at: now(),
            })
            .unwrap();
    }

    assert_eq!(store.list_runners().unwrap().len(), 3);

    store.remove_runner("r2").unwrap();
    assert_eq!(store.list_runners().unwrap().len(), 2);
    assert!(store.get_runner("r2").unwrap().is_none());
}

// ─── DeadLetterStore ───

#[test]
fn dead_letter_add_and_get() {
    let store = create_memory_store().unwrap();

    let dl = DeadLetter {
        id: Uuid::new_v4(),
        execution_id: Uuid::new_v4(),
        job_key: "billing:invoice".into(),
        fire_at: utc(2026, 3, 29, 2, 0),
        attempt: 5,
        error: "max retries exhausted".into(),
        dead_reason: "retry_exhausted".into(),
        metadata: HashMap::new(),
        created_at: now(),
        expires_at: Some(utc(2026, 4, 28, 12, 0)),
    };

    store.add_dead_letter(&dl).unwrap();
    let loaded = store.get_dead_letter(dl.id).unwrap().unwrap();

    assert_eq!(loaded.job_key, "billing:invoice");
    assert_eq!(loaded.attempt, 5);
    assert_eq!(loaded.error, "max retries exhausted");
}

#[test]
fn dead_letter_list_and_remove() {
    let store = create_memory_store().unwrap();

    for _ in 0..3 {
        store
            .add_dead_letter(&DeadLetter {
                id: Uuid::new_v4(),
                execution_id: Uuid::new_v4(),
                job_key: "etl:sync".into(),
                fire_at: now(),
                attempt: 3,
                error: "timeout".into(),
                dead_reason: "timeout".into(),
                metadata: HashMap::new(),
                created_at: now(),
                expires_at: None,
            })
            .unwrap();
    }

    let list = store
        .list_dead_letters(&DeadLetterFilter::default())
        .unwrap();
    assert_eq!(list.len(), 3);

    store.remove_dead_letter(list[0].id).unwrap();
    assert_eq!(
        store
            .list_dead_letters(&DeadLetterFilter::default())
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn complete_as_dead_writes_execution_and_dead_letter_atomically() {
    let store = create_memory_store().unwrap();

    let exec = make_execution("billing:invoice", utc(2026, 3, 29, 2, 0));
    store.create_execution(&exec).unwrap();

    let dl = DeadLetter {
        id: Uuid::new_v4(),
        execution_id: exec.id,
        job_key: exec.job_key.clone(),
        fire_at: exec.fire_at,
        attempt: 3,
        error: "connection refused".into(),
        dead_reason: "exhausted after 3 attempts: connection refused".into(),
        metadata: HashMap::new(),
        created_at: now(),
        expires_at: Some(utc(2026, 4, 28, 12, 0)),
    };

    store
        .complete_as_dead(exec.id, Some(123), Some("connection refused"), &dl, now())
        .unwrap();

    // Execution row is now in `dead` state.
    let updated = store.get_execution(exec.id).unwrap().unwrap();
    assert_eq!(updated.state, ExecutionState::Dead);
    assert_eq!(updated.duration_ms, Some(123));
    assert_eq!(updated.error.as_deref(), Some("connection refused"));
    assert_eq!(
        updated.dead_reason.as_deref(),
        Some("exhausted after 3 attempts: connection refused")
    );

    // Matching dead-letter row was inserted.
    let stored = store.get_dead_letter(dl.id).unwrap().unwrap();
    assert_eq!(stored.execution_id, exec.id);
    assert_eq!(stored.attempt, 3);
    assert_eq!(stored.expires_at, dl.expires_at);
}

#[test]
fn complete_as_dead_rolls_back_when_dead_letter_id_collides() {
    let store = create_memory_store().unwrap();

    let exec = make_execution("etl:sync", utc(2026, 3, 29, 2, 0));
    store.create_execution(&exec).unwrap();

    // Pre-seed a dead-letter row with a fixed ID so the second insert collides.
    let conflicting_id = Uuid::new_v4();
    store
        .add_dead_letter(&DeadLetter {
            id: conflicting_id,
            execution_id: Uuid::new_v4(),
            job_key: "other:job".into(),
            fire_at: now(),
            attempt: 1,
            error: "x".into(),
            dead_reason: "x".into(),
            metadata: HashMap::new(),
            created_at: now(),
            expires_at: None,
        })
        .unwrap();

    let dl = DeadLetter {
        id: conflicting_id, // primary-key collision
        execution_id: exec.id,
        job_key: exec.job_key.clone(),
        fire_at: exec.fire_at,
        attempt: 3,
        error: "boom".into(),
        dead_reason: "exhausted".into(),
        metadata: HashMap::new(),
        created_at: now(),
        expires_at: None,
    };

    let res = store.complete_as_dead(exec.id, Some(50), Some("boom"), &dl, now());
    assert!(
        res.is_err(),
        "expected complete_as_dead to fail on PK collision"
    );

    // Execution must still be in its original Queued state — the UPDATE was
    // rolled back together with the failing INSERT.
    let untouched = store.get_execution(exec.id).unwrap().unwrap();
    assert_eq!(
        untouched.state,
        ExecutionState::Queued,
        "execution state changed despite failed dead-letter insert — atomicity broken"
    );
}

#[test]
fn dead_letter_purge_expired() {
    let store = create_memory_store().unwrap();

    // Expired
    store
        .add_dead_letter(&DeadLetter {
            id: Uuid::new_v4(),
            execution_id: Uuid::new_v4(),
            job_key: "a:job".into(),
            fire_at: now(),
            attempt: 1,
            error: "err".into(),
            dead_reason: "reason".into(),
            metadata: HashMap::new(),
            created_at: now(),
            expires_at: Some(utc(2026, 3, 28, 0, 0)), // yesterday
        })
        .unwrap();

    // Not expired
    store
        .add_dead_letter(&DeadLetter {
            id: Uuid::new_v4(),
            execution_id: Uuid::new_v4(),
            job_key: "b:job".into(),
            fire_at: now(),
            attempt: 1,
            error: "err".into(),
            dead_reason: "reason".into(),
            metadata: HashMap::new(),
            created_at: now(),
            expires_at: Some(utc(2026, 4, 28, 0, 0)), // next month
        })
        .unwrap();

    let purged = store.purge_expired(now()).unwrap();
    assert_eq!(purged, 1);
    assert_eq!(
        store
            .list_dead_letters(&DeadLetterFilter::default())
            .unwrap()
            .len(),
        1
    );
}

// ─── ExecutionLogStore ───

fn log_entry(execution_id: Uuid, level: &str, message: &str) -> ExecutionLogEntry {
    ExecutionLogEntry {
        id: Uuid::new_v4(),
        execution_id,
        timestamp: now(),
        level: level.into(),
        message: message.into(),
        fields: HashMap::new(),
        seq: 0, // ignored by the store; assigned at insert
    }
}

/// Seed an execution row so the FOREIGN KEY on `execution_logs.execution_id`
/// is satisfied. Returns the row's UUID.
fn seed_execution(store: &impl ExecutionStore, suffix: &str) -> Uuid {
    let exec = make_execution(&format!("log:test:{suffix}"), utc(2026, 3, 29, 2, 0));
    let id = exec.id;
    store.create_execution(&exec).unwrap();
    id
}

#[test]
fn append_logs_batch_assigns_strictly_increasing_seq() {
    let store = create_memory_store().unwrap();
    let exec = seed_execution(&store, "a");

    let entries: Vec<ExecutionLogEntry> = (0..5)
        .map(|i| log_entry(exec, "info", &format!("line {i}")))
        .collect();

    store.append_logs_batch(&entries).unwrap();

    let read = store.read_logs(exec, 100).unwrap();
    assert_eq!(read.len(), 5);
    let seqs: Vec<i64> = read.iter().map(|l| l.seq).collect();
    assert_eq!(seqs, vec![0, 1, 2, 3, 4]);
    // Order matches insertion order — same timestamp, seq is the tiebreaker.
    assert_eq!(read[0].message, "line 0");
    assert_eq!(read[4].message, "line 4");
}

#[test]
fn append_logs_batch_continues_seq_across_calls() {
    let store = create_memory_store().unwrap();
    let exec = seed_execution(&store, "b");

    let first: Vec<_> = (0..3)
        .map(|i| log_entry(exec, "info", &format!("a{i}")))
        .collect();
    store.append_logs_batch(&first).unwrap();

    let second: Vec<_> = (0..2)
        .map(|i| log_entry(exec, "warn", &format!("b{i}")))
        .collect();
    store.append_logs_batch(&second).unwrap();

    let read = store.read_logs(exec, 100).unwrap();
    let seqs: Vec<i64> = read.iter().map(|l| l.seq).collect();
    // The second batch must NOT reuse seq 0,1 — those belong to the first.
    assert_eq!(seqs, vec![0, 1, 2, 3, 4]);
}

#[test]
fn append_log_single_also_assigns_seq() {
    let store = create_memory_store().unwrap();
    let exec = seed_execution(&store, "c");

    store.append_log(&log_entry(exec, "info", "first")).unwrap();
    store
        .append_log(&log_entry(exec, "info", "second"))
        .unwrap();

    let read = store.read_logs(exec, 100).unwrap();
    assert_eq!(read.len(), 2);
    assert_eq!(read[0].seq, 0);
    assert_eq!(read[1].seq, 1);
}

#[test]
fn append_logs_batch_isolates_seq_per_execution() {
    let store = create_memory_store().unwrap();
    let exec_a = seed_execution(&store, "iso-a");
    let exec_b = seed_execution(&store, "iso-b");

    let mixed = vec![
        log_entry(exec_a, "info", "a0"),
        log_entry(exec_b, "info", "b0"),
        log_entry(exec_a, "info", "a1"),
        log_entry(exec_b, "info", "b1"),
    ];
    store.append_logs_batch(&mixed).unwrap();

    let read_a = store.read_logs(exec_a, 100).unwrap();
    let read_b = store.read_logs(exec_b, 100).unwrap();

    let seqs_a: Vec<i64> = read_a.iter().map(|l| l.seq).collect();
    let seqs_b: Vec<i64> = read_b.iter().map(|l| l.seq).collect();
    assert_eq!(seqs_a, vec![0, 1]);
    assert_eq!(seqs_b, vec![0, 1]);
}

#[test]
fn append_logs_batch_empty_is_a_noop() {
    let store = create_memory_store().unwrap();
    store.append_logs_batch(&[]).unwrap();
    // No assertion needed — just confirming it doesn't panic or error.
}

// ─── Users ───

fn make_user(username: &str, role: Role) -> User {
    User {
        user_id: Uuid::new_v4().to_string(),
        username: username.into(),
        email: None,
        display_name: None,
        role,
        is_active: true,
        created_at: now(),
        updated_at: now(),
        last_login_at: None,
    }
}

#[test]
fn users_create_and_lookup_round_trip() {
    let store = create_memory_store().unwrap();
    let u = make_user("alex", Role::Admin);
    store.users_create(&u).unwrap();

    let by_id = store.users_get_by_id(&u.user_id).unwrap().unwrap();
    let by_name = store.users_get_by_username("alex").unwrap().unwrap();

    assert_eq!(by_id.user_id, u.user_id);
    assert_eq!(by_name.user_id, u.user_id);
    assert_eq!(by_id.role, Role::Admin);
}

#[test]
fn users_create_is_upsert_on_user_id() {
    let store = create_memory_store().unwrap();
    let mut u = make_user("alex", Role::Operator);
    store.users_create(&u).unwrap();

    u.role = Role::Admin;
    u.email = Some("alex@example.org".into());
    store.users_create(&u).unwrap();

    let loaded = store.users_get_by_id(&u.user_id).unwrap().unwrap();
    assert_eq!(loaded.role, Role::Admin);
    assert_eq!(loaded.email.as_deref(), Some("alex@example.org"));
}

#[test]
fn users_list_returns_all_sorted_by_username() {
    let store = create_memory_store().unwrap();
    store
        .users_create(&make_user("carol", Role::Admin))
        .unwrap();
    store.users_create(&make_user("alex", Role::Admin)).unwrap();
    store
        .users_create(&make_user("bob", Role::Operator))
        .unwrap();

    let names: Vec<String> = store
        .users_list()
        .unwrap()
        .into_iter()
        .map(|u| u.username)
        .collect();
    assert_eq!(names, vec!["alex", "bob", "carol"]);
}

#[test]
fn users_set_last_login_updates_field() {
    let store = create_memory_store().unwrap();
    let u = make_user("alex", Role::Admin);
    store.users_create(&u).unwrap();

    let when = utc(2026, 5, 23, 14, 30);
    store.users_set_last_login(&u.user_id, when).unwrap();

    let loaded = store.users_get_by_id(&u.user_id).unwrap().unwrap();
    assert_eq!(loaded.last_login_at, Some(when));
}

#[test]
fn users_count_active_admins_excludes_deactivated_and_non_admins() {
    let store = create_memory_store().unwrap();

    let mut a1 = make_user("a1", Role::Admin);
    let mut a2 = make_user("a2", Role::Admin);
    let op = make_user("op", Role::Operator);
    let view = make_user("view", Role::Viewer);

    a2.is_active = false; // deactivated admin doesn't count
    a1.is_active = true;

    store.users_create(&a1).unwrap();
    store.users_create(&a2).unwrap();
    store.users_create(&op).unwrap();
    store.users_create(&view).unwrap();

    assert_eq!(store.users_count_active_admins().unwrap(), 1);
}

#[test]
fn users_delete_removes_row() {
    let store = create_memory_store().unwrap();
    let u = make_user("alex", Role::Admin);
    store.users_create(&u).unwrap();
    store.users_delete(&u.user_id).unwrap();
    assert!(store.users_get_by_id(&u.user_id).unwrap().is_none());
}

#[test]
fn users_get_by_id_unknown_returns_none() {
    let store = create_memory_store().unwrap();
    assert!(store.users_get_by_id("does-not-exist").unwrap().is_none());
}

// ─── Invitations + Password Resets ───

fn seed_admin(store: &impl AuthStore) -> User {
    let u = make_user("admin-issuer", Role::Admin);
    store.users_create(&u).unwrap();
    u
}

fn make_invitation(invited_by: &str, email: &str, role: Role) -> Invitation {
    Invitation {
        invitation_id: Uuid::new_v4().to_string(),
        email: email.into(),
        role,
        token_hash: format!("hash-{}", Uuid::new_v4()),
        invited_by: invited_by.into(),
        expires_at: utc(2026, 6, 1, 0, 0),
        accepted_at: None,
        revoked_at: None,
        created_at: now(),
    }
}

#[test]
fn invitation_create_and_lookup_round_trip() {
    let store = create_memory_store().unwrap();
    let admin = seed_admin(&store);
    let inv = make_invitation(&admin.user_id, "bob@example.org", Role::Operator);
    store.invitations_create(&inv).unwrap();

    let by_id = store.invitations_get(&inv.invitation_id).unwrap().unwrap();
    let by_hash = store
        .invitations_get_by_token_hash(&inv.token_hash)
        .unwrap()
        .unwrap();
    assert_eq!(by_id.email, "bob@example.org");
    assert_eq!(by_id.role, Role::Operator);
    assert_eq!(by_hash.invitation_id, inv.invitation_id);
    assert!(by_id.accepted_at.is_none());
}

#[test]
fn invitations_list_ordered_by_recency() {
    let store = create_memory_store().unwrap();
    let admin = seed_admin(&store);

    let mut older = make_invitation(&admin.user_id, "a@e.org", Role::Viewer);
    older.created_at = utc(2026, 5, 1, 0, 0);
    let mut newer = make_invitation(&admin.user_id, "b@e.org", Role::Viewer);
    newer.created_at = utc(2026, 5, 20, 0, 0);

    store.invitations_create(&older).unwrap();
    store.invitations_create(&newer).unwrap();

    let list = store.invitations_list().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].email, "b@e.org");
    assert_eq!(list[1].email, "a@e.org");
}

#[test]
fn invitations_mark_accepted_persists() {
    let store = create_memory_store().unwrap();
    let admin = seed_admin(&store);
    let inv = make_invitation(&admin.user_id, "x@e.org", Role::Viewer);
    store.invitations_create(&inv).unwrap();

    let when = utc(2026, 5, 23, 14, 0);
    store
        .invitations_mark_accepted(&inv.invitation_id, when)
        .unwrap();

    let loaded = store.invitations_get(&inv.invitation_id).unwrap().unwrap();
    assert_eq!(loaded.accepted_at, Some(when));
}

#[test]
fn invitations_revoke_persists() {
    let store = create_memory_store().unwrap();
    let admin = seed_admin(&store);
    let inv = make_invitation(&admin.user_id, "x@e.org", Role::Viewer);
    store.invitations_create(&inv).unwrap();

    let when = utc(2026, 5, 23, 15, 0);
    store.invitations_revoke(&inv.invitation_id, when).unwrap();

    let loaded = store.invitations_get(&inv.invitation_id).unwrap().unwrap();
    assert_eq!(loaded.revoked_at, Some(when));
    assert!(loaded.accepted_at.is_none());
}

#[test]
fn password_reset_create_and_get_round_trip() {
    let store = create_memory_store().unwrap();
    let user = make_user("alex", Role::Operator);
    store.users_create(&user).unwrap();

    let reset = PasswordReset {
        reset_id: Uuid::new_v4().to_string(),
        user_id: user.user_id.clone(),
        token_hash: "reset-hash-abc".into(),
        expires_at: utc(2026, 5, 24, 0, 0),
        used_at: None,
        created_at: now(),
    };
    store.password_resets_create(&reset).unwrap();

    let loaded = store
        .password_resets_get_by_token_hash("reset-hash-abc")
        .unwrap()
        .unwrap();
    assert_eq!(loaded.reset_id, reset.reset_id);
    assert_eq!(loaded.user_id, user.user_id);
    assert!(loaded.used_at.is_none());
}

#[test]
fn password_resets_mark_used_persists() {
    let store = create_memory_store().unwrap();
    let user = make_user("alex", Role::Operator);
    store.users_create(&user).unwrap();

    let reset = PasswordReset {
        reset_id: Uuid::new_v4().to_string(),
        user_id: user.user_id.clone(),
        token_hash: "reset-hash-xyz".into(),
        expires_at: utc(2026, 5, 24, 0, 0),
        used_at: None,
        created_at: now(),
    };
    store.password_resets_create(&reset).unwrap();

    let when = utc(2026, 5, 23, 16, 0);
    store
        .password_resets_mark_used(&reset.reset_id, when)
        .unwrap();

    let loaded = store
        .password_resets_get_by_token_hash("reset-hash-xyz")
        .unwrap()
        .unwrap();
    assert_eq!(loaded.used_at, Some(when));
}

#[test]
fn password_resets_unknown_token_returns_none() {
    let store = create_memory_store().unwrap();
    assert!(
        store
            .password_resets_get_by_token_hash("nope")
            .unwrap()
            .is_none()
    );
}

// ─── TOTP secrets + recovery codes ───

fn seed_user(store: &impl AuthStore, username: &str) -> User {
    let u = make_user(username, Role::Admin);
    store.users_create(&u).unwrap();
    u
}

#[test]
fn totp_upsert_and_get_round_trip() {
    let store = create_memory_store().unwrap();
    let user = seed_user(&store, "alex");
    let secret = TotpSecret {
        user_id: user.user_id.clone(),
        secret_enc: "fake-wrapped".into(),
        enabled: false,
        confirmed_at: None,
        created_at: now(),
    };
    store.totp_upsert(&secret).unwrap();

    let loaded = store.totp_get(&user.user_id).unwrap().unwrap();
    assert!(!loaded.enabled);
    assert_eq!(loaded.secret_enc, "fake-wrapped");
}

#[test]
fn totp_upsert_is_idempotent() {
    let store = create_memory_store().unwrap();
    let user = seed_user(&store, "alex");
    let mut secret = TotpSecret {
        user_id: user.user_id.clone(),
        secret_enc: "first-wrap".into(),
        enabled: false,
        confirmed_at: None,
        created_at: now(),
    };
    store.totp_upsert(&secret).unwrap();
    // Re-upsert with a different wrapped secret (e.g. user retried setup).
    secret.secret_enc = "second-wrap".into();
    store.totp_upsert(&secret).unwrap();

    let loaded = store.totp_get(&user.user_id).unwrap().unwrap();
    assert_eq!(loaded.secret_enc, "second-wrap");
}

#[test]
fn totp_set_enabled_persists_confirmed_at() {
    let store = create_memory_store().unwrap();
    let user = seed_user(&store, "alex");
    store
        .totp_upsert(&TotpSecret {
            user_id: user.user_id.clone(),
            secret_enc: "wrapped".into(),
            enabled: false,
            confirmed_at: None,
            created_at: now(),
        })
        .unwrap();

    let when = utc(2026, 5, 23, 17, 0);
    store
        .totp_set_enabled(&user.user_id, true, Some(when))
        .unwrap();

    let loaded = store.totp_get(&user.user_id).unwrap().unwrap();
    assert!(loaded.enabled);
    assert_eq!(loaded.confirmed_at, Some(when));
}

#[test]
fn totp_delete_removes_secret_and_recovery_codes() {
    let store = create_memory_store().unwrap();
    let user = seed_user(&store, "alex");
    store
        .totp_upsert(&TotpSecret {
            user_id: user.user_id.clone(),
            secret_enc: "wrapped".into(),
            enabled: true,
            confirmed_at: Some(now()),
            created_at: now(),
        })
        .unwrap();

    let codes: Vec<RecoveryCode> = (0..10)
        .map(|i| RecoveryCode {
            code_id: Uuid::new_v4().to_string(),
            user_id: user.user_id.clone(),
            code_hash: format!("hash-{i}"),
            used_at: None,
            created_at: now(),
        })
        .collect();
    store
        .recovery_codes_replace_all(&user.user_id, &codes)
        .unwrap();
    assert_eq!(
        store.recovery_codes_count_unused(&user.user_id).unwrap(),
        10
    );

    store.totp_delete(&user.user_id).unwrap();

    assert!(store.totp_get(&user.user_id).unwrap().is_none());
    assert_eq!(store.recovery_codes_count_unused(&user.user_id).unwrap(), 0);
}

#[test]
fn recovery_codes_find_unused_skips_used_ones() {
    let store = create_memory_store().unwrap();
    let user = seed_user(&store, "alex");
    let code_id = Uuid::new_v4().to_string();
    let codes = vec![RecoveryCode {
        code_id: code_id.clone(),
        user_id: user.user_id.clone(),
        code_hash: "target-hash".into(),
        used_at: None,
        created_at: now(),
    }];
    store
        .recovery_codes_replace_all(&user.user_id, &codes)
        .unwrap();

    let found = store
        .recovery_codes_find_unused(&user.user_id, "target-hash")
        .unwrap();
    assert!(found.is_some());

    store
        .recovery_codes_mark_used(&code_id, utc(2026, 5, 23, 18, 0))
        .unwrap();

    let after = store
        .recovery_codes_find_unused(&user.user_id, "target-hash")
        .unwrap();
    assert!(after.is_none(), "used code must not match find_unused");
}

#[test]
fn recovery_codes_replace_all_clears_previous_set() {
    let store = create_memory_store().unwrap();
    let user = seed_user(&store, "alex");

    let first: Vec<RecoveryCode> = (0..10)
        .map(|i| RecoveryCode {
            code_id: Uuid::new_v4().to_string(),
            user_id: user.user_id.clone(),
            code_hash: format!("first-{i}"),
            used_at: None,
            created_at: now(),
        })
        .collect();
    store
        .recovery_codes_replace_all(&user.user_id, &first)
        .unwrap();

    let second: Vec<RecoveryCode> = (0..10)
        .map(|i| RecoveryCode {
            code_id: Uuid::new_v4().to_string(),
            user_id: user.user_id.clone(),
            code_hash: format!("second-{i}"),
            used_at: None,
            created_at: now(),
        })
        .collect();
    store
        .recovery_codes_replace_all(&user.user_id, &second)
        .unwrap();

    // None of the first batch matches anymore.
    for i in 0..10 {
        let hash = format!("first-{i}");
        assert!(
            store
                .recovery_codes_find_unused(&user.user_id, &hash)
                .unwrap()
                .is_none()
        );
    }
    assert_eq!(
        store.recovery_codes_count_unused(&user.user_id).unwrap(),
        10
    );
}

// ─── Personal Access Tokens ───

fn make_pat(user_id: &str, name: &str, token_hash: &str) -> PersonalAccessToken {
    PersonalAccessToken {
        token_id: Uuid::new_v4().to_string(),
        user_id: user_id.into(),
        name: name.into(),
        token_hash: token_hash.into(),
        token_prefix: "croniq_pat_".into(),
        scopes: vec!["jobs:read".into()],
        expires_at: None,
        revoked_at: None,
        last_used_at: None,
        created_at: now(),
    }
}

#[test]
fn pat_create_and_find_by_hash() {
    let store = create_memory_store().unwrap();
    let user = seed_user(&store, "alex");
    let pat = make_pat(&user.user_id, "laptop", "hash-a");
    store.pat_create(&pat).unwrap();

    let found = store.pat_find_by_hash("hash-a").unwrap().unwrap();
    assert_eq!(found.token_id, pat.token_id);
    assert_eq!(found.user_id, user.user_id);
    assert_eq!(found.scopes, vec!["jobs:read".to_string()]);
}

#[test]
fn pat_list_orders_by_created_desc() {
    let store = create_memory_store().unwrap();
    let user = seed_user(&store, "alex");

    let mut older = make_pat(&user.user_id, "older", "hash-old");
    older.created_at = utc(2026, 5, 1, 0, 0);
    let mut newer = make_pat(&user.user_id, "newer", "hash-new");
    newer.created_at = utc(2026, 5, 20, 0, 0);
    store.pat_create(&older).unwrap();
    store.pat_create(&newer).unwrap();

    let list = store.pat_list(&user.user_id).unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].name, "newer");
    assert_eq!(list[1].name, "older");
}

#[test]
fn pat_revoke_sets_revoked_at() {
    let store = create_memory_store().unwrap();
    let user = seed_user(&store, "alex");
    let pat = make_pat(&user.user_id, "laptop", "hash-r");
    store.pat_create(&pat).unwrap();

    let when = utc(2026, 5, 23, 19, 0);
    store.pat_revoke(&pat.token_id, when).unwrap();

    // After revoke, find_by_hash still returns the row (auth middleware
    // checks revoked_at separately) — but the timestamp is set.
    let loaded = store.pat_find_by_hash("hash-r").unwrap().unwrap();
    assert_eq!(loaded.revoked_at, Some(when));
}

#[test]
fn pat_touch_last_used_updates_field() {
    let store = create_memory_store().unwrap();
    let user = seed_user(&store, "alex");
    let pat = make_pat(&user.user_id, "laptop", "hash-t");
    store.pat_create(&pat).unwrap();

    let when = utc(2026, 5, 23, 20, 0);
    store.pat_touch_last_used(&pat.token_id, when).unwrap();

    let loaded = store.pat_find_by_hash("hash-t").unwrap().unwrap();
    assert_eq!(loaded.last_used_at, Some(when));
}

#[test]
fn pat_find_by_unknown_hash_returns_none() {
    let store = create_memory_store().unwrap();
    assert!(store.pat_find_by_hash("nope").unwrap().is_none());
}

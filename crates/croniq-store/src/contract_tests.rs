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

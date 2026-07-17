//! Postgres backend integration test (issue #298).
//!
//! Compile-checking the `postgres` feature guarantees the trait impls exist,
//! but the SQL strings are opaque `&str` to the compiler — a reserved-word
//! collision (`window`), a JSONB/TEXT type mismatch, or a wrong `$n` binding
//! only surfaces when the statements actually run against a server. This test
//! drives every method of the seven traits the SQLite backend also implements
//! so those runtime bugs are caught in CI.
//!
//! It is a no-op unless `CRONIQ_TEST_PG_URL` points at a reachable Postgres
//! (e.g. `postgres://croniq:croniq@localhost:5432/croniq`). CI sets it against
//! a service container; a plain `cargo test --features postgres` with no DB
//! simply skips. The whole file is gated on the `postgres` feature so the
//! default build compiles it away.
#![cfg(feature = "postgres")]

use chrono::{DateTime, TimeZone, Utc};
use croniq_store::models::*;
use croniq_store::pg::PgStore;
use croniq_store::traits::*;
use std::collections::HashMap;
use uuid::Uuid;

/// Fixed, whole-second timestamp that round-trips cleanly through Postgres
/// `TIMESTAMPTZ` (microsecond resolution) so equality assertions are stable.
fn ts() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 14, 12, 0, 0).unwrap()
}

/// A short random suffix so the test is re-runnable against a persistent DB
/// without colliding on primary keys / unique constraints.
fn suffix() -> String {
    Uuid::new_v4().simple().to_string()
}

#[test]
fn pg_backend_exercises_all_traits() {
    let Ok(url) = std::env::var("CRONIQ_TEST_PG_URL") else {
        eprintln!("CRONIQ_TEST_PG_URL not set — skipping Postgres integration test");
        return;
    };

    let store = PgStore::connect(&url).expect("connect + migrate");
    let s = suffix();

    auth_api_clients_and_keys(&store, &s);
    let user_id = auth_users(&store, &s);
    auth_password_credentials(&store, &s);
    auth_refresh_tokens(&store, &s, &user_id);
    auth_invitations(&store, &s, &user_id);
    auth_password_resets(&store, &s, &user_id);
    auth_totp_and_recovery(&store, &user_id);
    auth_pats(&store, &s, &user_id);
    auth_oidc(&store, &s, &user_id);
    auth_audit_log(&store, &s);

    job_definitions(&store, &s);
    trigger_definitions(&store, &s);
    calendar_definitions(&store, &s);
    execution_logs(&store);
    execution_retention(&store, &s);
    dsl_adoptions(&store, &s);
    alert_deliveries(&store, &s);
    alert_rule_overrides(&store, &s);
}

fn auth_api_clients_and_keys(store: &PgStore, s: &str) {
    let client_id = format!("client-{s}");
    store
        .create_client(&ApiClient {
            client_id: client_id.clone(),
            name: "CI client".into(),
            scopes: vec!["jobs:read".into(), "jobs:write".into()],
            is_active: true,
            created_at: ts(),
        })
        .unwrap();

    let got = store
        .get_client(&client_id)
        .unwrap()
        .expect("client exists");
    assert_eq!(got.name, "CI client");
    assert_eq!(got.scopes, vec!["jobs:read", "jobs:write"]);
    assert!(got.is_active);
    assert_eq!(got.created_at, ts());
    assert!(
        store
            .list_clients()
            .unwrap()
            .iter()
            .any(|c| c.client_id == client_id)
    );

    let key_hash = format!("hash-{s}");
    store
        .create_api_key(&ApiKey {
            key_id: format!("key-{s}"),
            client_id: client_id.clone(),
            key_hash: key_hash.clone(),
            key_prefix: "croniq_a".into(),
            expires_at: Some(ts()),
            revoked_at: None,
            created_at: ts(),
        })
        .unwrap();

    let key = store
        .find_api_key_by_hash(&key_hash)
        .unwrap()
        .expect("key exists");
    assert_eq!(key.client_id, client_id);
    assert_eq!(key.expires_at, Some(ts()));
    assert_eq!(store.list_api_keys(&client_id).unwrap().len(), 1);

    store.revoke_api_key(&key.key_id, ts()).unwrap();
    assert!(
        store
            .find_api_key_by_hash(&key_hash)
            .unwrap()
            .unwrap()
            .revoked_at
            .is_some()
    );

    // delete_client cascades the api_keys deletion.
    store.delete_client(&client_id).unwrap();
    assert!(store.get_client(&client_id).unwrap().is_none());
    assert!(store.find_api_key_by_hash(&key_hash).unwrap().is_none());
}

fn auth_users(store: &PgStore, s: &str) -> String {
    let user_id = format!("user-{s}");
    let mut user = User {
        user_id: user_id.clone(),
        username: format!("alex-{s}"),
        email: Some(format!("alex-{s}@example.com")),
        display_name: Some("Alex".into()),
        role: Role::Admin,
        is_active: true,
        created_at: ts(),
        updated_at: ts(),
        last_login_at: None,
    };
    store.users_create(&user).unwrap();

    let got = store.users_get_by_id(&user_id).unwrap().expect("user");
    assert_eq!(got.role, Role::Admin);
    assert_eq!(
        got.email.as_deref(),
        Some(&*format!("alex-{s}@example.com"))
    );
    assert_eq!(
        store
            .users_get_by_username(&format!("alex-{s}"))
            .unwrap()
            .unwrap()
            .user_id,
        user_id
    );
    assert!(
        store
            .users_list()
            .unwrap()
            .iter()
            .any(|u| u.user_id == user_id)
    );
    assert!(store.users_count_active_admins().unwrap() >= 1);

    store.users_set_last_login(&user_id, ts()).unwrap();
    assert_eq!(
        store
            .users_get_by_id(&user_id)
            .unwrap()
            .unwrap()
            .last_login_at,
        Some(ts())
    );

    // users_update is the upsert path; flip the role and confirm.
    user.role = Role::Operator;
    store.users_update(&user).unwrap();
    assert_eq!(
        store.users_get_by_id(&user_id).unwrap().unwrap().role,
        Role::Operator
    );
    // Set it back to admin so FK-referencing sub-tests have a stable user.
    user.role = Role::Admin;
    store.users_update(&user).unwrap();

    user_id
}

fn auth_password_credentials(store: &PgStore, s: &str) {
    let cred = PasswordCredential {
        user_id: format!("pwuser-{s}"),
        username: format!("pw-{s}"),
        password_hash: "bcrypt$abc".into(),
        failed_attempts: 2,
        locked_until: Some(ts()),
        created_at: ts(),
    };
    store.upsert_credentials(&cred).unwrap();
    let got = store
        .get_credentials(&format!("pw-{s}"))
        .unwrap()
        .expect("creds");
    assert_eq!(got.failed_attempts, 2);
    assert_eq!(got.locked_until, Some(ts()));

    // Upsert again with a bumped counter.
    let mut cred2 = cred.clone();
    cred2.failed_attempts = 5;
    store.upsert_credentials(&cred2).unwrap();
    assert_eq!(
        store
            .get_credentials(&format!("pw-{s}"))
            .unwrap()
            .unwrap()
            .failed_attempts,
        5
    );
}

fn auth_refresh_tokens(store: &PgStore, s: &str, user_id: &str) {
    let token_hash = format!("rt-{s}");
    store
        .create_refresh_token(&RefreshToken {
            token_hash: token_hash.clone(),
            client_id: format!("rtclient-{s}"),
            user_id: Some(user_id.to_string()),
            expires_at: ts(),
            revoked_at: None,
            created_at: ts(),
        })
        .unwrap();
    assert!(store.validate_refresh_token(&token_hash).unwrap().is_some());
    store.revoke_refresh_token(&token_hash, ts()).unwrap();
    assert!(store.validate_refresh_token(&token_hash).unwrap().is_none());
}

fn auth_invitations(store: &PgStore, s: &str, user_id: &str) {
    let invitation_id = format!("inv-{s}");
    let token_hash = format!("invtok-{s}");
    store
        .invitations_create(&Invitation {
            invitation_id: invitation_id.clone(),
            email: format!("invitee-{s}@example.com"),
            role: Role::Viewer,
            token_hash: token_hash.clone(),
            invited_by: user_id.to_string(),
            expires_at: ts(),
            accepted_at: None,
            revoked_at: None,
            created_at: ts(),
        })
        .unwrap();

    assert_eq!(
        store.invitations_get(&invitation_id).unwrap().unwrap().role,
        Role::Viewer
    );
    assert_eq!(
        store
            .invitations_get_by_token_hash(&token_hash)
            .unwrap()
            .unwrap()
            .invitation_id,
        invitation_id
    );
    assert!(
        store
            .invitations_list()
            .unwrap()
            .iter()
            .any(|i| i.invitation_id == invitation_id)
    );

    store
        .invitations_mark_accepted(&invitation_id, ts())
        .unwrap();
    assert!(
        store
            .invitations_get(&invitation_id)
            .unwrap()
            .unwrap()
            .accepted_at
            .is_some()
    );
    store.invitations_revoke(&invitation_id, ts()).unwrap();
    assert!(
        store
            .invitations_get(&invitation_id)
            .unwrap()
            .unwrap()
            .revoked_at
            .is_some()
    );
}

fn auth_password_resets(store: &PgStore, s: &str, user_id: &str) {
    let reset_id = format!("reset-{s}");
    let token_hash = format!("resettok-{s}");
    store
        .password_resets_create(&PasswordReset {
            reset_id: reset_id.clone(),
            user_id: user_id.to_string(),
            token_hash: token_hash.clone(),
            expires_at: ts(),
            used_at: None,
            created_at: ts(),
        })
        .unwrap();
    let got = store
        .password_resets_get_by_token_hash(&token_hash)
        .unwrap()
        .expect("reset");
    assert_eq!(got.reset_id, reset_id);
    assert!(got.used_at.is_none());
    store.password_resets_mark_used(&reset_id, ts()).unwrap();
    assert!(
        store
            .password_resets_get_by_token_hash(&token_hash)
            .unwrap()
            .unwrap()
            .used_at
            .is_some()
    );
}

fn auth_totp_and_recovery(store: &PgStore, user_id: &str) {
    store
        .totp_upsert(&TotpSecret {
            user_id: user_id.to_string(),
            secret_enc: "enc-blob".into(),
            enabled: false,
            confirmed_at: None,
            created_at: ts(),
        })
        .unwrap();
    assert!(!store.totp_get(user_id).unwrap().unwrap().enabled);
    store.totp_set_enabled(user_id, true, Some(ts())).unwrap();
    let totp = store.totp_get(user_id).unwrap().unwrap();
    assert!(totp.enabled);
    assert_eq!(totp.confirmed_at, Some(ts()));

    let codes: Vec<RecoveryCode> = (0..3)
        .map(|i| RecoveryCode {
            code_id: format!("code-{}-{}", user_id, i),
            user_id: user_id.to_string(),
            code_hash: format!("codehash-{}-{}", user_id, i),
            used_at: None,
            created_at: ts(),
        })
        .collect();
    store.recovery_codes_replace_all(user_id, &codes).unwrap();
    assert_eq!(store.recovery_codes_count_unused(user_id).unwrap(), 3);

    let hit = store
        .recovery_codes_find_unused(user_id, &format!("codehash-{}-1", user_id))
        .unwrap()
        .expect("unused code");
    store.recovery_codes_mark_used(&hit.code_id, ts()).unwrap();
    assert_eq!(store.recovery_codes_count_unused(user_id).unwrap(), 2);
    assert!(
        store
            .recovery_codes_find_unused(user_id, &format!("codehash-{}-1", user_id))
            .unwrap()
            .is_none()
    );

    // replace_all wipes the previous set.
    store.recovery_codes_replace_all(user_id, &[]).unwrap();
    assert_eq!(store.recovery_codes_count_unused(user_id).unwrap(), 0);

    store.totp_delete(user_id).unwrap();
    assert!(store.totp_get(user_id).unwrap().is_none());
}

fn auth_pats(store: &PgStore, s: &str, user_id: &str) {
    let token_hash = format!("pat-{s}");
    let token_id = format!("patid-{s}");
    store
        .pat_create(&PersonalAccessToken {
            token_id: token_id.clone(),
            user_id: user_id.to_string(),
            name: "laptop".into(),
            token_hash: token_hash.clone(),
            token_prefix: "croniq_pat_".into(),
            scopes: vec!["jobs:read".into()],
            expires_at: None,
            revoked_at: None,
            last_used_at: None,
            created_at: ts(),
        })
        .unwrap();

    let pat = store.pat_find_by_hash(&token_hash).unwrap().expect("pat");
    assert_eq!(pat.scopes, vec!["jobs:read"]);
    assert!(
        store
            .pat_list(user_id)
            .unwrap()
            .iter()
            .any(|p| p.token_id == token_id)
    );

    store.pat_touch_last_used(&token_id, ts()).unwrap();
    assert_eq!(
        store
            .pat_find_by_hash(&token_hash)
            .unwrap()
            .unwrap()
            .last_used_at,
        Some(ts())
    );
    store.pat_revoke(&token_id, ts()).unwrap();
    assert!(
        store
            .pat_find_by_hash(&token_hash)
            .unwrap()
            .unwrap()
            .revoked_at
            .is_some()
    );
}

fn auth_oidc(store: &PgStore, s: &str, user_id: &str) {
    let provider = "google";
    let subject = format!("sub-{s}");
    store
        .oidc_link(&OidcIdentity {
            provider: provider.into(),
            subject: subject.clone(),
            user_id: user_id.to_string(),
            email: Some("alex@example.com".into()),
            linked_at: ts(),
            last_login_at: None,
        })
        .unwrap();
    assert_eq!(
        store
            .oidc_get_by_subject(provider, &subject)
            .unwrap()
            .unwrap()
            .user_id,
        user_id
    );
    store
        .oidc_touch_last_login(provider, &subject, ts())
        .unwrap();
    assert_eq!(
        store
            .oidc_get_by_subject(provider, &subject)
            .unwrap()
            .unwrap()
            .last_login_at,
        Some(ts())
    );

    // Pending login: create → take (returns once, then gone).
    let state = format!("state-{s}");
    store
        .oidc_pending_create(&OidcPendingLogin {
            state: state.clone(),
            nonce: "nonce".into(),
            redirect_to: Some("/dash".into()),
            created_at: ts(),
            expires_at: ts(),
        })
        .unwrap();
    let taken = store.oidc_pending_take(&state).unwrap().expect("pending");
    assert_eq!(taken.nonce, "nonce");
    assert!(store.oidc_pending_take(&state).unwrap().is_none());

    // Expired-purge: seed one already-expired row and sweep it.
    let old_state = format!("oldstate-{s}");
    store
        .oidc_pending_create(&OidcPendingLogin {
            state: old_state.clone(),
            nonce: "n".into(),
            redirect_to: None,
            created_at: Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap(),
            expires_at: Utc.with_ymd_and_hms(2020, 1, 1, 0, 10, 0).unwrap(),
        })
        .unwrap();
    assert!(store.oidc_pending_purge_expired(ts()).unwrap() >= 1);
    assert!(store.oidc_pending_take(&old_state).unwrap().is_none());
}

fn auth_audit_log(store: &PgStore, s: &str) {
    let action = format!("job.created.{s}");
    store
        .audit_log(&AuditEvent {
            event_id: format!("evt-{s}"),
            actor_type: "user".into(),
            actor_id: Some("user-1".into()),
            action: action.clone(),
            target_type: "job".into(),
            target_id: Some("billing:invoice".into()),
            diff_json: Some("{}".into()),
            ip_address: Some("127.0.0.1".into()),
            user_agent: Some("ci".into()),
            created_at: ts(),
        })
        .unwrap();
    let listed = store
        .audit_list(&AuditFilter {
            action: Some(action.clone()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].action, action);
    assert_eq!(listed[0].target_id.as_deref(), Some("billing:invoice"));
}

fn job_definitions(store: &PgStore, s: &str) {
    let job_key = format!("etl:sync:{s}");
    let mut meta = HashMap::new();
    meta.insert("team".to_string(), "ops".to_string());
    store
        .create_job_definition(&JobDefinition {
            job_key: job_key.clone(),
            description: Some("sync".into()),
            assigned_runner_id: Some("runner-1".into()),
            is_active: true,
            metadata: meta.clone(),
            created_at: ts(),
            updated_at: ts(),
            timeout: Some("5m".into()),
            max_retries: Some(4),
            dead_letter_enabled: Some(false),
            tags: vec!["env=prod".into()],
        })
        .unwrap();

    let got = store
        .get_job_definition(&job_key)
        .unwrap()
        .expect("job def");
    assert_eq!(got.metadata.get("team").map(String::as_str), Some("ops"));
    assert_eq!(got.max_retries, Some(4));
    assert_eq!(got.dead_letter_enabled, Some(false));
    assert_eq!(got.tags, vec!["env=prod"]);
    assert!(
        store
            .list_job_definitions()
            .unwrap()
            .iter()
            .any(|j| j.job_key == job_key)
    );

    store.delete_job_definition(&job_key).unwrap();
    assert!(store.get_job_definition(&job_key).unwrap().is_none());
}

fn trigger_definitions(store: &PgStore, s: &str) {
    let job_key = format!("job:{s}");
    let trigger_id = format!("trig-{s}");
    // The `window` column is a reserved word in Postgres; a non-None value
    // proves the double-quoting in every statement is correct.
    store
        .create_trigger(&TriggerDefinition {
            trigger_id: trigger_id.clone(),
            job_key: job_key.clone(),
            cron_expression: Some("0 * * * *".into()),
            timezone: Some("UTC".into()),
            calendar: Some("business".into()),
            window: Some("business-hours".into()),
            not_before: Some(ts()),
            not_after: None,
            enabled: true,
            managed_by: "api".into(),
            created_at: ts(),
            updated_at: ts(),
        })
        .unwrap();

    let got = store.get_trigger(&trigger_id).unwrap().expect("trigger");
    assert_eq!(got.window.as_deref(), Some("business-hours"));
    assert_eq!(got.cron_expression.as_deref(), Some("0 * * * *"));
    assert_eq!(store.list_triggers(Some(&job_key)).unwrap().len(), 1);

    // api-managed trigger is updatable → true.
    let mut updated = got.clone();
    updated.enabled = false;
    updated.updated_at = ts();
    assert!(store.update_trigger(&updated).unwrap());
    assert!(!store.get_trigger(&trigger_id).unwrap().unwrap().enabled);

    // A dsl-managed trigger must refuse the update → false.
    let dsl_id = format!("trig-dsl-{s}");
    store
        .create_trigger(&TriggerDefinition {
            trigger_id: dsl_id.clone(),
            job_key: job_key.clone(),
            cron_expression: Some("0 0 * * *".into()),
            timezone: None,
            calendar: None,
            window: None,
            not_before: None,
            not_after: None,
            enabled: true,
            managed_by: "dsl".into(),
            created_at: ts(),
            updated_at: ts(),
        })
        .unwrap();
    let mut dsl = store.get_trigger(&dsl_id).unwrap().unwrap();
    dsl.enabled = false;
    assert!(!store.update_trigger(&dsl).unwrap());

    store.delete_trigger(&trigger_id).unwrap();
    store.delete_trigger(&dsl_id).unwrap();
    assert!(store.get_trigger(&trigger_id).unwrap().is_none());
}

fn calendar_definitions(store: &PgStore, s: &str) {
    let calendar_id = format!("cal-{s}");
    store
        .create_calendar(&CalendarDefinition {
            calendar_id: calendar_id.clone(),
            name: "Holidays".into(),
            timezone: Some("Europe/Berlin".into()),
            rules: "[{\"skip\":\"2026-12-25\"}]".into(),
            managed_by: "api".into(),
            created_at: ts(),
            updated_at: ts(),
        })
        .unwrap();
    let got = store.get_calendar(&calendar_id).unwrap().expect("calendar");
    assert_eq!(got.name, "Holidays");
    assert!(got.rules.contains("2026-12-25"));
    assert!(
        store
            .list_calendars()
            .unwrap()
            .iter()
            .any(|c| c.calendar_id == calendar_id)
    );

    store.delete_calendar(&calendar_id).unwrap();
    assert!(store.get_calendar(&calendar_id).unwrap().is_none());
}

fn execution_logs(store: &PgStore) {
    let execution_id = Uuid::new_v4();
    // Batch of three sharing one timestamp — seq (0,1,2) is the tiebreaker.
    let entries: Vec<ExecutionLogEntry> = (0..3)
        .map(|i| ExecutionLogEntry {
            id: Uuid::new_v4(),
            execution_id,
            timestamp: ts(),
            level: "info".into(),
            message: format!("line {i}"),
            fields: HashMap::new(),
            seq: 0, // ignored — store assigns
        })
        .collect();
    store.append_logs_batch(&entries).unwrap();

    // A single append continues the sequence.
    store
        .append_log(&ExecutionLogEntry {
            id: Uuid::new_v4(),
            execution_id,
            timestamp: ts(),
            level: "warn".into(),
            message: "line 3".into(),
            fields: HashMap::new(),
            seq: 0,
        })
        .unwrap();

    let logs = store.read_logs(execution_id, 100).unwrap();
    assert_eq!(logs.len(), 4);
    assert_eq!(
        logs.iter().map(|l| l.seq).collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
    assert_eq!(logs[0].message, "line 0");
    assert_eq!(logs[3].message, "line 3");
}

/// Seed an execution, optionally driving it to a terminal state at a chosen
/// `completed_at`. Returns the row id.
fn seed_execution(
    store: &PgStore,
    job_key: &str,
    completed: Option<(ExecutionState, DateTime<Utc>)>,
) -> Uuid {
    let id = Uuid::new_v4();
    store
        .create_execution(&Execution {
            id,
            job_key: job_key.to_string(),
            fire_at: ts(),
            scheduled_for: ts(),
            attempt: 1,
            state: ExecutionState::Queued,
            runner_id: None,
            claimed_at: None,
            started_at: None,
            completed_at: None,
            duration_ms: None,
            error: None,
            dead_reason: None,
            idempotency_key: None,
            metadata: HashMap::new(),
            created_at: ts(),
        })
        .unwrap();
    if let Some((state, at)) = completed {
        store
            .complete_execution(id, state, Some(1), None, None, at)
            .unwrap();
    }
    id
}

/// Execution retention (issue #344): age sweep + per-job keep_last, Postgres.
fn execution_retention(store: &PgStore, s: &str) {
    let old_ts = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let recent_ts = Utc.with_ymd_and_hms(2026, 7, 1, 0, 0, 0).unwrap();
    let cutoff = Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap();

    let job = format!("ret-{s}");
    let old = seed_execution(store, &job, Some((ExecutionState::Completed, old_ts)));
    store
        .append_log(&ExecutionLogEntry {
            id: Uuid::new_v4(),
            execution_id: old,
            timestamp: ts(),
            level: "info".into(),
            message: "old".into(),
            fields: HashMap::new(),
            seq: 0,
        })
        .unwrap();
    let recent = seed_execution(store, &job, Some((ExecutionState::Failed, recent_ts)));
    let live = seed_execution(store, &job, None);
    let dead = seed_execution(store, &job, Some((ExecutionState::Dead, old_ts)));

    let deleted = store.prune_executions_older_than(cutoff, 100).unwrap();
    assert_eq!(deleted, 1, "only the old completed execution is pruned");
    assert!(store.get_execution(old).unwrap().is_none());
    assert!(store.get_execution(recent).unwrap().is_some());
    assert!(store.get_execution(live).unwrap().is_some());
    assert!(store.get_execution(dead).unwrap().is_some());
    assert!(store.read_logs(old, 100).unwrap().is_empty());

    // keep_last: newest 1 of 4 completed survives.
    let cap = format!("cap-{s}");
    let mut ids = Vec::new();
    for min in 0..4u32 {
        let at = Utc.with_ymd_and_hms(2026, 7, 1, 0, min, 0).unwrap();
        ids.push((
            seed_execution(store, &cap, Some((ExecutionState::Completed, at))),
            min,
        ));
    }
    let capped = store.prune_executions_keep_last(&cap, 1, 100).unwrap();
    assert_eq!(capped, 3, "keeps the newest, deletes the 3 oldest");
    for (id, min) in &ids {
        assert_eq!(store.get_execution(*id).unwrap().is_some(), *min == 3);
    }
}

fn dsl_adoptions(store: &PgStore, s: &str) {
    let key = format!("cal-{s}");
    store
        .insert_adoption(&DslAdoption {
            resource_type: "calendar".into(),
            resource_key: key.clone(),
            adopted_at: ts(),
            adopted_by: Some("admin".into()),
        })
        .unwrap();
    assert!(store.is_adopted("calendar", &key).unwrap());
    assert!(
        store
            .list_adoptions("calendar")
            .unwrap()
            .iter()
            .any(|a| a.resource_key == key)
    );

    assert!(store.delete_adoption("calendar", &key).unwrap());
    assert!(!store.is_adopted("calendar", &key).unwrap());
    // Deleting a missing adoption returns false.
    assert!(!store.delete_adoption("calendar", &key).unwrap());
}

fn alert_deliveries(store: &PgStore, s: &str) {
    let delivery_id = format!("del-{s}");
    let rule_name = format!("rule-{s}");
    let job_key = format!("job-{s}");
    let delivery = AlertDelivery {
        delivery_id: delivery_id.clone(),
        rule_name: rule_name.clone(),
        channel_name: "slack".into(),
        job_key: job_key.clone(),
        execution_id: Some(Uuid::new_v4().to_string()),
        state: AlertDeliveryState::Delivered,
        error: None,
        fired_at: ts(),
        delivered_at: Some(ts()),
    };
    store.record_alert_delivery(&delivery).unwrap();
    // Idempotent on delivery_id (ON CONFLICT DO NOTHING).
    store.record_alert_delivery(&delivery).unwrap();

    let got = store
        .get_alert_delivery(&delivery_id)
        .unwrap()
        .expect("delivery");
    assert_eq!(got.state, AlertDeliveryState::Delivered);
    assert_eq!(
        store
            .list_alert_deliveries(&AlertDeliveryFilter {
                rule_name: Some(rule_name.clone()),
                ..Default::default()
            })
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        store.last_alert_fire_at(&rule_name, &job_key).unwrap(),
        Some(ts())
    );
}

fn alert_rule_overrides(store: &PgStore, s: &str) {
    let rule_a = format!("rule-a-{s}");
    let rule_b = format!("rule-b-{s}");
    store
        .upsert_alert_rule_override(&AlertRuleOverride {
            rule_name: rule_a.clone(),
            enabled: Some(false),
            snooze_until: Some(ts()),
            throttle_secs: Some(1800),
            note: "flapping".into(),
            set_by_user_id: "admin".into(),
            set_at: ts(),
            expires_at: None,
        })
        .unwrap();
    let got = store
        .get_alert_rule_override(&rule_a)
        .unwrap()
        .expect("override");
    assert_eq!(got.enabled, Some(false));
    assert_eq!(got.throttle_secs, Some(1800));
    assert!(
        store
            .list_alert_rule_overrides()
            .unwrap()
            .iter()
            .any(|o| o.rule_name == rule_a)
    );

    // An already-expired override is swept by delete_expired.
    store
        .upsert_alert_rule_override(&AlertRuleOverride {
            rule_name: rule_b.clone(),
            enabled: None,
            snooze_until: None,
            throttle_secs: None,
            note: "temp".into(),
            set_by_user_id: "admin".into(),
            set_at: Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap(),
            expires_at: Some(Utc.with_ymd_and_hms(2020, 1, 1, 1, 0, 0).unwrap()),
        })
        .unwrap();
    let cleared = store.delete_expired_alert_rule_overrides(ts()).unwrap();
    assert!(cleared.contains(&rule_b));
    assert!(store.get_alert_rule_override(&rule_b).unwrap().is_none());

    // prune removes overrides whose name is not in the valid set. rule_a is
    // kept; a scratch orphan is removed.
    let orphan = format!("rule-orphan-{s}");
    store
        .upsert_alert_rule_override(&AlertRuleOverride {
            rule_name: orphan.clone(),
            enabled: None,
            snooze_until: None,
            throttle_secs: None,
            note: "orphan".into(),
            set_by_user_id: "admin".into(),
            set_at: ts(),
            expires_at: None,
        })
        .unwrap();
    let pruned = store
        .prune_alert_rule_overrides(std::slice::from_ref(&rule_a))
        .unwrap();
    assert!(pruned.contains(&orphan));
    assert!(store.get_alert_rule_override(&orphan).unwrap().is_none());
    assert!(store.get_alert_rule_override(&rule_a).unwrap().is_some());

    store.delete_alert_rule_override(&rule_a).unwrap();
    assert!(store.get_alert_rule_override(&rule_a).unwrap().is_none());
}

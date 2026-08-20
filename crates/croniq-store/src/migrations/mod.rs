//! Embedded SQL migrations for SQLite.
//!
//! Numbers are monotonic; pick the next free slot when opening a PR. If
//! two migration PRs are in flight at once, the second to merge rebases
//! to bump its number above the first. Migrations are idempotent and
//! additive (or use `IF NOT EXISTS` guards) so order shifts don't break
//! existing data, but the embedded list must stay monotonic for
//! deterministic apply order. See AGENTS.md for the full convention.

use rusqlite::Connection;

const MIGRATIONS: &[(&str, &str)] = &[
    ("001_initial", include_str!("001_initial.sql")),
    ("002_auth", include_str!("002_auth.sql")),
    ("003_definitions", include_str!("003_definitions.sql")),
    ("004_job_policy", include_str!("004_job_policy.sql")),
    ("005_perf_indexes", include_str!("005_perf_indexes.sql")),
    (
        "006_calendar_managed_by",
        include_str!("006_calendar_managed_by.sql"),
    ),
    ("007_dsl_adoptions", include_str!("007_dsl_adoptions.sql")),
    ("008_job_tags", include_str!("008_job_tags.sql")),
    (
        "009_backfill_dead_letters",
        include_str!("009_backfill_dead_letters.sql"),
    ),
    (
        "010_execution_log_seq",
        include_str!("010_execution_log_seq.sql"),
    ),
    ("011_users", include_str!("011_users.sql")),
    (
        "012_invitations_and_resets",
        include_str!("012_invitations_and_resets.sql"),
    ),
    (
        "013_totp_and_recovery",
        include_str!("013_totp_and_recovery.sql"),
    ),
    (
        "014_personal_access_tokens",
        include_str!("014_personal_access_tokens.sql"),
    ),
    ("015_oidc", include_str!("015_oidc.sql")),
    ("016_audit_log", include_str!("016_audit_log.sql")),
    (
        "017_alert_deliveries",
        include_str!("017_alert_deliveries.sql"),
    ),
    (
        "018_alert_rule_overrides",
        include_str!("018_alert_rule_overrides.sql"),
    ),
    (
        "019_trigger_idempotency",
        include_str!("019_trigger_idempotency.sql"),
    ),
    ("020_maintenance", include_str!("020_maintenance.sql")),
    (
        "021_execution_retention_indexes",
        include_str!("021_execution_retention_indexes.sql"),
    ),
    ("022_scheduled_for", include_str!("022_scheduled_for.sql")),
    (
        "023_dead_letter_policy",
        include_str!("023_dead_letter_policy.sql"),
    ),
    (
        "024_runner_identities",
        include_str!("024_runner_identities.sql"),
    ),
    (
        "025_token_generation",
        include_str!("025_token_generation.sql"),
    ),
    (
        "026_api_client_managed_by",
        include_str!("026_api_client_managed_by.sql"),
    ),
    (
        "027_dead_letter_execution_index",
        include_str!("027_dead_letter_execution_index.sql"),
    ),
];

/// Run all pending migrations.
pub fn migrate(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _migrations (
            name TEXT PRIMARY KEY,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )?;

    for (name, sql) in MIGRATIONS {
        let applied: bool = conn
            .prepare("SELECT COUNT(*) FROM _migrations WHERE name = ?1")?
            .query_row([name], |row| row.get::<_, i64>(0))
            .map(|c| c > 0)?;

        if !applied {
            conn.execute_batch(sql)?;
            conn.execute("INSERT INTO _migrations (name) VALUES (?1)", [name])?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Apply every migration up to and including `target`, then return the
    /// connection so the caller can seed data and run the remaining
    /// migrations manually.
    fn apply_through(conn: &Connection, target: &str) -> Result<(), rusqlite::Error> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS _migrations (
                name TEXT PRIMARY KEY,
                applied_at TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )?;
        for (name, sql) in MIGRATIONS {
            conn.execute_batch(sql)?;
            conn.execute("INSERT INTO _migrations (name) VALUES (?1)", [*name])?;
            if *name == target {
                return Ok(());
            }
        }
        Ok(())
    }

    #[test]
    fn migration_024_creates_runner_identity_table() {
        let conn = Connection::open_in_memory().unwrap();
        apply_through(&conn, "023_dead_letter_policy").unwrap();

        // Pre-condition: the binding table does not exist yet, so an
        // upgraded deployment starts with every runner_id unclaimed.
        assert!(
            conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'runner_identities'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap()
                == 0
        );

        let (_, sql) = MIGRATIONS
            .iter()
            .find(|(name, _)| *name == "024_runner_identities")
            .unwrap();
        conn.execute_batch(sql).unwrap();

        conn.execute(
            "INSERT INTO runner_identities (runner_id, owner_id, bound_at)
             VALUES ('worker-1', 'client-a', '2026-05-08T00:00:00Z')",
            [],
        )
        .unwrap();

        // runner_id is the primary key: one owner per runner identity.
        assert!(
            conn.execute(
                "INSERT INTO runner_identities (runner_id, owner_id, bound_at)
                 VALUES ('worker-1', 'client-b', '2026-05-08T00:00:01Z')",
                [],
            )
            .is_err()
        );

        let owner: String = conn
            .query_row(
                "SELECT owner_id FROM runner_identities WHERE runner_id = 'worker-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(owner, "client-a");
    }

    #[test]
    fn migration_009_backfills_orphan_dead_executions() {
        let conn = Connection::open_in_memory().unwrap();
        // Apply everything except the backfill migration.
        apply_through(&conn, "008_job_tags").unwrap();

        // Seed two dead executions: one orphan, one already covered.
        conn.execute(
            "INSERT INTO executions (id, job_key, fire_at, attempt, state, completed_at, error, dead_reason, metadata, created_at)
             VALUES (?1, 'a:job', '2026-05-08T00:00:00Z', 3, 'dead', '2026-05-08T00:00:30Z', 'boom', 'exhausted', '{}', '2026-05-08T00:00:00Z')",
            ["00000000-0000-0000-0000-000000000001"],
        ).unwrap();
        conn.execute(
            "INSERT INTO executions (id, job_key, fire_at, attempt, state, completed_at, error, dead_reason, metadata, created_at)
             VALUES (?1, 'b:job', '2026-05-08T00:00:00Z', 1, 'dead', '2026-05-08T00:00:30Z', 'crash', 'exhausted', '{}', '2026-05-08T00:00:00Z')",
            ["00000000-0000-0000-0000-000000000002"],
        ).unwrap();

        // Pre-existing dead-letter for the second execution.
        conn.execute(
            "INSERT INTO dead_letters (id, execution_id, job_key, fire_at, attempt, error, dead_reason, metadata, created_at)
             VALUES (?1, ?2, 'b:job', '2026-05-08T00:00:00Z', 1, 'crash', 'exhausted', '{}', '2026-05-08T00:00:30Z')",
            [
                "11111111-1111-1111-1111-111111111111",
                "00000000-0000-0000-0000-000000000002",
            ],
        ).unwrap();

        // Now apply the backfill migration.
        let (_, sql) = MIGRATIONS
            .iter()
            .find(|(name, _)| *name == "009_backfill_dead_letters")
            .unwrap();
        conn.execute_batch(sql).unwrap();

        // Total dead-letters: 1 pre-existing + 1 backfilled = 2.
        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM dead_letters", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total, 2);

        // The orphan execution now has a matching dead-letter row.
        let backfilled: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM dead_letters WHERE execution_id = ?1",
                ["00000000-0000-0000-0000-000000000001"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(backfilled, 1);

        // No new row was created for the already-covered execution.
        let still_one: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM dead_letters WHERE execution_id = ?1",
                ["00000000-0000-0000-0000-000000000002"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(still_one, 1);

        // Backfilled row carries no expires_at so the purge sweeper leaves
        // it alone — the operator chose to revisit history, not auto-expire.
        let expires: Option<String> = conn
            .query_row(
                "SELECT expires_at FROM dead_letters WHERE execution_id = ?1",
                ["00000000-0000-0000-0000-000000000001"],
                |r| r.get(0),
            )
            .unwrap();
        assert!(expires.is_none());
    }

    #[test]
    fn migration_027_indexes_the_retention_reachability_probe() {
        let conn = Connection::open_in_memory().unwrap();
        apply_through(&conn, "026_api_client_managed_by").unwrap();

        // Pre-condition: `dead_letters` is indexed by job_key and expires_at
        // only, so the `dl.execution_id = e.id` probe of #470 has nothing to
        // stand on.
        let before: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'index' AND name = 'idx_dead_letters_execution_id'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(before, 0);

        let (_, sql) = MIGRATIONS
            .iter()
            .find(|(name, _)| *name == "027_dead_letter_execution_index")
            .unwrap();
        conn.execute_batch(sql).unwrap();

        // Post-condition: the planner actually picks it for the retention
        // predicate. Asserting on the plan rather than on the index's mere
        // existence is the point — an index the query cannot use would satisfy
        // a name check while changing nothing about the scan it was added for.
        let plan: Vec<String> = conn
            .prepare(
                "EXPLAIN QUERY PLAN
                 SELECT e.id FROM executions e
                 WHERE e.completed_at IS NOT NULL
                   AND (e.state <> 'dead'
                        OR NOT EXISTS (SELECT 1 FROM dead_letters dl
                                       WHERE dl.execution_id = e.id))",
            )
            .unwrap()
            .query_map([], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(
            plan.iter()
                .any(|step| step.contains("idx_dead_letters_execution_id")),
            "the probe must use idx_dead_letters_execution_id; plan was: {plan:#?}"
        );
    }

    #[test]
    fn migration_009_is_a_noop_when_no_orphans_exist() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM dead_letters", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn migration_011_backfills_users_from_password_credentials() {
        let conn = Connection::open_in_memory().unwrap();
        // Apply everything up to and including 010 so password_credentials exists
        // but the users table doesn't yet.
        apply_through(&conn, "010_execution_log_seq").unwrap();

        // Seed two existing single-user-deploy rows.
        conn.execute(
            "INSERT INTO password_credentials (user_id, username, password_hash, failed_attempts, created_at)
             VALUES (?1, 'alex', 'bcrypt-hash-1', 0, '2026-01-01T00:00:00+00:00')",
            ["00000000-0000-0000-0000-000000000010"],
        ).unwrap();
        conn.execute(
            "INSERT INTO password_credentials (user_id, username, password_hash, failed_attempts, created_at)
             VALUES (?1, 'andre', 'bcrypt-hash-2', 0, '2026-02-01T00:00:00+00:00')",
            ["00000000-0000-0000-0000-000000000011"],
        ).unwrap();

        // Apply 011 in isolation.
        let (_, sql) = MIGRATIONS
            .iter()
            .find(|(name, _)| *name == "011_users")
            .unwrap();
        conn.execute_batch(sql).unwrap();

        // Both rows landed in users with role=admin.
        let users: Vec<(String, String, String, i64)> = conn
            .prepare("SELECT user_id, username, role, is_active FROM users ORDER BY username")
            .unwrap()
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(users.len(), 2);
        assert_eq!(users[0].1, "alex");
        assert_eq!(users[0].2, "admin");
        assert_eq!(users[0].3, 1);
        assert_eq!(users[1].1, "andre");
        assert_eq!(users[1].2, "admin");
    }

    #[test]
    fn migration_011_is_idempotent_on_reapply() {
        // Simulates a failed first apply that left an inconsistent state — the
        // second run must not duplicate the backfilled rows. INSERT OR IGNORE
        // on user_id PK makes this safe.
        let conn = Connection::open_in_memory().unwrap();
        apply_through(&conn, "010_execution_log_seq").unwrap();

        conn.execute(
            "INSERT INTO password_credentials (user_id, username, password_hash, failed_attempts, created_at)
             VALUES (?1, 'alex', 'bcrypt-hash', 0, '2026-01-01T00:00:00+00:00')",
            ["00000000-0000-0000-0000-000000000020"],
        ).unwrap();

        let (_, sql) = MIGRATIONS
            .iter()
            .find(|(name, _)| *name == "011_users")
            .unwrap();
        conn.execute_batch(sql).unwrap();
        // Replaying the backfill is safe — no duplicate row.
        conn.execute(
            "INSERT OR IGNORE INTO users (user_id, username, role, is_active, created_at, updated_at)
             SELECT user_id, username, 'admin', 1, created_at, created_at FROM password_credentials",
            [],
        ).unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM users WHERE username = 'alex'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn migration_011_handles_empty_password_credentials() {
        // Fresh deploy: no existing users, table is empty after migration.
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn migration_019_adds_idempotency_key_column_and_index() {
        let conn = Connection::open_in_memory().unwrap();
        // Apply everything up to (and including) the previous migration so
        // the executions table exists in its pre-019 shape.
        apply_through(&conn, "018_alert_rule_overrides").unwrap();

        // Seed a pre-migration execution row (no idempotency_key column yet).
        conn.execute(
            "INSERT INTO executions (id, job_key, fire_at, attempt, state, metadata, created_at)
             VALUES (?1, 'billing:invoice', '2026-07-01T00:00:00Z', 1, 'completed', '{}', '2026-07-01T00:00:00Z')",
            ["00000000-0000-0000-0000-000000000001"],
        )
        .unwrap();

        // Apply 019 in isolation.
        let (_, sql) = MIGRATIONS
            .iter()
            .find(|(name, _)| *name == "019_trigger_idempotency")
            .unwrap();
        conn.execute_batch(sql).unwrap();

        // The pre-existing row reads back with a NULL idempotency_key.
        let key: Option<String> = conn
            .query_row(
                "SELECT idempotency_key FROM executions WHERE id = ?1",
                ["00000000-0000-0000-0000-000000000001"],
                |r| r.get(0),
            )
            .unwrap();
        assert!(key.is_none(), "pre-migration rows must default to NULL");

        // New rows can persist and read back a key.
        conn.execute(
            "INSERT INTO executions (id, job_key, fire_at, attempt, state, metadata, created_at, idempotency_key)
             VALUES (?1, 'billing:invoice', '2026-07-01T01:00:00Z', 1, 'queued', '{}', '2026-07-01T01:00:00Z', 'evt-42')",
            ["00000000-0000-0000-0000-000000000002"],
        )
        .unwrap();
        let key: Option<String> = conn
            .query_row(
                "SELECT idempotency_key FROM executions WHERE id = ?1",
                ["00000000-0000-0000-0000-000000000002"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(key.as_deref(), Some("evt-42"));

        // The partial dedup index exists.
        let idx: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'index' AND name = 'idx_executions_job_key_idempotency_key'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(idx, 1, "dedup index must be created");
    }

    #[test]
    fn migration_022_backfills_scheduled_for_from_fire_at() {
        let conn = Connection::open_in_memory().unwrap();
        // Apply everything up to (and including) the previous migration so the
        // executions / dead_letters tables exist in their pre-022 shape.
        apply_through(&conn, "021_execution_retention_indexes").unwrap();

        // Seed a pre-migration execution and dead-letter (no scheduled_for yet).
        conn.execute(
            "INSERT INTO executions (id, job_key, fire_at, attempt, state, metadata, created_at)
             VALUES (?1, 'billing:report', '2026-06-01T06:00:00Z', 2, 'dead', '{}', '2026-06-08T00:00:00Z')",
            ["00000000-0000-0000-0000-000000000001"],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO dead_letters (id, execution_id, job_key, fire_at, attempt, error, dead_reason, metadata, created_at)
             VALUES (?1, ?2, 'billing:report', '2026-06-01T06:00:00Z', 2, 'boom', 'exhausted', '{}', '2026-06-08T00:00:30Z')",
            [
                "11111111-1111-1111-1111-111111111111",
                "00000000-0000-0000-0000-000000000001",
            ],
        )
        .unwrap();

        // Apply 022 in isolation.
        let (_, sql) = MIGRATIONS
            .iter()
            .find(|(name, _)| *name == "022_scheduled_for")
            .unwrap();
        conn.execute_batch(sql).unwrap();

        // Backfill copies fire_at into the new column for both tables.
        let exec_sched: String = conn
            .query_row(
                "SELECT scheduled_for FROM executions WHERE id = ?1",
                ["00000000-0000-0000-0000-000000000001"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(exec_sched, "2026-06-01T06:00:00Z");
        let dl_sched: String = conn
            .query_row(
                "SELECT scheduled_for FROM dead_letters WHERE id = ?1",
                ["11111111-1111-1111-1111-111111111111"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(dl_sched, "2026-06-01T06:00:00Z");

        // New rows can persist a scheduled_for distinct from fire_at (the
        // retry/replay case where fire_at has drifted but the logical time
        // stays pinned to the original schedule).
        conn.execute(
            "INSERT INTO executions (id, job_key, fire_at, scheduled_for, attempt, state, metadata, created_at)
             VALUES (?1, 'billing:report', '2026-06-08T00:05:00Z', '2026-06-01T06:00:00Z', 3, 'queued', '{}', '2026-06-08T00:05:00Z')",
            ["00000000-0000-0000-0000-000000000002"],
        )
        .unwrap();
        let (fire, sched): (String, String) = conn
            .query_row(
                "SELECT fire_at, scheduled_for FROM executions WHERE id = ?1",
                ["00000000-0000-0000-0000-000000000002"],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(fire, "2026-06-08T00:05:00Z");
        assert_eq!(sched, "2026-06-01T06:00:00Z");
    }

    #[test]
    fn migration_018_creates_alert_rule_overrides_table() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();

        // Table exists and accepts a row keyed by rule_name.
        conn.execute(
            "INSERT INTO alert_rule_overrides
                (rule_name, enabled, snooze_until, throttle_secs, note, set_by_user_id, set_at, expires_at)
             VALUES ('billing-fail', 0, NULL, 1800, 'flapping', 'user-1', '2026-06-04T00:00:00Z', NULL)",
            [],
        )
        .unwrap();

        // PRIMARY KEY on rule_name rejects a duplicate plain INSERT.
        let dup = conn.execute(
            "INSERT INTO alert_rule_overrides
                (rule_name, note, set_by_user_id, set_at)
             VALUES ('billing-fail', 'again', 'user-1', '2026-06-04T01:00:00Z')",
            [],
        );
        assert!(dup.is_err(), "rule_name must be a primary key");

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM alert_rule_overrides", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn migration_026_defaults_existing_clients_to_api_ownership() {
        // An upgrade must not hand existing clients to the environment:
        // ownership decides whether the dashboard may still edit them, and a
        // row that silently flips would start refusing edits with no cause an
        // operator could see.
        let conn = Connection::open_in_memory().unwrap();
        apply_through(&conn, "025_token_generation").unwrap();

        conn.execute(
            "INSERT INTO api_clients (client_id, name, scopes, is_active, created_at)
             VALUES ('c-1', 'default', '[\"admin\"]', 1, '2026-01-01T00:00:00+00:00')",
            [],
        )
        .unwrap();

        assert!(
            conn.query_row("SELECT managed_by FROM api_clients", [], |r| r
                .get::<_, String>(0))
                .is_err(),
            "managed_by must not exist before 026, or this proves nothing"
        );

        let (_, sql) = MIGRATIONS
            .iter()
            .find(|(name, _)| *name == "026_api_client_managed_by")
            .unwrap();
        conn.execute_batch(sql).unwrap();

        let owner: String = conn
            .query_row(
                "SELECT managed_by FROM api_clients WHERE client_id = 'c-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(owner, "api");

        // And the column carries its default for rows that do not name it.
        conn.execute(
            "INSERT INTO api_clients (client_id, name, scopes, is_active, created_at)
             VALUES ('c-2', 'other', '[]', 1, '2026-02-01T00:00:00+00:00')",
            [],
        )
        .unwrap();
        let owner: String = conn
            .query_row(
                "SELECT managed_by FROM api_clients WHERE client_id = 'c-2'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(owner, "api");
    }

    #[test]
    fn migration_025_adds_token_generation_starting_at_zero() {
        // Seed-then-apply: bootstrap the schema up to the migration before this
        // one, insert a user the way an existing deployment would have it, then
        // apply 025 in isolation and assert the post-condition.
        let conn = Connection::open_in_memory().unwrap();
        apply_through(&conn, "024_runner_identities").unwrap();

        conn.execute(
            "INSERT INTO users (user_id, username, role, is_active, created_at, updated_at)
             VALUES (?1, 'alex', 'admin', 1, '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00')",
            ["00000000-0000-0000-0000-000000000030"],
        )
        .unwrap();

        // The column must not exist yet, or the migration is a no-op and this
        // test would pass without proving anything.
        assert!(
            conn.query_row("SELECT token_generation FROM users", [], |r| r
                .get::<_, i64>(0))
                .is_err(),
            "token_generation must not exist before 025"
        );

        let (_, sql) = MIGRATIONS
            .iter()
            .find(|(name, _)| *name == "025_token_generation")
            .unwrap();
        conn.execute_batch(sql).unwrap();

        // Existing rows are backfilled to generation 0, which is what a token
        // minted before the upgrade (carrying no claim) reads as — so a rolling
        // restart does not sign every user out.
        let generation: i64 = conn
            .query_row(
                "SELECT token_generation FROM users WHERE username = 'alex'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(generation, 0);

        // New rows get the default without naming the column.
        conn.execute(
            "INSERT INTO users (user_id, username, role, is_active, created_at, updated_at)
             VALUES (?1, 'blake', 'viewer', 1, '2026-02-01T00:00:00+00:00', '2026-02-01T00:00:00+00:00')",
            ["00000000-0000-0000-0000-000000000031"],
        )
        .unwrap();
        let generation: i64 = conn
            .query_row(
                "SELECT token_generation FROM users WHERE username = 'blake'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(generation, 0);

        // And it is a plain counter the bump can increment.
        conn.execute(
            "UPDATE users SET token_generation = token_generation + 1 WHERE username = 'alex'",
            [],
        )
        .unwrap();
        let generation: i64 = conn
            .query_row(
                "SELECT token_generation FROM users WHERE username = 'alex'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(generation, 1);
    }
}

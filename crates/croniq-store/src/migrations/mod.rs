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
}

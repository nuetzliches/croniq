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
}

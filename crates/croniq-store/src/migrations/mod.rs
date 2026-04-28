//! Embedded SQL migrations for SQLite.

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

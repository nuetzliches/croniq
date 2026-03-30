//! CLI commands that read from a croniq SQLite database directly.

use std::path::Path;

use croniq_store::{
    models::DeadLetterFilter,
    sqlite::SqliteStore,
    traits::DeadLetterStore,
};
use miette::{IntoDiagnostic, Result, miette};

// ─── dead-letters ─────────────────────────────────────────────────────────────

/// `croniq dead-letters` — list dead-lettered executions from the store.
pub fn dead_letters(
    data_dir: &Path,
    job_key: Option<&str>,
    limit: u32,
) -> Result<()> {
    let db_path = data_dir.join("croniq.db");
    let store = SqliteStore::open(&db_path)
        .map_err(|e| miette!("Could not open store at {}: {e}", db_path.display()))?;

    let filter = DeadLetterFilter {
        job_key: job_key.map(str::to_string),
        limit: Some(limit),
    };

    let letters = store.list_dead_letters(&filter).into_diagnostic()?;

    if letters.is_empty() {
        if let Some(key) = job_key {
            println!("No dead letters for job '{key}'.");
        } else {
            println!("Dead letter queue is empty.");
        }
        return Ok(());
    }

    println!(
        "{:<38} {:<32} {:>7} {}",
        "DEAD LETTER ID", "JOB KEY", "ATTEMPT", "ERROR"
    );
    println!("{}", "-".repeat(100));

    for dl in &letters {
        let error_preview: String = dl.error.chars().take(40).collect();
        let error_display = if dl.error.len() > 40 {
            format!("{error_preview}…")
        } else {
            error_preview
        };
        println!(
            "{:<38} {:<32} {:>7} {}",
            dl.id, dl.job_key, dl.attempt, error_display
        );
    }

    println!();
    println!("Total: {} dead letter(s)", letters.len());

    if let Some(first) = letters.first() {
        println!(
            "\nRun `croniq dead-letters inspect {}` for full details.",
            first.id
        );
    }

    Ok(())
}

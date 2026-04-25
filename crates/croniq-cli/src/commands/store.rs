//! CLI commands that read from a croniq SQLite database directly.

use std::path::Path;

use croniq_store::{models::DeadLetterFilter, sqlite::SqliteStore, traits::DeadLetterStore};
use miette::{IntoDiagnostic, Result, miette};
use uuid::Uuid;

// ─── dead-letters ─────────────────────────────────────────────────────────────

/// `croniq dead-letters` — list dead-lettered executions from the store.
pub fn dead_letters(data_dir: &Path, job_key: Option<&str>, limit: u32) -> Result<()> {
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
        "{:<38} {:<32} {:>7} ERROR",
        "DEAD LETTER ID", "JOB KEY", "ATTEMPT"
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
            "\nRun `croniq dead-letters-inspect {} --data-dir <path>` for full details.",
            first.id
        );
    }

    Ok(())
}

/// `croniq dead-letters-inspect <id>` — show full details of a dead letter.
pub fn dead_letters_inspect(data_dir: &Path, id: &str) -> Result<()> {
    let db_path = data_dir.join("croniq.db");
    let store = SqliteStore::open(&db_path)
        .map_err(|e| miette!("Could not open store at {}: {e}", db_path.display()))?;

    let uuid = Uuid::parse_str(id).map_err(|e| miette!("Invalid UUID '{id}': {e}"))?;

    let dl = store
        .get_dead_letter(uuid)
        .into_diagnostic()?
        .ok_or_else(|| miette!("Dead letter '{id}' not found."))?;

    println!("Dead Letter Details");
    println!("{}", "=".repeat(60));
    println!("ID:            {}", dl.id);
    println!("Execution ID:  {}", dl.execution_id);
    println!("Job Key:       {}", dl.job_key);
    println!("Fire At:       {}", dl.fire_at);
    println!("Attempt:       {}", dl.attempt);
    println!("Dead Reason:   {}", dl.dead_reason);
    println!("Created At:    {}", dl.created_at);
    if let Some(expires) = dl.expires_at {
        println!("Expires At:    {}", expires);
    }
    println!();
    println!("Error:");
    println!("{}", dl.error);

    if !dl.metadata.is_empty() {
        println!();
        println!("Metadata:");
        for (k, v) in &dl.metadata {
            println!("  {k}: {v}");
        }
    }

    Ok(())
}

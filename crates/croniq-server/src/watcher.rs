//! Filesystem watcher: monitors the Croniqfile for changes and signals reload.
//!
//! Uses `notify` to watch the config file. On modification, sends a signal
//! through a tokio channel so the scheduler task can reload the configuration.

use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::{Event, EventKind, RecursiveMode, Watcher};
use tokio::sync::mpsc;

/// Start watching a file for changes. Returns a receiver that emits the file
/// path whenever a modification is detected (debounced to 500ms).
pub fn watch_config(
    config_path: &Path,
) -> Result<mpsc::UnboundedReceiver<PathBuf>, notify::Error> {
    let (tx, rx) = mpsc::unbounded_channel();
    let canonical = config_path
        .canonicalize()
        .unwrap_or_else(|_| config_path.to_path_buf());
    let watched = canonical.clone();

    let mut watcher = notify::recommended_watcher(move |res: Result<Event, _>| {
        if let Ok(event) = res {
            if matches!(
                event.kind,
                EventKind::Modify(_) | EventKind::Create(_)
            ) {
                let _ = tx.send(watched.clone());
            }
        }
    })?;

    // Watch the parent directory to handle editor save-and-rename patterns
    let watch_dir = canonical.parent().unwrap_or(Path::new("."));
    watcher.watch(watch_dir, RecursiveMode::NonRecursive)?;

    // Keep the watcher alive by leaking it — it runs for the process lifetime
    std::mem::forget(watcher);

    Ok(rx)
}

/// Debounced wrapper: consumes rapid events and yields at most one signal per
/// `debounce` interval.
pub async fn debounced_reload_loop(
    mut rx: mpsc::UnboundedReceiver<PathBuf>,
    debounce: Duration,
    reload_tx: mpsc::UnboundedSender<PathBuf>,
) {
    loop {
        // Wait for the first event
        let Some(path) = rx.recv().await else {
            break;
        };

        // Drain any events that arrive within the debounce window
        tokio::time::sleep(debounce).await;
        while rx.try_recv().is_ok() {}

        // Signal a reload
        if reload_tx.send(path).is_err() {
            break;
        }
    }
}

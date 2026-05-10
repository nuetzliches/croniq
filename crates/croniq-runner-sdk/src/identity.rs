//! Resolve and persist a stable runner identity across container recreates.
//!
//! Runner instances need a stable `runner_id` so that historical execution
//! rows in the server database remain attributable to the same logical
//! runner after a `docker compose up -d --force-recreate`. Without this,
//! Docker's default hostname (the container short-ID) makes every recreate
//! look like a brand-new runner and the Runner Detail Sheet's history
//! disappears (see GitHub issue #103).
//!
//! Resolution order:
//!
//! 1. `RUNNER_ID` env var — explicit operator override, used as-is.
//! 2. State file `${CRONIQ_RUNNER_DATA_DIR:-/var/lib/croniq-runner}/runner-id`
//!    — read if it exists.
//! 3. Generate `{prefix}-{8-hex-uuid}` and persist it to the state file
//!    so subsequent starts pick it up.
//!
//! If step 3 cannot write the file (e.g. no volume mounted), the runner
//! falls back to a hostname-derived volatile ID and logs a warning. The
//! runner still starts; only the cross-recreate stability is lost.

use std::path::Path;

const DEFAULT_DATA_DIR: &str = "/var/lib/croniq-runner";
const STATE_FILE_NAME: &str = "runner-id";

/// Resolve a stable `runner_id` for this runner instance.
///
/// `prefix` is used when generating a fresh identity, e.g. `"shell-runner"`
/// produces `shell-runner-a1b2c3d4`.
pub fn resolve_runner_id(prefix: &str) -> String {
    if let Some(id) = read_env_override() {
        return id;
    }

    let data_dir = std::env::var("CRONIQ_RUNNER_DATA_DIR")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_DATA_DIR.to_string());

    resolve_or_persist(prefix, Path::new(&data_dir))
}

fn read_env_override() -> Option<String> {
    std::env::var("RUNNER_ID")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Read the existing identity from `data_dir/runner-id` or generate a fresh
/// one and persist it. Falls back to a volatile hostname-based ID on I/O
/// errors so the runner can still start.
fn resolve_or_persist(prefix: &str, data_dir: &Path) -> String {
    let state_path = data_dir.join(STATE_FILE_NAME);

    if let Ok(contents) = std::fs::read_to_string(&state_path) {
        let id = contents.trim();
        if !id.is_empty() {
            return id.to_string();
        }
    }

    let new_id = generate_id(prefix);

    match persist(&state_path, &new_id) {
        Ok(()) => {
            tracing::info!(
                path = %state_path.display(),
                runner_id = %new_id,
                "generated new runner identity and persisted to state file",
            );
            new_id
        }
        Err(e) => {
            let fallback = volatile_fallback(prefix);
            tracing::warn!(
                path = %state_path.display(),
                error = %e,
                runner_id = %fallback,
                "could not persist runner identity — falling back to volatile ID. \
                 Mount a writable volume at this path to make runner_id stable \
                 across container recreates.",
            );
            fallback
        }
    }
}

fn generate_id(prefix: &str) -> String {
    let uuid = uuid::Uuid::new_v4().simple().to_string();
    format!("{prefix}-{}", &uuid[..8])
}

fn persist(path: &Path, id: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, id)
}

fn volatile_fallback(prefix: &str) -> String {
    let suffix = std::env::var("HOSTNAME")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let uuid = uuid::Uuid::new_v4().simple().to_string();
            uuid[..8].to_string()
        });
    format!("{prefix}-{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn generate_id_uses_prefix_and_short_uuid() {
        let id = generate_id("shell-runner");
        let prefix_part = "shell-runner-";
        assert!(id.starts_with(prefix_part), "expected prefix in {id}");
        let suffix = &id[prefix_part.len()..];
        assert_eq!(suffix.len(), 8, "expected 8-char suffix in {id}");
        assert!(suffix.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn persists_generated_id_when_dir_is_writable() {
        let dir = TempDir::new().unwrap();
        let id1 = resolve_or_persist("shell-runner", dir.path());
        let id2 = resolve_or_persist("shell-runner", dir.path());
        assert_eq!(id1, id2, "second resolve should read the persisted file");
        assert!(id1.starts_with("shell-runner-"));

        let on_disk = std::fs::read_to_string(dir.path().join(STATE_FILE_NAME)).unwrap();
        assert_eq!(on_disk.trim(), id1);
    }

    #[test]
    fn reads_existing_state_file_verbatim() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join(STATE_FILE_NAME), "shell-runner-vps-prod").unwrap();

        let id = resolve_or_persist("shell-runner", dir.path());
        assert_eq!(id, "shell-runner-vps-prod");
    }

    #[test]
    fn ignores_empty_state_file_and_regenerates() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join(STATE_FILE_NAME), "  \n").unwrap();

        let id = resolve_or_persist("shell-runner", dir.path());
        assert!(id.starts_with("shell-runner-"));
        assert_ne!(id.trim(), "");
    }

    #[test]
    fn falls_back_to_volatile_id_when_dir_not_writable() {
        // /proc is a read-only pseudo-filesystem on Linux; create_dir_all under
        // it fails. On platforms where this is writable the test would still
        // pass — we just don't exercise the fallback branch.
        let unwritable = Path::new("/proc/croniq-runner-test-unwritable");
        let id = resolve_or_persist("shell-runner", unwritable);
        assert!(id.starts_with("shell-runner-"));
    }
}

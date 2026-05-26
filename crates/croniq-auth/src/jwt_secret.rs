//! Locate or initialize the JWT secret used for token signing and TOTP
//! at-rest encryption.
//!
//! Resolution order:
//!   1. `CRONIQ_JWT_SECRET` environment variable
//!   2. `<data_dir>/jwt.secret` (auto-created on first call if missing,
//!      mode 0600 on Unix)
//!
//! The server additionally honours a Croniqfile `pull_api.auth`
//! override, which only exists at server-config-load time and is not
//! reachable from pre-server tools (CLI init). As long as `pull_api.auth`
//! is not set, CLI-side encryption and server-side decryption agree.

use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum JwtSecretError {
    #[error("failed to access JWT secret file: {0}")]
    Io(#[from] std::io::Error),
}

/// Returns the JWT secret for the given data directory, creating
/// `<data_dir>/jwt.secret` if neither `CRONIQ_JWT_SECRET` nor the file
/// already provide one. Safe to call repeatedly; only the first call
/// writes the file.
pub fn ensure(data_dir: &Path) -> Result<String, JwtSecretError> {
    if let Ok(s) = std::env::var("CRONIQ_JWT_SECRET") {
        let s = s.trim().to_string();
        if !s.is_empty() {
            return Ok(s);
        }
    }

    let secret_path = data_dir.join("jwt.secret");
    if let Ok(s) = std::fs::read_to_string(&secret_path) {
        let s = s.trim().to_string();
        if !s.is_empty() {
            return Ok(s);
        }
    }

    std::fs::create_dir_all(data_dir)?;
    let secret = uuid::Uuid::new_v4().to_string();
    write_secret_file(&secret_path, &secret)?;
    Ok(secret)
}

/// Write `content` to `path` with mode 0600 on Unix (world-unreadable).
/// Same write semantics as the server's inline writer in `main.rs`;
/// duplicated here so the CLI doesn't pull in `croniq-server`.
fn write_secret_file(path: &Path, content: &str) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        f.write_all(content.as_bytes())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("croniq-jwt-secret-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn generates_then_returns_stable_value() {
        // The env-var path is exercised implicitly by the server's
        // existing JWT-secret logic; here we cover the new
        // file-create-or-read fallback that the CLI relies on. Env-var
        // mutation tests would need process-wide serialization to avoid
        // racing with sibling tests on `CRONIQ_JWT_SECRET`.
        let tmp = tempdir();
        let s1 = ensure(&tmp).unwrap();
        assert!(!s1.is_empty(), "generated secret must be non-empty");
        assert!(
            tmp.join("jwt.secret").exists(),
            "first call must persist the secret"
        );

        let s2 = ensure(&tmp).unwrap();
        assert_eq!(s1, s2, "second call must read the same value back");
    }
}

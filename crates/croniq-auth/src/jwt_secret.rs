//! Locate or initialize the JWT secret used for token signing and TOTP
//! at-rest encryption.
//!
//! Resolution order:
//!   1. `CRONIQ_JWT_SECRET` environment variable
//!   2. `<data_dir>/jwt.secret` (auto-created on first call if missing,
//!      mode 0600 on Unix)
//!
//! Both the server and pre-server tools (CLI init) resolve the secret
//! through this function, so CLI-side TOTP encryption and server-side
//! decryption always agree on the same value.

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

/// Write `content` to `path` restricted to the current user: mode 0600 on
/// Unix, a single-ACE DACL on Windows (see [`restrict_to_current_user`]).
/// Same write semantics as the server's inline writer in `main.rs`;
/// duplicated here so the CLI doesn't pull in `croniq-server`.
///
/// On Windows the file is created empty, restricted, and only then filled, so
/// the secret never exists on disk under the directory's inherited ACL.
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
    #[cfg(windows)]
    {
        std::fs::write(path, b"")?;
        restrict_to_current_user(path)?;
        std::fs::write(path, content)
    }
    #[cfg(not(any(unix, windows)))]
    {
        std::fs::write(path, content)
    }
}

/// Replace `path`'s DACL with a single ACE granting the current user full
/// control, and stop inheriting from the parent directory.
///
/// Windows has no mode bits, so before #431 the JWT secret simply inherited
/// whatever the data directory allowed — typically `Users:(RX)` under
/// `C:\ProgramData`, i.e. readable by every local account. That file both
/// signs every token and derives the TOTP at-rest key, so it is the one file
/// in the tree that most deserves restriction.
///
/// Implemented by shelling out to `icacls` rather than pulling in a Win32
/// crate: setting a DACL through `SetNamedSecurityInfoW` would mean several
/// blocks of `unsafe`, and this runs once, at first boot, on the
/// secret-creation path only. `icacls` has shipped in every supported Windows
/// release. Its stdout is localised, so only the exit status is inspected.
///
/// Failure is fatal by design. Writing the signing key with a permissive ACL
/// and continuing is precisely the bug being closed; an operator who cannot
/// run `icacls` can supply `CRONIQ_JWT_SECRET` instead and never reach here.
#[cfg(windows)]
pub fn restrict_to_current_user(path: &Path) -> std::io::Result<()> {
    let principal = current_user_principal()?;
    let status = std::process::Command::new("icacls")
        .arg(path)
        .arg("/inheritance:r")
        .arg("/grant:r")
        .arg(format!("{principal}:F"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    if !status.success() {
        return Err(std::io::Error::other(format!(
            "icacls could not restrict {} to {principal} (exit {:?}); \
             the file would otherwise keep the directory's inherited permissions",
            path.display(),
            status.code()
        )));
    }
    Ok(())
}

/// The `icacls` principal for the current user: the SID (prefixed `*`, which
/// is how `icacls` is told to read a literal SID) when `whoami /user` can be
/// parsed, otherwise `DOMAIN\user` from the environment.
///
/// The SID is preferred because it is locale- and rename-independent —
/// `icacls` account names go through the localised name resolver, and this
/// code has to work on a German or Japanese Windows too. `whoami`'s CSV
/// output is not localised in its field order, so parsing it is safe.
#[cfg(windows)]
fn current_user_principal() -> std::io::Result<String> {
    if let Ok(out) = std::process::Command::new("whoami")
        .args(["/user", "/fo", "csv", "/nh"])
        .output()
        && out.status.success()
        && let Ok(text) = String::from_utf8(out.stdout)
        && let Some(sid) = text
            .split(',')
            .nth(1)
            .map(|f| f.trim().trim_matches('"'))
            .filter(|s| s.starts_with("S-1-"))
    {
        return Ok(format!("*{sid}"));
    }

    let user = std::env::var("USERNAME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            std::io::Error::other(
                "could not determine the current Windows user: `whoami /user` failed \
                 and USERNAME is unset, so the JWT secret's ACL cannot be set",
            )
        })?;
    match std::env::var("USERDOMAIN") {
        Ok(domain) if !domain.trim().is_empty() => Ok(format!("{domain}\\{user}")),
        _ => Ok(user),
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

    /// The Windows half of the "0600 on Unix" guarantee (issue #431). Reads
    /// the DACL back through `icacls` and asserts it collapsed to a single
    /// non-inherited ACE. Assertions are structural, never on `icacls`'
    /// localised wording — this must pass on a German or Japanese Windows.
    #[cfg(windows)]
    #[test]
    fn windows_secret_is_restricted_to_a_single_ace() {
        let tmp = tempdir();
        let path = tmp.join("jwt.secret");
        write_secret_file(&path, "top-secret").expect("write ok");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "top-secret");

        let out = std::process::Command::new("icacls")
            .arg(&path)
            .output()
            .expect("icacls runs");
        assert!(out.status.success(), "icacls failed: {:?}", out.status);
        let text = String::from_utf8_lossy(&out.stdout);

        // `icacls FILE` prints one ACE per line — the first prefixed with the
        // path — then a blank line, then a localised summary.
        let aces: Vec<&str> = text
            .lines()
            .take_while(|l| !l.trim().is_empty())
            .map(str::trim)
            .collect();
        assert_eq!(
            aces.len(),
            1,
            "the secret must carry exactly one ACE; got:\n{text}"
        );

        let ace = aces[0]
            .strip_prefix(path.display().to_string().as_str())
            .expect("first ACE line starts with the path")
            .trim();
        assert!(
            ace.ends_with(":(F)"),
            "the single ACE must grant full control to one principal; got {ace:?}"
        );
        // `(I)` marks an inherited ACE. None may survive `/inheritance:r`,
        // which is what stops the data directory's default `Users:(RX)` from
        // applying to the signing key.
        assert!(
            !text.contains("(I)"),
            "no inherited ACE may remain:\n{text}"
        );
    }
}

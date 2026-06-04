//! Read secret-bearing env vars with the Docker/Compose/K8s `<VAR>_FILE`
//! convention.
//!
//! Secret managers (Docker/Swarm secrets, Kubernetes mounted Secret
//! volumes, Infisical, Vault, AWS Secrets Manager) write each secret to a
//! file inside the container rather than into the environment. Accepting a
//! sibling `<VAR>_FILE` lets operators point at that file so the value
//! never enters the process environment — `docker inspect` then only shows
//! a path, not the secret.

/// Resolve `var`, falling back to the trimmed contents of the file named by
/// the `<var>_FILE` sibling when `var` itself is unset or empty.
///
/// The direct env value is returned verbatim (a password may legitimately
/// contain spaces). File contents are trimmed — secret managers and
/// `echo secret > file` routinely leave a trailing newline. Returns `None`
/// when neither source yields a non-empty value.
pub fn env_or_file(var: &str) -> Option<String> {
    if let Ok(v) = std::env::var(var)
        && !v.is_empty()
    {
        return Some(v);
    }
    let file_var = format!("{var}_FILE");
    let path = std::env::var(&file_var).ok()?;
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    match std::fs::read_to_string(path) {
        Ok(contents) => {
            let trimmed = contents.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        Err(e) => {
            tracing::error!(
                target: "croniq::config",
                var = %file_var,
                error = %e,
                "failed to read secret file referenced by env var"
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // These tests mutate process-global env vars, so they must not run
    // concurrently with each other. Each uses a unique var name to avoid
    // cross-test interference under the default parallel runner.
    #[test]
    fn direct_env_wins_and_is_verbatim() {
        let var = "CRONIQ_TEST_SECRET_DIRECT";
        unsafe { std::env::set_var(var, "  pass word  ") };
        assert_eq!(env_or_file(var).as_deref(), Some("  pass word  "));
        unsafe { std::env::remove_var(var) };
    }

    #[test]
    fn falls_back_to_file_and_trims() {
        let var = "CRONIQ_TEST_SECRET_FILE";
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(f, "supersecret").unwrap();
        unsafe { std::env::remove_var(var) };
        unsafe { std::env::set_var(format!("{var}_FILE"), f.path()) };
        assert_eq!(env_or_file(var).as_deref(), Some("supersecret"));
        unsafe { std::env::remove_var(format!("{var}_FILE")) };
    }

    #[test]
    fn none_when_unset() {
        let var = "CRONIQ_TEST_SECRET_UNSET";
        unsafe { std::env::remove_var(var) };
        unsafe { std::env::remove_var(format!("{var}_FILE")) };
        assert_eq!(env_or_file(var), None);
    }
}

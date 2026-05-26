//! Leak-safe output for freshly minted secrets.
//!
//! `croniq init` / `quickstart` surface secrets that exist nowhere else:
//! the seeded API key and the generated admin password (only their hashes
//! reach the DB). Printed straight to stdout they land in whatever log the
//! operator pipes the command into — docker/journald, CI transcripts,
//! `tee init.log`. CodeQL's `rust/cleartext-logging` flags exactly this.
//!
//! The sink reveals secrets on stdout only when stdout is a terminal (or
//! `--print-secrets` forces it). Otherwise it writes them to
//! `$DATA_DIR/initial-credentials` (mode 0600 on Unix) and prints just the
//! path — the `kubeadm init` shape.

use std::io::IsTerminal;
use std::path::Path;

use miette::{IntoDiagnostic, Result};

const CREDENTIALS_FILE: &str = "initial-credentials";

struct Entry {
    label: String,
    value: String,
    usage: Option<String>,
}

/// Collects secrets during a command and emits them once at the end,
/// choosing between an inline stdout reveal and a private file.
pub struct CredentialSink {
    reveal: bool,
    entries: Vec<Entry>,
}

impl CredentialSink {
    /// `force_print` mirrors the `--print-secrets` flag. When it is false,
    /// secrets are revealed inline only if stdout is an interactive terminal.
    pub fn new(force_print: bool) -> Self {
        Self::with_reveal(force_print || std::io::stdout().is_terminal())
    }

    fn with_reveal(reveal: bool) -> Self {
        Self {
            reveal,
            entries: Vec::new(),
        }
    }

    /// Queue a labelled secret (e.g. an admin login line).
    pub fn add(&mut self, label: impl Into<String>, value: impl Into<String>) {
        self.entries.push(Entry {
            label: label.into(),
            value: value.into(),
            usage: None,
        });
    }

    /// Queue a labelled secret plus a usage hint that itself embeds the
    /// secret (e.g. `Authorization: ApiKey <key>`), so the hint is gated by
    /// the same reveal decision rather than leaking via a stray `println!`.
    pub fn add_with_usage(
        &mut self,
        label: impl Into<String>,
        value: impl Into<String>,
        usage: impl Into<String>,
    ) {
        self.entries.push(Entry {
            label: label.into(),
            value: value.into(),
            usage: Some(usage.into()),
        });
    }

    /// Emit queued secrets. Prints to stdout when revealing, otherwise
    /// writes a 0600 file under `data_dir` and prints its path. No-op when
    /// nothing was queued, so commands that seed no secret stay silent.
    pub fn flush(self, data_dir: &Path) -> Result<()> {
        if self.entries.is_empty() {
            return Ok(());
        }
        if self.reveal {
            self.print_inline();
            Ok(())
        } else {
            self.write_file(data_dir)
        }
    }

    fn print_inline(&self) {
        println!();
        println!("=== Credentials (save these — they won't be shown again) ===");
        for e in &self.entries {
            println!();
            println!("{}:", e.label);
            println!("  {}", e.value);
            if let Some(usage) = &e.usage {
                println!("  {usage}");
            }
        }
        println!();
    }

    fn write_file(&self, data_dir: &Path) -> Result<()> {
        let path = data_dir.join(CREDENTIALS_FILE);

        let mut body = String::new();
        body.push_str("# Croniq initial credentials — generated once during init.\n");
        body.push_str("# Copy these into your secret store, then delete this file.\n");
        for e in &self.entries {
            body.push('\n');
            body.push_str(&format!("{}: {}\n", e.label, e.value));
            if let Some(usage) = &e.usage {
                body.push_str(&format!("  {usage}\n"));
            }
        }

        write_private(&path, &body)?;

        println!();
        println!(
            "Credentials written to {} (stdout is not a terminal, so they were not printed).",
            path.display()
        );
        println!("  Restrict access, copy them to your secret store, then delete the file.");
        println!("  Re-run with --print-secrets to echo them to stdout instead.");
        Ok(())
    }
}

#[cfg(unix)]
fn write_private(path: &Path, body: &str) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    // `.mode(0o600)` covers the create case; `set_permissions` afterwards
    // tightens an already-existing file from a previous (wider) run.
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .into_diagnostic()?;
    f.write_all(body.as_bytes()).into_diagnostic()?;
    f.flush().into_diagnostic()?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).into_diagnostic()?;
    Ok(())
}

#[cfg(not(unix))]
fn write_private(path: &Path, body: &str) -> Result<()> {
    // Windows has no POSIX mode bits; the data dir is operator-private by
    // convention, so we write the file and rely on its directory ACLs.
    std::fs::write(path, body).into_diagnostic()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn tempdir() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("croniq-secret-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn writes_secrets_to_private_file_when_not_revealing() {
        let dir = tempdir();
        let mut sink = CredentialSink::with_reveal(false);
        sink.add_with_usage(
            "API Key",
            "croniq_secret123",
            "Authorization: ApiKey croniq_secret123",
        );
        sink.flush(&dir).unwrap();

        let path = dir.join(CREDENTIALS_FILE);
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("croniq_secret123"));
        assert!(body.contains("Authorization: ApiKey croniq_secret123"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "credentials file must be 0600");
        }
    }

    #[test]
    fn revealing_prints_inline_and_writes_no_file() {
        let dir = tempdir();
        let mut sink = CredentialSink::with_reveal(true);
        sink.add("Admin login", "admin / hunter2");
        sink.flush(&dir).unwrap();

        assert!(
            !dir.join(CREDENTIALS_FILE).exists(),
            "revealing inline must not drop a credentials file"
        );
    }

    #[test]
    fn empty_sink_is_a_noop() {
        let dir = tempdir();
        let sink = CredentialSink::with_reveal(false);
        sink.flush(&dir).unwrap();

        assert!(
            !dir.join(CREDENTIALS_FILE).exists(),
            "a command that seeds no secret must not create the file"
        );
    }
}

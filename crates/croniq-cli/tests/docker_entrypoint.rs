//! `docker-entrypoint.sh` hands seed credentials to `croniq init` and then
//! starts the server. Neither step may expose them more than it has to
//! (review 2026-10-24, C-3 and R-9):
//!
//! * `croniq init` gets the admin password and the API key through its
//!   environment, never its argv (world-readable in `/proc/<pid>/cmdline`);
//! * the server is exec'd without the admin password and without any value
//!   the script itself loaded from a `<VAR>_FILE`.
//!
//! The script runs unprivileged here, so it skips the `su-exec` hop. A fake
//! `croniq` on `PATH` records what it was given, and `env` stands in for the
//! server so its environment can be read back.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docker-entrypoint.sh")
}

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "croniq-entrypoint-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn seed_secrets_stay_off_argv_and_out_of_the_server_environment() {
    use std::os::unix::fs::PermissionsExt;

    let dir = scratch();
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let data = dir.join("data");
    let record = dir.join("init-record");
    // Fake `croniq`: record argv and the two variables, create the DB file
    // the entrypoint checks for, succeed.
    let fake = bin.join("croniq");
    std::fs::write(
        &fake,
        format!(
            "#!/bin/sh\n\
             {{ echo \"ARGV $*\"; echo \"PW $CRONIQ_ADMIN_PASSWORD\"; echo \"KEY $CRONIQ_INIT_API_KEY\"; }} > '{}'\n\
             mkdir -p \"$3\" && touch \"$3/croniq.db\"\n",
            record.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();

    let pw_file = dir.join("pw");
    std::fs::write(&pw_file, "file-password-123\n").unwrap();
    let key_file = dir.join("key");
    std::fs::write(&key_file, "croniq_filekey123\n").unwrap();

    let out = Command::new("sh")
        .arg(script())
        .arg("env")
        .env_clear()
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin:/usr/sbin:/sbin", bin.display()),
        )
        .env("CRONIQ_DATA_DIR", &data)
        .env("CRONIQ_ADMIN_PASSWORD_FILE", &pw_file)
        .env("CRONIQ_INIT_API_KEY_FILE", &key_file)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "entrypoint failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let record = std::fs::read_to_string(&record).unwrap();
    assert!(
        !record.lines().next().unwrap().contains("file-password-123"),
        "password on argv: {record}"
    );
    assert!(
        !record.lines().next().unwrap().contains("croniq_filekey123"),
        "API key on argv: {record}"
    );
    assert!(record.contains("PW file-password-123"), "{record}");
    assert!(record.contains("KEY croniq_filekey123"), "{record}");

    let server_env = String::from_utf8_lossy(&out.stdout);
    assert!(
        !server_env.contains("file-password-123"),
        "admin password reached the server: {server_env}"
    );
    assert!(
        !server_env.contains("croniq_filekey123"),
        "file-loaded key reached the server: {server_env}"
    );
    assert!(
        server_env.contains("CRONIQ_INIT_API_KEY_FILE="),
        "the _FILE pointer stays for the server to resolve itself"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

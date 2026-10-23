//! `croniq-server doctor` and `CRONIQ_REFRESH_COOKIE` (issue #880).
//!
//! An unusable cookie name stops the boot, so `doctor` — the pre-deploy gate —
//! has to report it instead of calling a configuration healthy that the next
//! start refuses. These run the real binary: the env read lives in `main.rs`.

use std::process::{Command, Output};

const FINDING: &str = "CRONIQ_REFRESH_COOKIE is not a usable cookie name";

fn doctor(cookie_name: Option<&str>) -> Output {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("Croniqfile");
    std::fs::write(&config, "").unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_croniq-server"));
    cmd.arg("--config")
        .arg(&config)
        .arg("--data-dir")
        .arg(dir.path().join("data"))
        .arg("doctor")
        .env_remove("CRONIQ_REFRESH_COOKIE");
    if let Some(name) = cookie_name {
        cmd.env("CRONIQ_REFRESH_COOKIE", name);
    }
    cmd.output().unwrap()
}

#[test]
fn doctor_reports_an_unusable_cookie_name_as_critical() {
    let out = doctor(Some("croniq;refresh"));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "{stdout}");
    assert!(
        stdout.contains(&format!("[CRITICAL] {FINDING}")),
        "{stdout}"
    );
}

#[test]
fn doctor_accepts_a_usable_cookie_name() {
    for name in [None, Some("croniq_refresh_lane-a")] {
        let out = doctor(name);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(!stdout.contains(FINDING), "{name:?}: {stdout}");
    }
}

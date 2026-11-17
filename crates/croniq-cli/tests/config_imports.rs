//! `croniq validate` and `croniq compile` read a Croniqfile the way the server
//! loads it: with its `import`s inlined. They used to check the one file on its
//! own, so a split config the server loads without complaint failed with
//! "calendar … is not defined" / "concurrency_group … is not defined" and
//! warned about a missing timezone and retry policy that the imported
//! `defaults { }` supplies (review 2026-11-17, finding 3).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "croniq-cli-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A main file whose jobs lean on an imported `defaults`, calendar and group.
fn split_config(dir: &Path) -> PathBuf {
    std::fs::write(
        dir.join("shared.croniq"),
        r#"defaults {
  timezone Europe/Vienna
  retry fixed { max_attempts 2; delay 5s }
}
calendar business { timezone Europe/Vienna; include weekly monday tuesday }
concurrency_group crm { max_concurrent 1 }
"#,
    )
    .unwrap();
    let main = dir.join("main.croniq");
    std::fs::write(
        &main,
        r#"import ./shared.croniq
job a:one { every day at 04:20 { calendar business } }
job a:two { every 5 minutes; concurrency_group crm }
"#,
    )
    .unwrap();
    main
}

fn croniq(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_croniq"))
        .args(args)
        .output()
        .expect("runs croniq")
}

#[test]
fn validate_sees_what_an_import_supplies() {
    let dir = scratch("validate");
    let main = split_config(&dir);
    let out = croniq(&["validate", main.to_str().unwrap()]);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(out.status.success(), "{stderr}");
    for unwanted in ["not defined", "declares no timezone", "built-in policy"] {
        assert!(!stderr.contains(unwanted), "{unwanted:?} in:\n{stderr}");
    }
    assert!(stderr.contains("ok: 2 job(s)"), "{stderr}");
}

#[test]
fn compile_applies_the_imported_defaults() {
    let dir = scratch("compile");
    let main = split_config(&dir);
    let out = croniq(&["compile", main.to_str().unwrap()]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");

    let json: serde_json::Value = serde_json::from_slice(&out.stdout).expect("JSON on stdout");
    let jobs = json["jobs"].as_array().expect("jobs array");
    let one = jobs.iter().find(|j| j["key"] == "a:one").expect("a:one");
    assert_eq!(one["retry"]["strategy"], "fixed", "{one}");
    assert_eq!(one["retry"]["max_attempts"], 2, "{one}");
    assert_eq!(one["timezone"], "Europe/Vienna", "{one}");
}

// The server skips an import it cannot inline with a warning, and the jobs in
// it are then missing without anything failing. `validate` and `compile` are
// the gate before a deploy, so there it is an error.

#[test]
fn validate_fails_on_an_import_that_matches_nothing() {
    let dir = scratch("missing");
    let main = dir.join("main.croniq");
    std::fs::write(
        &main,
        "import ./nowhere/*.croniq\njob a:b { every 5 minutes; retry none { } }\n",
    )
    .unwrap();
    let out = croniq(&["validate", main.to_str().unwrap()]);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("error: import") && stderr.contains("nowhere"),
        "{stderr}"
    );
}

#[test]
fn validate_and_compile_fail_on_an_imported_file_that_does_not_parse() {
    let dir = scratch("broken");
    std::fs::write(dir.join("broken.croniq"), "job x:y { every 5 minutes\n").unwrap();
    let main = dir.join("main.croniq");
    std::fs::write(
        &main,
        "import ./broken.croniq\njob a:b { every 5 minutes; retry none { } }\n",
    )
    .unwrap();

    for command in ["validate", "compile"] {
        let out = croniq(&[command, main.to_str().unwrap()]);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{command}: {stderr}");
        assert!(
            stderr.contains("error: imported file") && stderr.contains("broken.croniq"),
            "{command}: {stderr}"
        );
        assert!(out.stdout.is_empty(), "{command} printed output: {stderr}");
    }
}

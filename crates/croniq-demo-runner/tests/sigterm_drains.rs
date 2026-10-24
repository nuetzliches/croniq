//! SIGTERM drains the runner and exits 0 (review 2026-10-24, R-4).
//!
//! Before, neither runner binary handled a signal: `docker stop` killed the
//! process mid-job, with no ack, and the claims sat until the reaper requeued
//! them. Unix-only, because that is where SIGTERM exists.
#![cfg(unix)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Answers every request with an empty poll response, after a short pause so
/// the idle loop does not spin. Counts polls.
fn mock_server() -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let polls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&polls);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let seen = Arc::clone(&seen);
            std::thread::spawn(move || {
                let mut buf = [0u8; 8192];
                let n = stream.read(&mut buf).unwrap_or(0);
                if String::from_utf8_lossy(&buf[..n]).starts_with("POST /v1/work/poll") {
                    seen.fetch_add(1, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(100));
                }
                let body = br#"{"work":[],"cancel":[]}"#;
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(body);
            });
        }
    });
    (base, polls)
}

#[test]
fn sigterm_drains_and_exits_cleanly() {
    let (base, polls) = mock_server();
    let state = std::env::temp_dir().join(format!("croniq-demo-sigterm-{}", std::process::id()));
    let mut child = Command::new(env!("CARGO_BIN_EXE_croniq-demo-runner"))
        .env("CRONIQ_SERVER_URL", &base)
        .env("RUNNER_ID", "sigterm-test")
        .env("CRONIQ_RUNNER_DATA_DIR", &state)
        .env("RUST_LOG", "warn")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("runner binary starts");

    let deadline = Instant::now() + Duration::from_secs(10);
    while polls.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(polls.load(Ordering::SeqCst) > 0, "the runner never polled");

    let pid = child.id().to_string();
    assert!(
        Command::new("kill")
            .args(["-TERM", &pid])
            .status()
            .unwrap()
            .success()
    );

    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("the runner did not exit after SIGTERM");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let _ = std::fs::remove_dir_all(&state);
    assert_eq!(
        status.code(),
        Some(0),
        "SIGTERM must drain and exit 0, not kill the process: {status:?}"
    );
}

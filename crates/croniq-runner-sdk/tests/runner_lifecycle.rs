//! The run loop against a scripted server, for behaviour the YAML runner
//! conformance cases pin for the other SDKs but no Rust binding runs yet
//! (review 2026-10-24, R-3 / R-4 / R-7 / K-1 / K-2 / K-3):
//!
//! - case 05: lease renewal at the configured `renew_interval`;
//! - case 06: drain lets an in-flight handler finish and acks it as success;
//! - cases 13 / 14: hostile identifiers are refused on ingest;
//! - a renew answered with 409 (lease lost) aborts the handler and acks it as
//!   failure; a 403 stops renewing;
//! - a drain timeout aborts the handler and still sends the failure ack;
//! - an ack answered with 503 is sent again.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use croniq_runner_sdk::CroniqRunner;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// `(path, call index for that path) -> (status, body)`.
type Script = Arc<dyn Fn(&str, usize) -> (u16, Value) + Send + Sync>;

#[derive(Default)]
struct Log {
    requests: Vec<(String, Value)>,
}

impl Log {
    fn bodies(&self, path: &str) -> Vec<Value> {
        self.requests
            .iter()
            .filter(|(p, _)| p == path)
            .map(|(_, b)| b.clone())
            .collect()
    }
}

async fn serve_one(mut socket: TcpStream, script: Script, log: Arc<Mutex<Log>>) {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    let header_end = loop {
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
        match socket.read(&mut tmp).await {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let path = head
        .split_whitespace()
        .nth(1)
        .unwrap_or_default()
        .to_string();
    let content_length = head
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| v.trim().parse::<usize>().ok())?
        })
        .unwrap_or(0);
    let mut body = buf[header_end..].to_vec();
    while body.len() < content_length {
        match socket.read(&mut tmp).await {
            Ok(0) | Err(_) => break,
            Ok(n) => body.extend_from_slice(&tmp[..n]),
        }
    }
    let json: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let nth = {
        let mut log = log.lock().unwrap();
        let nth = log.requests.iter().filter(|(p, _)| *p == path).count();
        log.requests.push((path.clone(), json));
        nth
    };
    let (status, response) = script(&path, nth);
    if path == "/v1/work/poll" && response["work"].as_array().is_none_or(|w| w.is_empty()) {
        // Pace the idle loop the way a long poll would.
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let payload = serde_json::to_vec(&response).unwrap();
    let head = format!(
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    let _ = socket.write_all(head.as_bytes()).await;
    let _ = socket.write_all(&payload).await;
    let _ = socket.shutdown().await;
}

async fn start_server(script: Script) -> (String, Arc<Mutex<Log>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let log = Arc::new(Mutex::new(Log::default()));
    let server_log = Arc::clone(&log);
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            tokio::spawn(serve_one(
                socket,
                Arc::clone(&script),
                Arc::clone(&server_log),
            ));
        }
    });
    (base, log)
}

fn assignment(execution_id: &str, job_key: &str) -> Value {
    json!({
        "execution_id": execution_id,
        "job_key": job_key,
        "fire_at": "2026-10-24T10:00:00Z",
        "attempt": 1,
        "metadata": {},
        "timeout": "5m",
    })
}

/// Hand out `work` on the first poll, nothing afterwards; answer everything
/// else with 200 unless `other` says otherwise.
fn script_with(
    work: Vec<Value>,
    other: impl Fn(&str, usize) -> Option<(u16, Value)> + Send + Sync + 'static,
) -> Script {
    Arc::new(move |path, nth| {
        if let Some(answer) = other(path, nth) {
            return answer;
        }
        match path {
            "/v1/work/poll" if nth == 0 => (200, json!({ "work": work.clone(), "cancel": [] })),
            "/v1/work/poll" => (200, json!({ "work": [], "cancel": [] })),
            _ => (200, json!({})),
        }
    })
}

fn runner(base: &str) -> croniq_runner_sdk::runner::RunnerBuilder {
    CroniqRunner::builder(base, "test-runner")
        .api_key("croniq_testkey")
        .max_inflight(1)
        .poll_retry_delay(Duration::from_millis(100))
        .capacity_backoff(Duration::from_millis(50))
}

async fn wait_until(budget: Duration, cond: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + budget;
    while Instant::now() < deadline {
        if cond() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    cond()
}

#[tokio::test]
async fn renew_runs_at_the_configured_interval() {
    // Conformance case 05.
    let (base, log) = start_server(script_with(
        vec![assignment("exec-renew", "long:job")],
        |_, _| None,
    ))
    .await;
    let runner = Arc::new(
        runner(&base)
            .renew_interval(Duration::from_millis(200))
            .build(),
    );
    runner
        .register("long:job", |_ctx| async {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            Ok(())
        })
        .await;
    let run = tokio::spawn({
        let runner = Arc::clone(&runner);
        async move { runner.start().await }
    });
    let acked = wait_until(Duration::from_secs(4), || {
        !log.lock().unwrap().bodies("/v1/work/ack").is_empty()
    })
    .await;
    run.abort();
    assert!(acked, "the execution was never acked");
    let log = log.lock().unwrap();
    assert!(
        log.bodies("/v1/work/renew").len() >= 2,
        "at least two renews"
    );
    assert_eq!(log.bodies("/v1/work/ack")[0]["status"], "success");
}

#[tokio::test]
async fn a_lost_lease_aborts_the_handler_and_acks_failure() {
    // Review 2026-10-24, R-3: a 409 on renew means another runner may already
    // run this execution. The handler must stop, not finish its side effect.
    let (base, log) = start_server(script_with(
        vec![assignment("exec-lost", "long:job")],
        |path, _| (path == "/v1/work/renew").then(|| (409, json!({ "renewed": false }))),
    ))
    .await;
    let finished = Arc::new(AtomicBool::new(false));
    let runner = Arc::new(
        runner(&base)
            .renew_interval(Duration::from_millis(100))
            .build(),
    );
    {
        let finished = Arc::clone(&finished);
        runner
            .register("long:job", move |_ctx| {
                let finished = Arc::clone(&finished);
                async move {
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    finished.store(true, Ordering::SeqCst);
                    Ok(())
                }
            })
            .await;
    }
    let run = tokio::spawn({
        let runner = Arc::clone(&runner);
        async move { runner.start().await }
    });
    let acked = wait_until(Duration::from_secs(2), || {
        !log.lock().unwrap().bodies("/v1/work/ack").is_empty()
    })
    .await;
    run.abort();
    assert!(
        acked,
        "a lost lease must end the execution well before the handler would"
    );
    let log = log.lock().unwrap();
    let ack = &log.bodies("/v1/work/ack")[0];
    assert_eq!(ack["status"], "failure");
    assert!(
        ack["error"].as_str().unwrap().contains("lease lost"),
        "{ack}"
    );
    assert_eq!(
        log.bodies("/v1/work/renew").len(),
        1,
        "no renew after the loss"
    );
    assert!(
        !finished.load(Ordering::SeqCst),
        "the handler must have been aborted"
    );
}

#[tokio::test]
async fn a_403_on_renew_stops_renewing() {
    // Review 2026-10-24, R-8: the refusal is permanent (ADR-0008).
    let (base, log) = start_server(script_with(
        vec![assignment("exec-403", "long:job")],
        |path, _| (path == "/v1/work/renew").then(|| (403, json!({}))),
    ))
    .await;
    let runner = Arc::new(
        runner(&base)
            .renew_interval(Duration::from_millis(100))
            .build(),
    );
    runner
        .register("long:job", |_ctx| async {
            tokio::time::sleep(Duration::from_millis(800)).await;
            Ok(())
        })
        .await;
    let run = tokio::spawn({
        let runner = Arc::clone(&runner);
        async move { runner.start().await }
    });
    let acked = wait_until(Duration::from_secs(3), || {
        !log.lock().unwrap().bodies("/v1/work/ack").is_empty()
    })
    .await;
    run.abort();
    assert!(acked);
    assert_eq!(log.lock().unwrap().bodies("/v1/work/renew").len(), 1);
}

#[tokio::test]
async fn a_5xx_ack_is_sent_again() {
    // Review 2026-10-24, K-1: "ack 503 then 200 → exactly two acks".
    let (base, log) = start_server(script_with(
        vec![assignment("exec-ack", "fast:job")],
        |path, nth| (path == "/v1/work/ack" && nth == 0).then(|| (503, json!({}))),
    ))
    .await;
    let runner = Arc::new(runner(&base).build());
    runner.register("fast:job", |_ctx| async { Ok(()) }).await;
    let run = tokio::spawn({
        let runner = Arc::clone(&runner);
        async move { runner.start().await }
    });
    let twice = wait_until(Duration::from_secs(4), || {
        log.lock().unwrap().bodies("/v1/work/ack").len() >= 2
    })
    .await;
    // Room for a third, wrong, attempt to show up.
    tokio::time::sleep(Duration::from_millis(300)).await;
    run.abort();
    assert!(twice, "the 503 ack was not retried");
    let acks = log.lock().unwrap().bodies("/v1/work/ack");
    assert_eq!(acks.len(), 2);
    assert!(
        acks.iter()
            .all(|a| a["execution_id"] == "exec-ack" && a["status"] == "success")
    );
}

#[tokio::test]
async fn a_hostile_job_key_is_acked_as_failure_without_running() {
    // Conformance case 13.
    let key = "billing:\x1b[31minvoice\r\n2026-01-01 ERROR forged record\x1b[0m";
    let (base, log) = start_server(script_with(
        vec![assignment("exec-hostile", key)],
        |_, _| None,
    ))
    .await;
    let ran = Arc::new(AtomicBool::new(false));
    let runner = Arc::new(runner(&base).build());
    {
        let ran = Arc::clone(&ran);
        runner
            .set_default_handler(move |_ctx| {
                let ran = Arc::clone(&ran);
                async move {
                    ran.store(true, Ordering::SeqCst);
                    Ok(())
                }
            })
            .await;
    }
    let run = tokio::spawn({
        let runner = Arc::clone(&runner);
        async move { runner.start().await }
    });
    let done = wait_until(Duration::from_secs(3), || {
        let log = log.lock().unwrap();
        !log.bodies("/v1/work/ack").is_empty() && log.bodies("/v1/work/poll").len() >= 2
    })
    .await;
    run.abort();
    assert!(done);
    let log = log.lock().unwrap();
    let acks = log.bodies("/v1/work/ack");
    assert_eq!(acks.len(), 1);
    assert_eq!(acks[0]["execution_id"], "exec-hostile");
    assert_eq!(acks[0]["status"], "failure");
    let error = acks[0]["error"].as_str().unwrap();
    assert!(
        !error.contains('\x1b') && !error.contains('\n'),
        "{error:?}"
    );
    assert!(log.bodies("/v1/work/renew").is_empty());
    assert!(
        !ran.load(Ordering::SeqCst),
        "not even a catch-all may run it"
    );
}

#[tokio::test]
async fn a_hostile_execution_id_is_dropped_without_an_ack() {
    // Conformance case 14.
    let (base, log) = start_server(script_with(
        vec![assignment("exec\x1b[2J\r\n001", "billing:invoice")],
        |_, _| None,
    ))
    .await;
    let ran = Arc::new(AtomicBool::new(false));
    let runner = Arc::new(runner(&base).build());
    {
        let ran = Arc::clone(&ran);
        runner
            .set_default_handler(move |_ctx| {
                let ran = Arc::clone(&ran);
                async move {
                    ran.store(true, Ordering::SeqCst);
                    Ok(())
                }
            })
            .await;
    }
    let run = tokio::spawn({
        let runner = Arc::clone(&runner);
        async move { runner.start().await }
    });
    let polled = wait_until(Duration::from_secs(3), || {
        log.lock().unwrap().bodies("/v1/work/poll").len() >= 3
    })
    .await;
    run.abort();
    assert!(polled, "the loop must keep polling");
    let log = log.lock().unwrap();
    assert!(log.bodies("/v1/work/ack").is_empty());
    assert!(log.bodies("/v1/work/renew").is_empty());
    assert!(!ran.load(Ordering::SeqCst));
    // And the dropped id was never reported as in flight.
    assert!(
        log.bodies("/v1/work/poll")
            .iter()
            .all(|p| p["inflight"].as_array().unwrap().is_empty())
    );
}

#[tokio::test]
async fn drain_lets_the_handler_finish_and_acks_success() {
    // Conformance case 06, through `run_until`.
    let (base, log) = start_server(script_with(
        vec![assignment("exec-drain", "graceful:job")],
        |_, _| None,
    ))
    .await;
    let runner = runner(&base).drain_timeout(Duration::from_secs(3)).build();
    runner
        .register("graceful:job", |_ctx| async {
            tokio::time::sleep(Duration::from_millis(500)).await;
            Ok(())
        })
        .await;
    let started = Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(4),
        runner.run_until(tokio::time::sleep(Duration::from_millis(300))),
    )
    .await
    .expect("drain must return");
    assert!(result.is_ok(), "{result:?}");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
    let acks = log.lock().unwrap().bodies("/v1/work/ack");
    assert_eq!(acks.len(), 1);
    assert_eq!(acks[0]["status"], "success");
}

#[tokio::test]
async fn a_drain_timeout_aborts_the_handler_and_still_acks_failure() {
    // Review 2026-10-24, R-4 / K-3 / K-4: bounded, and the final ack is sent.
    let (base, log) = start_server(script_with(
        vec![assignment("exec-hung", "hung:job")],
        |_, _| None,
    ))
    .await;
    let runner = runner(&base)
        .drain_timeout(Duration::from_millis(300))
        .build();
    runner
        .register("hung:job", |_ctx| {
            std::future::pending::<Result<(), croniq_runner_sdk::HandlerError>>()
        })
        .await;
    let started = Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        runner.run_until(tokio::time::sleep(Duration::from_millis(200))),
    )
    .await
    .expect("a hung handler must not block the shutdown");
    assert!(result.is_ok(), "{result:?}");
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
    let acks = log.lock().unwrap().bodies("/v1/work/ack");
    assert_eq!(acks.len(), 1);
    assert_eq!(acks[0]["status"], "failure");
    assert!(
        acks[0]["error"].as_str().unwrap().contains("shutting down"),
        "{}",
        acks[0]
    );
}

#[tokio::test]
async fn drain_interrupts_an_idle_long_poll() {
    // A shutdown must not wait out the server's 30 s long-poll window.
    let script: Script = Arc::new(|path, _| match path {
        "/v1/work/poll" => (200, json!({ "work": [], "cancel": [] })),
        _ => (200, json!({})),
    });
    // A server that holds the poll for 10 s.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            let script = Arc::clone(&script);
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(10)).await;
                serve_one(socket, script, Arc::new(Mutex::new(Log::default()))).await;
            });
        }
    });
    let runner = runner(&base).build();
    let started = Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        runner.run_until(tokio::time::sleep(Duration::from_millis(200))),
    )
    .await
    .expect("drain must not wait for the long poll");
    assert!(result.is_ok());
    assert!(started.elapsed() < Duration::from_secs(2));
}

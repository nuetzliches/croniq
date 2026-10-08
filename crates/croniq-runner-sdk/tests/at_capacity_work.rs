//! The poll loop at `max_inflight` capacity.
//!
//! The Rust SDK has no binding for the runner conformance cases, so these tests
//! drive the poll loop against a minimal scripted HTTP server to pin what the
//! YAML cases pin for the other SDKs:
//!
//! - `21-work-on-at-capacity-poll-is-run.yaml` (issue #817): with
//!   `max_inflight = 1` and one execution still running, the runner keeps
//!   polling (control-slot polling, #176), and when the server answers that
//!   poll with an assignment the runner must dispatch it. The server has
//!   already committed the claim by then; an assignment the runner ignores
//!   stays `claimed` until its timeout plus grace.
//! - `22-freed-slot-ends-capacity-backoff.yaml` (issue #845): the backoff after
//!   an at-capacity poll ends as soon as the in-flight execution completes, so
//!   a freed slot is refilled at once rather than after the full backoff.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use croniq_runner_sdk::CroniqRunner;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[derive(Default)]
struct Recorded {
    /// The `inflight` list of every poll, in arrival order.
    polls: Vec<Vec<String>>,
    /// `(execution_id, status)` of every ack.
    acks: Vec<(String, String)>,
}

fn assignment(execution_id: &str, job_key: &str) -> serde_json::Value {
    serde_json::json!({
        "execution_id": execution_id,
        "job_key": job_key,
        "fire_at": "2026-10-06T10:00:00Z",
        "attempt": 1,
        "metadata": {},
        "timeout": "5m",
    })
}

/// The work the scripted server hands out on the poll with this 0-based index.
type PollScript = fn(usize) -> Vec<serde_json::Value>;

async fn handle(
    mut socket: TcpStream,
    script: PollScript,
    polls_seen: Arc<AtomicUsize>,
    recorded: Arc<Mutex<Recorded>>,
) -> std::io::Result<()> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let header_end = loop {
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
        let n = socket.read(&mut tmp).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&tmp[..n]);
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
        let n = socket.read(&mut tmp).await?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();

    let response = match path.as_str() {
        "/v1/work/poll" => {
            let inflight: Vec<String> = json["inflight"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let idle = inflight.is_empty();
            recorded.lock().unwrap().polls.push(inflight);
            let work = script(polls_seen.fetch_add(1, Ordering::SeqCst));
            if work.is_empty() && idle {
                // Pace the idle loop: a real server long-polls here. An
                // at-capacity poll is answered at once, as the server does.
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            serde_json::json!({ "work": work, "cancel": [] })
        }
        "/v1/work/ack" => {
            recorded.lock().unwrap().acks.push((
                json["execution_id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                json["status"].as_str().unwrap_or_default().to_string(),
            ));
            serde_json::json!({})
        }
        _ => serde_json::json!({}),
    };

    let payload = serde_json::to_vec(&response).unwrap();
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    socket.write_all(head.as_bytes()).await?;
    socket.write_all(&payload).await?;
    socket.shutdown().await
}

/// Runs a `max_inflight = 1` runner against `script` until both `exec-a` (a
/// handler sleeping `slow_ms`) and `exec-b` (a no-op) are acked, or `budget`
/// runs out.
async fn run_against(
    script: PollScript,
    capacity_backoff: Duration,
    slow_ms: u64,
    budget: Duration,
) -> Recorded {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let polls_seen = Arc::new(AtomicUsize::new(0));
    let recorded = Arc::new(Mutex::new(Recorded::default()));

    let server = {
        let polls_seen = Arc::clone(&polls_seen);
        let recorded = Arc::clone(&recorded);
        tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                let polls_seen = Arc::clone(&polls_seen);
                let recorded = Arc::clone(&recorded);
                tokio::spawn(async move {
                    let _ = handle(socket, script, polls_seen, recorded).await;
                });
            }
        })
    };

    let runner = Arc::new(
        CroniqRunner::builder(&base_url, "test-runner")
            .api_key("croniq_testkey")
            .capabilities(vec!["work".to_string()])
            .max_inflight(1)
            .capacity_backoff(capacity_backoff)
            .poll_retry_delay(Duration::from_millis(100))
            .build(),
    );
    runner
        .register("slow:job", move |_ctx| async move {
            tokio::time::sleep(Duration::from_millis(slow_ms)).await;
            Ok(())
        })
        .await;
    runner.register("fast:job", |_ctx| async { Ok(()) }).await;

    let run = {
        let runner = Arc::clone(&runner);
        tokio::spawn(async move { runner.start().await })
    };

    let deadline = Instant::now() + budget;
    loop {
        if recorded.lock().unwrap().acks.len() >= 2 || Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    run.abort();
    server.abort();

    std::mem::take(&mut *recorded.lock().unwrap())
}

fn sorted_acks(recorded: &Recorded) -> Vec<(String, String)> {
    let mut acks = recorded.acks.clone();
    acks.sort();
    acks
}

fn both_succeeded() -> [(String, String); 2] {
    [
        ("exec-a".to_string(), "success".to_string()),
        ("exec-b".to_string(), "success".to_string()),
    ]
}

#[tokio::test]
async fn work_delivered_on_an_at_capacity_poll_is_run() {
    let recorded = run_against(
        |poll| match poll {
            0 => vec![assignment("exec-a", "slow:job")],
            // Sent while exec-a still holds the only slot.
            1 => vec![assignment("exec-b", "fast:job")],
            _ => vec![],
        },
        Duration::from_millis(50),
        800,
        Duration::from_secs(4),
    )
    .await;

    assert!(
        recorded.polls.len() >= 2 && recorded.polls[1] == ["exec-a"],
        "the second poll must report exec-a in flight (at capacity), got {:?}",
        recorded.polls
    );
    assert_eq!(
        sorted_acks(&recorded),
        both_succeeded(),
        "work delivered on the at-capacity poll must be run and acked, not dropped"
    );
}

#[tokio::test]
async fn a_freed_slot_ends_the_capacity_backoff() {
    let started = Instant::now();
    let recorded = run_against(
        |poll| match poll {
            0 => vec![assignment("exec-a", "slow:job")],
            // Poll 1 is sent at capacity and gets nothing, so the runner
            // backs off; poll 2 is the first after exec-a completes.
            2 => vec![assignment("exec-b", "fast:job")],
            _ => vec![],
        },
        // Far longer than the budget: exec-b is reachable in time only if
        // exec-a's completion cuts the backoff short.
        Duration::from_secs(5),
        300,
        Duration::from_millis(2500),
    )
    .await;

    assert!(
        recorded.polls.len() >= 2 && recorded.polls[1] == ["exec-a"],
        "the second poll must report exec-a in flight (at capacity), got {:?}",
        recorded.polls
    );
    assert_eq!(
        sorted_acks(&recorded),
        both_succeeded(),
        "exec-b must be claimed once exec-a frees the slot, not after the 5 s backoff (took {:?})",
        started.elapsed()
    );
}

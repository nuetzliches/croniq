//! Work delivered on an at-capacity poll is run, never dropped (issue #817).
//!
//! The Rust SDK has no binding for the runner conformance cases, so this test
//! drives the poll loop against a minimal scripted HTTP server to pin the
//! behaviour that `sdks/conformance/cases/21-work-on-at-capacity-poll-is-run.yaml`
//! pins for the other SDKs: with `max_inflight = 1` and one execution still
//! running, the runner keeps polling (control-slot polling, #176), and when the
//! server answers that poll with an assignment the runner must dispatch it.
//! The server has already committed the claim by then; an assignment the
//! runner ignores stays `claimed` until its timeout plus grace.

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

async fn handle(
    mut socket: TcpStream,
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
            let inflight = json["inflight"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            recorded.lock().unwrap().polls.push(inflight);
            let work = match polls_seen.fetch_add(1, Ordering::SeqCst) {
                0 => vec![assignment("exec-a", "slow:job")],
                // Sent while exec-a still holds the only slot.
                1 => vec![assignment("exec-b", "fast:job")],
                _ => {
                    // Pace the idle loop: a real server long-polls here.
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    vec![]
                }
            };
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

#[tokio::test]
async fn work_delivered_on_an_at_capacity_poll_is_run() {
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
                    let _ = handle(socket, polls_seen, recorded).await;
                });
            }
        })
    };

    let runner = Arc::new(
        CroniqRunner::builder(&base_url, "test-runner")
            .api_key("croniq_testkey")
            .capabilities(vec!["work".to_string()])
            .max_inflight(1)
            .capacity_backoff(Duration::from_millis(50))
            .poll_retry_delay(Duration::from_millis(100))
            .build(),
    );
    runner
        .register("slow:job", |_ctx| async {
            tokio::time::sleep(Duration::from_millis(800)).await;
            Ok(())
        })
        .await;
    runner.register("fast:job", |_ctx| async { Ok(()) }).await;

    let run = {
        let runner = Arc::clone(&runner);
        tokio::spawn(async move { runner.start().await })
    };

    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        if recorded.lock().unwrap().acks.len() >= 2 || Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    run.abort();
    server.abort();

    let recorded = recorded.lock().unwrap();
    assert!(
        recorded.polls.len() >= 2 && recorded.polls[1] == ["exec-a"],
        "the second poll must report exec-a in flight (at capacity), got {:?}",
        recorded.polls
    );
    let mut acks = recorded.acks.clone();
    acks.sort();
    assert_eq!(
        acks,
        [
            ("exec-a".to_string(), "success".to_string()),
            ("exec-b".to_string(), "success".to_string()),
        ],
        "work delivered on the at-capacity poll must be run and acked, not dropped"
    );
}

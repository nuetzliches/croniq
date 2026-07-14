//! YAML-driven conformance suite for the trigger (producer) client.
//!
//! Drives [`TriggerClient`] against the shared, language-agnostic cases in
//! `sdks/conformance/cases-trigger/` (issue #287) — this is the first Rust
//! binding to consume the trigger cases. Each case spins up a scripted mock
//! HTTP server, makes the declared `trigger(...)` calls in order, and asserts
//! both the recorded wire traffic (method, path, headers, body shape, request
//! counts) and the outcome surfaced to the caller (a value or an error).
//!
//! The case schema and matching semantics mirror the reference binding in
//! `sdks/conformance/bindings/typescript` (`mock-server.ts`, `body-matcher.ts`,
//! `conformance.test.ts`) so all bindings enforce the same wire contract.

// Case structs are deserialization DTOs; not every field is read on every path.
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use croniq_runner_sdk::TriggerClient;
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

// ─────────────────────────── case schema (YAML DTOs) ───────────────────────

#[derive(Debug, Deserialize)]
struct CaseSpec {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    trigger_config: TriggerConfig,
    trigger_calls: Vec<TriggerCall>,
    server_script: Vec<ScriptEntry>,
    expectations: Expectations,
}

#[derive(Debug, Default, Deserialize)]
struct TriggerConfig {
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    bearer_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TriggerCall {
    request: CallRequest,
    expect: CallExpect,
}

#[derive(Debug, Deserialize)]
struct CallRequest {
    job_key: String,
    #[serde(default)]
    require: Option<Vec<String>>,
    #[serde(default)]
    prefer: Option<Vec<String>>,
    #[serde(default)]
    metadata: Option<serde_json::Value>,
    #[serde(default)]
    timeout: Option<String>,
    #[serde(default)]
    idempotency_key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CallExpect {
    #[serde(default)]
    response: Option<ExpectResponse>,
    #[serde(default)]
    error: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ExpectResponse {
    #[serde(default)]
    execution_id: Option<String>,
    #[serde(default)]
    queued: Option<i64>,
    #[serde(default)]
    deduplicated: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ScriptEntry {
    on: String,
    #[serde(default)]
    match_count: Option<usize>,
    respond: Respond,
}

#[derive(Debug, Clone, Deserialize)]
struct Respond {
    status: u16,
    #[serde(default)]
    body: Option<serde_json::Value>,
    #[serde(default)]
    delay_ms: Option<u64>,
    #[serde(default)]
    headers: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
struct Expectations {
    #[serde(default)]
    duration_max_ms: Option<u64>,
    http: Vec<HttpExpectation>,
}

#[derive(Debug, Deserialize)]
struct HttpExpectation {
    method: String,
    path: String,
    #[serde(default)]
    exact_count: Option<usize>,
    #[serde(default)]
    min_count: Option<usize>,
    #[serde(default)]
    max_count: Option<usize>,
    #[serde(default)]
    headers: Option<HashMap<String, String>>,
    #[serde(default)]
    body_match: Option<serde_json::Value>,
    #[serde(default)]
    body_absent: Option<Vec<String>>,
}

// ─────────────────────────────── mock server ───────────────────────────────

#[derive(Debug, Clone)]
struct RecordedRequest {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: String,
}

/// One (method, path) rule group with an independent hit counter.
struct RuleGroup {
    rules: Vec<Respond>,
    /// `match_count` for each rule, aligned by index with `rules`.
    match_counts: Vec<Option<usize>>,
    hits: usize,
}

/// Scripted HTTP server for a single conformance case. Groups rules by
/// `(method, path)`; each request increments the group's hit counter and
/// selects the rule whose `match_count` matches (or the fallthrough with no
/// `match_count`). Every request is recorded for post-hoc assertions.
struct MockServer {
    base_url: String,
    recorded: Arc<Mutex<Vec<RecordedRequest>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl MockServer {
    async fn start(script: &[ScriptEntry]) -> MockServer {
        let mut groups: HashMap<String, RuleGroup> = HashMap::new();
        for entry in script {
            let (method, path) = split_on(&entry.on);
            let group = groups
                .entry(format!("{method} {path}"))
                .or_insert(RuleGroup {
                    rules: Vec::new(),
                    match_counts: Vec::new(),
                    hits: 0,
                });
            group.rules.push(entry.respond.clone());
            group.match_counts.push(entry.match_count);
        }
        let groups = Arc::new(Mutex::new(groups));
        let recorded: Arc<Mutex<Vec<RecordedRequest>>> = Arc::new(Mutex::new(Vec::new()));

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let addr = listener.local_addr().expect("local addr");
        let base_url = format!("http://{addr}");

        let groups_task = Arc::clone(&groups);
        let recorded_task = Arc::clone(&recorded);
        let task = tokio::spawn(async move {
            loop {
                let (socket, _) = match listener.accept().await {
                    Ok(pair) => pair,
                    Err(_) => break,
                };
                let groups = Arc::clone(&groups_task);
                let recorded = Arc::clone(&recorded_task);
                tokio::spawn(async move {
                    let _ = handle_connection(socket, groups, recorded).await;
                });
            }
        });

        MockServer {
            base_url,
            recorded,
            task,
        }
    }
}

async fn handle_connection(
    mut socket: tokio::net::TcpStream,
    groups: Arc<Mutex<HashMap<String, RuleGroup>>>,
    recorded: Arc<Mutex<Vec<RecordedRequest>>>,
) -> std::io::Result<()> {
    // Read until the end of the header block.
    let mut buf: Vec<u8> = Vec::new();
    let mut tmp = [0u8; 4096];
    let header_end = loop {
        if let Some(pos) = find_subsequence(&buf, b"\r\n\r\n") {
            break pos + 4;
        }
        let n = socket.read(&mut tmp).await?;
        if n == 0 {
            return Ok(()); // connection closed before a full request
        }
        buf.extend_from_slice(&tmp[..n]);
    };

    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();

    let mut headers = HashMap::new();
    let mut content_length = 0usize;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            let key = k.trim().to_ascii_lowercase();
            let value = v.trim().to_string();
            if key == "content-length" {
                content_length = value.parse().unwrap_or(0);
            }
            headers.insert(key, value);
        }
    }

    // Read the remaining body bytes (reqwest sends Content-Length for JSON).
    let mut body = buf[header_end..].to_vec();
    while body.len() < content_length {
        let n = socket.read(&mut tmp).await?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    let body_str = String::from_utf8_lossy(&body[..content_length.min(body.len())]).to_string();

    recorded.lock().unwrap().push(RecordedRequest {
        method: method.clone(),
        path: path.clone(),
        headers,
        body: body_str,
    });

    // Select the scripted response.
    let selected = {
        let mut guard = groups.lock().unwrap();
        guard.get_mut(&format!("{method} {path}")).map(|group| {
            group.hits += 1;
            let hit = group.hits;
            let idx = group
                .match_counts
                .iter()
                .position(|mc| *mc == Some(hit))
                .or_else(|| group.match_counts.iter().position(|mc| mc.is_none()));
            idx.map(|i| group.rules[i].clone())
        })
    };

    let response = match selected {
        Some(Some(respond)) => respond,
        _ => Respond {
            status: 404,
            body: Some(serde_json::json!({ "error": format!("no rule for {method} {path}") })),
            delay_ms: None,
            headers: None,
        },
    };

    if let Some(delay) = response.delay_ms
        && delay > 0
    {
        tokio::time::sleep(Duration::from_millis(delay)).await;
    }

    write_response(&mut socket, &response).await
}

async fn write_response(
    socket: &mut tokio::net::TcpStream,
    respond: &Respond,
) -> std::io::Result<()> {
    let body_bytes: Vec<u8> = match &respond.body {
        None | Some(serde_json::Value::Null) => Vec::new(),
        Some(serde_json::Value::String(s)) => s.clone().into_bytes(),
        Some(value) => serde_json::to_vec(value).unwrap_or_default(),
    };

    let mut head = format!(
        "HTTP/1.1 {} {}\r\n",
        respond.status,
        reason_phrase(respond.status)
    );
    let mut wrote_content_type = false;
    if let Some(custom) = &respond.headers {
        for (k, v) in custom {
            if k.eq_ignore_ascii_case("content-type") {
                wrote_content_type = true;
            }
            head.push_str(&format!("{k}: {v}\r\n"));
        }
    }
    if !body_bytes.is_empty() && !wrote_content_type {
        head.push_str("content-type: application/json\r\n");
    }
    head.push_str(&format!("content-length: {}\r\n", body_bytes.len()));
    head.push_str("connection: close\r\n\r\n");

    socket.write_all(head.as_bytes()).await?;
    if !body_bytes.is_empty() {
        socket.write_all(&body_bytes).await?;
    }
    socket.flush().await?;
    let _ = socket.shutdown().await;
    Ok(())
}

// ───────────────────────────────── matching ────────────────────────────────

/// Subset matcher with a single wildcard token (`"*"`), mirroring the shared
/// `body-matcher.ts`:
///  * literal scalars must match exactly (numbers within a small epsilon),
///  * `"*"` matches any non-empty value,
///  * objects match recursively (extra actual keys ignored),
///  * arrays match length-and-order element-by-element,
///  * `null` asserts the key is present and explicitly null.
fn match_body(
    expected: &serde_json::Value,
    actual: &serde_json::Value,
    path: &str,
) -> Result<(), String> {
    use serde_json::Value;

    if expected.is_null() {
        return if actual.is_null() {
            Ok(())
        } else {
            Err(format!("{path}: expected null but got {actual}"))
        };
    }

    if let Value::String(s) = expected
        && s == "*"
    {
        return match actual {
            Value::Null => Err(format!(
                "{path}: expected non-empty wildcard match but got null"
            )),
            Value::String(a) if a.is_empty() => {
                Err(format!("{path}: expected non-empty string but got empty"))
            }
            _ => Ok(()),
        };
    }

    match expected {
        Value::Array(exp) => {
            let Value::Array(act) = actual else {
                return Err(format!("{path}: expected array but got {actual}"));
            };
            if exp.len() != act.len() {
                return Err(format!(
                    "{path}: expected {} item(s) but got {}",
                    exp.len(),
                    act.len()
                ));
            }
            for (i, (e, a)) in exp.iter().zip(act.iter()).enumerate() {
                match_body(e, a, &format!("{path}[{i}]"))?;
            }
            Ok(())
        }
        Value::Object(exp) => {
            let Value::Object(act) = actual else {
                return Err(format!("{path}: expected object but got {actual}"));
            };
            for (key, value) in exp {
                match act.get(key) {
                    None => return Err(format!("{path}.{key}: missing key")),
                    Some(a) => match_body(value, a, &format!("{path}.{key}"))?,
                }
            }
            Ok(())
        }
        Value::Number(exp) => {
            let (Some(e), Some(a)) = (exp.as_f64(), actual.as_f64()) else {
                return Err(format!("{path}: expected number but got {actual}"));
            };
            if (e - a).abs() > 1e-9 {
                Err(format!("{path}: expected {exp} but got {actual}"))
            } else {
                Ok(())
            }
        }
        Value::Bool(exp) => match actual.as_bool() {
            Some(a) if a == *exp => Ok(()),
            _ => Err(format!("{path}: expected {exp} but got {actual}")),
        },
        Value::String(exp) => match actual.as_str() {
            Some(a) if a == exp => Ok(()),
            _ => Err(format!("{path}: expected '{exp}' but got {actual}")),
        },
        Value::Null => unreachable!("handled above"),
    }
}

// ───────────────────────────────── driver ──────────────────────────────────

async fn run_case(spec: &CaseSpec) -> Result<(), String> {
    let server = MockServer::start(&spec.server_script).await;

    let mut builder = TriggerClient::builder(&server.base_url);
    if let Some(key) = &spec.trigger_config.api_key {
        builder = builder.api_key(key);
    }
    if let Some(token) = &spec.trigger_config.bearer_token {
        builder = builder.bearer_token(token);
    }
    let client = builder.build();

    let started = Instant::now();

    for (i, call) in spec.trigger_calls.iter().enumerate() {
        let mut req = client.trigger(&call.request.job_key);
        if let Some(metadata) = &call.request.metadata {
            let obj = metadata
                .as_object()
                .ok_or_else(|| format!("call[{i}]: metadata must be a JSON object"))?;
            req = req.metadata(obj.clone().into_iter().collect());
        }
        if let Some(require) = &call.request.require {
            req = req.require(require.clone());
        }
        if let Some(prefer) = &call.request.prefer {
            req = req.prefer(prefer.clone());
        }
        if let Some(timeout) = &call.request.timeout {
            req = req.timeout(timeout.clone());
        }
        if let Some(key) = &call.request.idempotency_key {
            req = req.idempotency_key(key.clone());
        }

        let result = req.send().await;
        check_call_outcome(i, &call.expect, &result)?;
    }

    if let Some(max_ms) = spec.expectations.duration_max_ms {
        let elapsed = started.elapsed().as_millis() as u64;
        if elapsed > max_ms {
            return Err(format!(
                "case exceeded duration_max_ms={max_ms} (took {elapsed}ms)"
            ));
        }
    }

    let recorded = server.recorded.lock().unwrap().clone();
    assert_http_expectations(&spec.expectations, &recorded)
}

fn check_call_outcome(
    i: usize,
    expect: &CallExpect,
    result: &Result<croniq_runner_sdk::TriggerResult, croniq_runner_sdk::TriggerError>,
) -> Result<(), String> {
    if expect.error == Some(true) {
        return match result {
            Err(_) => Ok(()),
            Ok(value) => Err(format!("call[{i}]: expected an error but got {value:?}")),
        };
    }

    let value = match result {
        Ok(value) => value,
        Err(err) => return Err(format!("call[{i}]: expected success but got error: {err}")),
    };

    if let Some(response) = &expect.response {
        if let Some(execution_id) = &response.execution_id {
            if execution_id == "*" {
                if value.execution_id.is_empty() {
                    return Err(format!("call[{i}]: expected non-empty execution_id"));
                }
            } else if &value.execution_id != execution_id {
                return Err(format!(
                    "call[{i}]: execution_id expected '{execution_id}' but got '{}'",
                    value.execution_id
                ));
            }
        }
        if let Some(queued) = response.queued
            && value.queued != queued
        {
            return Err(format!(
                "call[{i}]: queued expected {queued} but got {}",
                value.queued
            ));
        }
        if let Some(deduplicated) = response.deduplicated
            && value.deduplicated != deduplicated
        {
            return Err(format!(
                "call[{i}]: deduplicated expected {deduplicated} but got {}",
                value.deduplicated
            ));
        }
    }
    Ok(())
}

fn assert_http_expectations(
    expectations: &Expectations,
    recorded: &[RecordedRequest],
) -> Result<(), String> {
    for ex in &expectations.http {
        let matches: Vec<&RecordedRequest> = recorded
            .iter()
            .filter(|r| r.method.eq_ignore_ascii_case(&ex.method) && r.path == ex.path)
            .collect();
        let label = format!("{} {}", ex.method, ex.path);

        if let Some(exact) = ex.exact_count
            && matches.len() != exact
        {
            return Err(format!(
                "{label}: expected exact_count={exact} but got {}",
                matches.len()
            ));
        }
        if let Some(min) = ex.min_count
            && matches.len() < min
        {
            return Err(format!(
                "{label}: expected min_count={min} but got {}",
                matches.len()
            ));
        }
        if let Some(max) = ex.max_count
            && matches.len() > max
        {
            return Err(format!(
                "{label}: expected max_count={max} but got {}",
                matches.len()
            ));
        }

        // Header, body, and absence checks apply to the first matching request.
        if let Some(first) = matches.first() {
            if let Some(headers) = &ex.headers {
                for (name, expected) in headers {
                    let lower = name.to_ascii_lowercase();
                    let actual = first
                        .headers
                        .get(&lower)
                        .ok_or_else(|| format!("{label}: missing header '{name}'"))?;
                    if expected == "*" {
                        if actual.is_empty() {
                            return Err(format!("{label}: header '{name}' was empty"));
                        }
                    } else if actual != expected {
                        return Err(format!(
                            "{label}: header '{name}' expected '{expected}' but got '{actual}'"
                        ));
                    }
                }
            }

            if let Some(body_match) = &ex.body_match {
                let actual: serde_json::Value = if first.body.is_empty() {
                    serde_json::Value::Null
                } else {
                    serde_json::from_str(&first.body)
                        .map_err(|e| format!("{label}: request body was not valid JSON: {e}"))?
                };
                match_body(body_match, &actual, "$")
                    .map_err(|e| format!("{label}: body mismatch — {e}"))?;
            }

            if let Some(absent) = &ex.body_absent {
                let actual: serde_json::Value = if first.body.is_empty() {
                    serde_json::Value::Null
                } else {
                    serde_json::from_str(&first.body)
                        .map_err(|e| format!("{label}: request body was not valid JSON: {e}"))?
                };
                if let Some(obj) = actual.as_object() {
                    for key in absent {
                        if obj.contains_key(key) {
                            return Err(format!(
                                "{label}: body key '{key}' must be absent but was present"
                            ));
                        }
                    }
                }
            }
        } else if ex.headers.is_some() || ex.body_match.is_some() || ex.body_absent.is_some() {
            // A header/body assertion with zero matching requests is only OK
            // when the counts explicitly allow zero (e.g. max_count with no
            // min/exact). Otherwise it signals a missing request.
            let allows_zero = ex.exact_count == Some(0)
                || (ex.exact_count.is_none()
                    && ex.min_count.unwrap_or(0) == 0
                    && ex.max_count.is_some());
            if !allows_zero {
                return Err(format!(
                    "{label}: no matching request to assert headers/body against"
                ));
            }
        }
    }
    Ok(())
}

// ───────────────────────────────── helpers ─────────────────────────────────

fn split_on(on: &str) -> (String, String) {
    match on.split_once(' ') {
        Some((method, path)) => (method.to_ascii_uppercase(), path.to_string()),
        None => (on.to_ascii_uppercase(), String::new()),
    }
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Status",
    }
}

fn cases_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sdks/conformance/cases-trigger")
}

fn load_case_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read cases dir {}: {e}", dir.display()))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension()
                .is_some_and(|ext| ext == "yaml" || ext == "yml")
        })
        .collect();
    files.sort();
    files
}

/// Drives the trigger client against the shared producer cases in
/// `sdks/conformance/cases-trigger/` (issue #287).
///
/// The case suite is a separate, cross-SDK artifact that may not be present on
/// every base (e.g. before #287 merges to `main`). When the directory is
/// absent or empty this test **skips** rather than failing, so the runner is
/// already wired and lights up automatically once the shared cases land. The
/// trigger client's own behaviour is covered unconditionally by the unit tests
/// in `src/trigger.rs`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn trigger_conformance_suite() {
    let dir = cases_dir();
    if !dir.is_dir() {
        eprintln!(
            "skipping trigger conformance suite: {} not present \
             (shared cases from #287 not on this base yet)",
            dir.display()
        );
        return;
    }

    let files = load_case_files(&dir);
    if files.is_empty() {
        eprintln!(
            "skipping trigger conformance suite: no case files in {}",
            dir.display()
        );
        return;
    }

    let debug = std::env::var("CRONIQ_CONFORMANCE_DEBUG").as_deref() == Ok("1");
    let mut failures: Vec<String> = Vec::new();
    let mut ran = 0usize;
    for path in &files {
        let file_name = path.file_name().unwrap().to_string_lossy().to_string();
        let text =
            std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {file_name}: {e}"));
        let spec: CaseSpec = match serde_yaml::from_str(&text) {
            Ok(spec) => spec,
            Err(e) => {
                failures.push(format!("{file_name}: failed to parse case: {e}"));
                continue;
            }
        };
        ran += 1;
        match run_case(&spec).await {
            Ok(()) => {
                if debug {
                    eprintln!("[conformance] PASS {file_name} — {}", spec.name);
                }
            }
            Err(err) => failures.push(format!("{file_name} ({}): {err}", spec.name)),
        }
    }

    if debug {
        eprintln!(
            "[conformance] ran {ran} trigger case(s), {} failed",
            failures.len()
        );
    }

    assert!(
        failures.is_empty(),
        "{} of {} trigger conformance case(s) failed:\n  - {}",
        failures.len(),
        ran,
        failures.join("\n  - ")
    );
}

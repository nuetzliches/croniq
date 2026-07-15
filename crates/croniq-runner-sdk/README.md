# Croniq Runner SDK for Rust

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Build job execution runners for [Croniq](https://github.com/nuetzliches/croniq) in Rust. The SDK polls a Croniq server for work, dispatches async handlers, streams structured logs back, renews leases, and reports completion — with `tokio` throughout. It is also a **first-class producer**: the same crate ships a [`TriggerClient`](#triggering-jobs-on-demand-producer) that fires jobs on demand via `POST /v1/trigger`.

This is the reference SDK — the language-agnostic [conformance suite](../../sdks/conformance) is derived from its wire behaviour.

## Install

The crate lives in the Croniq workspace and is **not published to crates.io separately yet**. Depend on it by path or git:

```toml
# path (inside the workspace)
croniq-runner-sdk = { path = "crates/croniq-runner-sdk" }

# or git
croniq-runner-sdk = { git = "https://github.com/nuetzliches/croniq" }
```

Runtime deps are minimal: `reqwest` (rustls, no OpenSSL), `tokio`, `serde`, `chrono`, `tracing`, `uuid`, `thiserror`.

## Quick start (consumer / runner)

```rust
use croniq_runner_sdk::{CroniqRunner, ExecutionContext};

#[tokio::main]
async fn main() {
    let runner = CroniqRunner::builder("http://localhost:4000", "my-runner")
        .api_key("croniq_abc123")
        .capabilities(vec!["billing".into()])
        .max_inflight(5)
        .build();

    runner
        .register("billing:invoice", |ctx: ExecutionContext| async move {
            tracing::info!(execution_id = %ctx.execution_id, attempt = ctx.attempt, "processing");
            Ok(())
        })
        .await;

    runner.start().await.unwrap();
}
```

See [`examples/basic.rs`](examples/basic.rs) for a full template (including `register_with_schedule`, which self-registers the job via `POST /v1/jobs/register` at startup).

## Features

- **Async, `tokio`-native** — handlers are `async` closures; the runner drives up to `max_inflight` concurrent executions.
- **Server-side cancellation** — the poll response's cancel signal is wired into the handler's `ExecutionContext` so long-running work stops when the server cancels.
- **Streaming log writer** — `ctx.log_writer()` buffers events into a bounded channel; a flusher batches them and drains before ack. Use it when a handler wraps a chatty subprocess.
- **Lease renewal** — a per-execution task posts to `/v1/work/renew` until the handler returns.
- **Self-registration** — `register_with_schedule("billing:invoice", "5m", fn)` calls `POST /v1/jobs/register`; Croniqfile-managed jobs take precedence.
- **Catch-all handler** — `set_default_handler(...)` for runners that handle any `job_key` — see [`examples/catch_all.rs`](examples/catch_all.rs).
- **Persistent runner identity** — resolve a stable runner id from env / data-dir so a restarted runner keeps its identity.

## Capabilities vs Tags

Capabilities drive job routing (`require` / `prefer` in the Croniqfile); tags are filter-only (UI + operations). Don't put implementation details (`rust`, `linux-amd64`) into capabilities — use tags for those so a future runner in another language with the same business capabilities can take over without rewriting Croniqfile entries.

## Triggering jobs on demand (producer)

Runners are the *consumer* side. The *producer* side — firing a job immediately, e.g. from a web handler or a message consumer, in addition to the Croniqfile schedule — is a separate, first-class client, [`TriggerClient`](src/trigger.rs), that wraps `POST /v1/trigger`. The **same** registered handler serves both periodic and event-driven runs — one execution path, one observability path.

```rust
use croniq_runner_sdk::{TriggerClient, TriggerError};
use std::collections::HashMap;

let client = TriggerClient::builder("http://localhost:4000")
    .api_key("croniq_trigger_key") // jobs:trigger scope — not a runner poll key
    .build();

let result = client
    .trigger("billing:invoice")
    .metadata(HashMap::from([("invoice_id".into(), "inv_42".into())]))
    .require(vec!["billing".into()])
    .timeout("10m")
    .idempotency_key("evt-2026-07-14-001") // optional dedup key
    .send()
    .await;

match result {
    Ok(res) => tracing::info!(
        execution_id = %res.execution_id,
        queued = res.queued,
        deduplicated = res.deduplicated,
        "triggered",
    ),
    // Per-job queue-overflow backpressure: the job is at its max_queue_depth cap.
    Err(TriggerError::QueueOverflow { .. }) => { /* back off / retry later */ }
    Err(e) => tracing::error!(error = %e, "trigger failed"),
}
```

- **Separate credentials.** Triggering requires the `jobs:trigger` (or `admin`) scope, which runner poll keys typically don't carry — so `TriggerClient` takes its own API key / bearer token, fully independent of `CroniqRunner`.
- **Unset optionals are omitted.** `metadata`, `require`, `prefer`, `timeout`, and `idempotency_key` are left out of the JSON body entirely when unset (never sent as `null`) — the server applies its own defaults.
- **Idempotency.** Supply `idempotency_key` (≤ 200 chars, 10-minute dedup window by default) to dedup at-least-once producers. A repeat trigger with the same `(job_key, idempotency_key)` returns the *existing* `execution_id` with `deduplicated == true` instead of enqueuing again.
- **Backpressure.** When the job is at its `max_queue_depth` cap the server returns `429`, surfaced as the dedicated `TriggerError::QueueOverflow` variant (distinct from `TriggerError::Server`) so a batching producer can observe backpressure and slow down.

See [`examples/trigger.rs`](examples/trigger.rs) for a runnable producer.

## License

Licensed under either of [MIT](../../LICENSE-MIT) or [Apache-2.0](../../LICENSE-APACHE) at your option.

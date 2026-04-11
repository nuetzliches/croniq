# Croniq

Better Cron — a distributed job scheduling platform built in Rust.

## Features

- **Croniqfile DSL** — human-readable job scheduling configuration
- **Distributed execution** — runners poll for work via HTTP Pull-API
- **Calendar support** — include/exclude rules for business days, holidays
- **Retry policies** — exponential, linear, fixed backoff with jitter
- **Dead letter queue** — failed executions preserved for inspection and replay
- **Auth** — JWT tokens, API keys, password authentication
- **Dashboard** — React UI with real-time status
- **MCP Server** — AI assistant integration via Model Context Protocol
- **Metrics** — Prometheus-compatible `/metrics` endpoint
- **Hot-reload** — watch Croniqfile for changes, apply without restart
- **Queue overflow protection** — max 10 queued executions per job

## Quick Start

```sh
# Initialize the database and create an admin user
croniq init --data-dir ./.data --username admin --password changeme

# Start the server
croniq-server --config Croniqfile --data-dir ./.data

# With UI serving
croniq-server --config Croniqfile --data-dir ./.data --ui-dir ui/dist

# Or with Docker (auto-initializes on first run)
docker run -p 4000:4000 -e CRONIQ_ADMIN_PASSWORD=changeme croniq:latest
```

Open **http://localhost:4000** — login with `admin` / `changeme`.

## Croniqfile Example

```
server {
  listen :4000
  data_dir /var/lib/croniq
}

observability {
  metrics { listen :9900; path /metrics }
}

defaults {
  timezone Europe/Vienna
  retry exponential { max_attempts 3; base 2s; cap 30s }
  timeout 5m
}

calendar business-days {
  include weekly monday tuesday wednesday thursday friday
  exclude annual 01-01 12-25 12-26
}

job billing:invoice {
  every weekday at 02:00 { calendar business-days }
  runner { require billing }
  timeout 15m
}

job etl:sync {
  every 15 minutes
  timeout 10m
}
```

## Architecture

```
                 ┌──────────────────────────────────────┐
  Croniqfile ──► │           croniq-server               │
                 │                                       │
                 │  Scheduler ─► Queue ─► HTTP Pull-API  │
                 │  Watchdog  ─► Registry                │
                 │  Metrics   ─► /metrics                │
                 │  Auth      ─► JWT / API Keys          │
                 └───────────────────┬──────────────────┘
                                     │
                          Runner SDK (HTTP Poll)
                                     │
                              ┌──────┴──────┐
                              │   Runners   │
                              └─────────────┘
```

### Crates

| Crate | Description |
|---|---|
| `croniq-config` | DSL parser, compiler, formatter, validator |
| `croniq-scheduler` | Cron engine, calendar evaluation, trigger state machine |
| `croniq-store` | Persistence traits + SQLite/Postgres implementations |
| `croniq-execution` | Retry, timeout, dead-letter pipeline |
| `croniq-runner` | HTTP Pull-API server, registry, work queue |
| `croniq-bridge` | JobConfig to WorkItem translation |
| `croniq-auth` | JWT issuance, API key hashing, password auth |
| `croniq-server` | HTTP server with ~35 REST endpoints |
| `croniq-mcp` | MCP server for AI assistants |
| `croniq-cli` | CLI tool (validate, fmt, compile, init, status, ...) |
| `croniq-runner-sdk` | Client library for building runners |

## Server Flags

```
croniq-server [OPTIONS]

Options:
  -c, --config <PATH>     Croniqfile path [default: Croniqfile]
  -l, --listen <ADDR>     Listen address [default: :4000]
  -d, --data-dir <PATH>   SQLite data directory [default: ./.data]
      --metrics <ADDR>    Prometheus metrics endpoint (e.g. :9900)
      --watch             Hot-reload Croniqfile on changes
      --ui-dir <PATH>     Serve React UI static files from this directory
```

## Environment Variables

| Variable | Description | Default |
|---|---|---|
| `RUST_LOG` | Log level filter | `info` |
| `CRONIQ_JWT_SECRET` | JWT signing secret (fallback if not in Croniqfile) | random per-start |
| `CRONIQ_ADMIN_USER` | Admin username for Docker auto-init | `admin` |
| `CRONIQ_ADMIN_PASSWORD` | Admin password for Docker auto-init | `changeme` |
| `CRONIQ_ON_FAILURE_CMD` | Shell command to run on execution failure (dead-letter/drop) | none |

## REST API

All endpoints under `/v1/` require authentication (`Authorization: Bearer <jwt>` or `Authorization: ApiKey <key>`).

| Group | Endpoints |
|---|---|
| Auth | `POST /v1/auth/login`, `/refresh`, `/logout` |
| Jobs | `GET/POST /v1/jobs`, `GET/DELETE /v1/jobs/{key}`, `POST .../activate`, `POST /v1/jobs/register` |
| Schedules | `GET/POST /v1/schedules`, `GET/DELETE /v1/schedules/{id}` |
| Runners | `GET /v1/runners`, `DELETE /v1/runners/{id}` |
| Work | `POST /v1/work/poll`, `/ack`, `/renew`, `/{id}/events` |
| Executions | `GET /v1/executions`, `GET /v1/executions/{id}/logs` |
| Dead Letters | `GET /v1/dead-letters`, `GET/DELETE .../dead-letters/{id}`, `POST .../replay` |
| Calendars | `GET/POST /v1/calendars`, `GET/DELETE /v1/calendars/{id}` |
| Dashboard | `GET /v1/dashboard/forecast?window_minutes=60&bucket_minutes=5` |
| API Clients | `GET/POST /v1/api-clients`, `DELETE .../api-clients/{id}`, `POST .../tokens` |
| API Keys | `POST /v1/api-keys`, `DELETE /v1/api-keys/{id}` |
| Health | `GET /health` (public, no auth) |
| Metrics | `GET /metrics` (separate port via `--metrics` or observability config) |

## Runner SDK

```rust
use croniq_runner_sdk::{CroniqRunner, ExecutionContext};

#[tokio::main]
async fn main() {
    let runner = CroniqRunner::builder("http://localhost:4000", "my-runner")
        .api_key("croniq_abc123")
        .capabilities(vec!["billing".into()])
        .max_inflight(5)
        .build();

    // Register handler + schedule — server creates the job automatically
    runner.register_with_schedule("billing:invoice", "5m", |ctx: ExecutionContext| async move {
        println!("Processing invoice: {}", ctx.execution_id);
        Ok(())
    }).await;

    runner.start().await.unwrap();
}
```

## CLI

```sh
croniq quickstart                  # Zero-to-running: init + sample Croniqfile
croniq init --data-dir .data       # Seed admin user + API key
croniq validate Croniqfile         # Check for errors
croniq fmt Croniqfile --write      # Format in place
croniq compile Croniqfile          # Print compiled JSON
croniq convert '*/15 * * * *'     # Cron expression to DSL
croniq migrate crontab.txt -o Croniqfile  # Convert crontab to Croniqfile
croniq status                      # Live scheduler status
croniq list-runners                # Connected runners
croniq trigger billing:invoice     # Fire job immediately
croniq dead-letters --data-dir .   # List dead letters
croniq dead-letters-inspect <id>   # Full dead letter details
```

## Docker

```sh
# Run with auto-init
docker run -p 4000:4000 -e CRONIQ_ADMIN_PASSWORD=mysecret croniq:latest

# docker-compose
docker compose up

# Build locally
docker build -t croniq:latest .
```

## Development

```sh
# Build
cargo build --workspace

# Test
cargo test --workspace

# Dev mode (separate processes)
cd ui && npm run dev                                        # Vite on :5173
croniq-server --config Croniqfile.example --data-dir .data  # API on :4000

# Production mode (single process)
cd ui && npm run build
croniq-server --config Croniqfile.example --data-dir .data --ui-dir ui/dist
```

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT License ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

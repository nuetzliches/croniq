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

## Quick Start

```sh
# Initialize the database and create an admin user
croniq init --data-dir ./.data --username admin --password changeme

# Start the server
croniq-server --config Croniqfile --data-dir ./.data --listen :8080

# Or with Docker
docker run -p 8080:8080 -e CRONIQ_ADMIN_PASSWORD=changeme croniq:latest
```

## Croniqfile Example

```
server { listen :8080; data_dir /var/lib/croniq }

pull_api {
  auth token my-secret
  lease_ttl 60s
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

## REST API

All endpoints under `/v1/` require authentication (Bearer JWT or ApiKey header).

| Group | Endpoints |
|---|---|
| Auth | `POST /v1/auth/login`, `/refresh`, `/logout` |
| Jobs | `GET/POST /v1/jobs`, `GET/DELETE /v1/jobs/{key}` |
| Schedules | `GET/POST /v1/schedules`, `GET/DELETE /v1/schedules/{id}` |
| Runners | `GET /v1/runners`, `DELETE /v1/runners/{id}` |
| Work | `POST /v1/work/poll`, `/ack`, `/renew`, `/{id}/events` |
| Executions | `GET /v1/executions`, `GET /v1/executions/{id}/logs` |
| Dead Letters | `GET /v1/dead-letters`, `POST /v1/dead-letters/{id}/replay` |
| Calendars | `GET/POST /v1/calendars`, `GET/DELETE /v1/calendars/{id}` |
| Dashboard | `GET /v1/dashboard/forecast` |
| API Clients | `GET/POST /v1/api-clients`, `DELETE /v1/api-clients/{id}` |
| API Keys | `POST /v1/api-keys`, `DELETE /v1/api-keys/{id}` |
| Health | `GET /health` (public) |
| Metrics | `GET /metrics` (separate port) |

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

    runner.register("billing:invoice", |ctx: ExecutionContext| async move {
        println!("Processing invoice: {}", ctx.execution_id);
        Ok(())
    }).await;

    runner.start().await.unwrap();
}
```

## CLI

```sh
croniq validate Croniqfile       # Check for errors
croniq fmt Croniqfile --write    # Format in place
croniq compile Croniqfile        # Print compiled JSON
croniq init --data-dir .data     # Seed admin user + API key
croniq status --url :8080        # Live scheduler status
croniq list-runners --url :8080  # Connected runners
croniq trigger billing:invoice   # Fire job immediately
croniq dead-letters --data-dir . # List dead letters
```

## License

MIT

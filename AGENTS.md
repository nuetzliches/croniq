# Guidance for AI Assistants

## Core Expectations

1. **Target Stack** — Rust (latest stable edition), React + TypeScript for UI
2. **Language** — Documentation, commits, and code comments in English
3. **Dependencies** — MIT-compatible licenses only. Use latest stable versions.
4. **Code Style** — `cargo clippy` clean, `cargo fmt` formatted
5. **Testing** — Add tests for new functionality. `cargo test --workspace` must pass.
6. **Default Port** — `:4000` for the HTTP server (avoids Prometheus :9090 and common :8080)

## Architecture

- Workspace with 14 crates under `crates/`
- `croniq-server` is the main binary (HTTP server, scheduler, watchdog, metrics, UI serving)
- `croniq-cli` is the CLI tool
- `croniq-runner-sdk` is the client library for building runners
- `croniq-shell-runner` is the generic runner that executes `runner shell { … }` / `runner exec { … }` jobs as subprocesses
- `croniq-store` holds persistence traits + SQLite/Postgres implementations
- `croniq-auth` handles JWT, API keys, password auth
- UI is a React SPA under `ui/`

## Key Patterns

- Store traits are in `croniq-store/src/traits.rs` — implement for new backends
- API handlers are in `croniq-server/src/api/` — one module per domain
- Auth middleware in `croniq-server/src/api/auth_middleware.rs`
- DSL parser in `croniq-config` — lexer -> parser -> AST -> compiler
- Scheduler loop ticks every second in `croniq-server/src/scheduler.rs`
- Triggers return to Armed immediately after firing (async execution model)
- Queue overflow protection: max 10 queued executions per job key
- Internal metadata keys are prefixed with `__` (`__require`, `__prefer`, `__runner_exec`) — set by the scheduler / DSL compiler and consumed by runners; do not let user-supplied metadata clash with this namespace
- JWT secret resolved from: Croniqfile > CRONIQ_JWT_SECRET env > random fallback

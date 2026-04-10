# Guidance for AI Assistants

## Core Expectations

1. **Target Stack** — Rust (latest stable edition), React + TypeScript for UI
2. **Language** — Documentation, commits, and code comments in English
3. **Dependencies** — MIT-compatible licenses only. Use latest stable versions.
4. **Code Style** — `cargo clippy` clean, `cargo fmt` formatted
5. **Testing** — Add tests for new functionality. `cargo test --workspace` must pass.

## Architecture

- Workspace with 11 crates under `crates/`
- `croniq-server` is the main binary (HTTP server, scheduler, watchdog)
- `croniq-cli` is the CLI tool
- `croniq-runner-sdk` is the client library for runners
- `croniq-store` holds persistence traits + SQLite/Postgres implementations
- UI is a React SPA under `ui/`

## Key Patterns

- Store traits are in `croniq-store/src/traits.rs` — implement for new backends
- API handlers are in `croniq-server/src/api/` — one module per domain
- Auth middleware in `croniq-server/src/api/auth_middleware.rs`
- DSL parser in `croniq-config` — lexer → parser → AST → compiler
- Scheduler loop ticks every second in `croniq-server/src/scheduler.rs`

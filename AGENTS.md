# Guidance for AI Assistants

## Core Expectations

1. **Target Stack** — Rust (latest stable edition). The shipping dashboard is
   React + TypeScript and stays so until cutover, but it is being rebuilt in
   Vue 3 with a new design
   ([ADR-0004](docs/adr/0004-vue-rebuild-for-the-dashboard.md), superseding
   [ADR-0003](docs/adr/0003-react-for-the-dashboard.md)). Fixes to a screen
   that already exists in the Vue tree go there first.
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
- UI is a React SPA under `ui/`, served by `croniq-server --ui-dir` from the
  same origin as the API ([ADR-0001](docs/adr/0001-same-origin-dashboard.md),
  [ADR-0002](docs/adr/0002-single-image-delivery.md))

## Architecture decisions

Standing constraints that no single file owns are recorded under
[`docs/adr/`](docs/adr/README.md). Read the index before proposing a change to
delivery topology, auth-token storage, or the UI stack — each of those has an
ADR that names the alternatives already weighed.

Most decisions do **not** belong there. Reasoning goes in a comment at the
decision site, in the CHANGELOG entry for the change, or in `operations.md` for
operator-visible behaviour. `docs/adr/README.md` states the admission test.

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

## Security scanning conventions

- **CodeQL `rust/cleartext-logging` on public identifiers is a false-positive
  in this project.** Croniq logs identifiers like `runner_id`, `job_key`,
  `execution_id`, request paths via `tracing::*!` for operator visibility.
  These are not credentials — they are broadcast in every PollRequest, ack,
  log row, and UI display. CodeQL's heuristic flags any identifier-shaped
  string written to a log sink. Established response: dismiss as
  "false positive" via the GitHub API (`gh api -X PATCH
  repos/.../code-scanning/alerts/<n> -f state=dismissed -f 'dismissed_reason=false positive'`).
  Eight prior alerts were dismissed for this exact pattern as of v0.11.0.
- **CodeQL cannot see `#[cfg(test)]`.** Its Rust data-flow treats
  `assert!(cond, "{value:?}")` inside an inline `mod tests` as a log sink, and
  a fixture literal there as a hard-coded credential — so test modules keep
  regenerating `rust/cleartext-logging` and
  `rust/hard-coded-cryptographic-value` alerts. Path filters do not help;
  these modules live in the same file as the code they test. Before
  dismissing as `used in tests`, verify two things and say so in the comment:
  the flagged line is past the file's `#[cfg(test)]` marker (name the line
  number), and the tainted value reaches no `tracing!`/`println!` in the
  production half (grep it). Alerts 1-3, 16, 25-31, 37-39 were closed this
  way.
- **Genuine credentials must never be logged.** API keys, JWT secrets,
  passwords, OAuth tokens, signing keys — never print or log these in
  plain form. Init/quickstart roadmap items (`TTY-aware secret output`)
  cover the remaining sharp edges around credentials in stdout.

## Migration conventions

- Migrations live in `crates/croniq-store/src/migrations/NNN_name.sql` and
  are registered in numeric order in
  [`migrations/mod.rs`](crates/croniq-store/src/migrations/mod.rs). Numbers
  are monotonic; new ones pick the next free slot at PR-open time.
- When two migration PRs are in flight at once, the second one to merge
  rebases to bump its number above the first. Migrations are idempotent
  and additive (or use `IF NOT EXISTS` guards), so order shifts don't
  break data — but the embedded list must stay monotonic so the runner
  applies in deterministic order.
- Always include a unit test under `migrations::tests` that applies the
  migration against a hand-bootstrapped schema state and asserts the
  expected post-condition (see `migration_009_backfills_orphan_dead_executions`
  for the seed-then-apply pattern).

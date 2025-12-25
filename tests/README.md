## Testing Guidance

- Run the full solution suite via `pwsh ./scripts/run-tests.ps1 -Configuration Debug` from the repo root. The script wires required env vars (`CRONIQ_SQL`, coverage reporting, binlogs) and executes `dotnet test croniq.slnx`.
- `run-tests.ps1` only redirects `dotnet test` console output to `artifacts/ci/dotnet-test.console.log` when `CRONIQ_AGENT_TERMINAL=1` is set (intended for automated agents). Developer runs in VS Code keep streaming output by default.
- Every `run-tests.ps1` invocation also writes a quick `run-summary.txt` into the artifacts folder so you can resume debugging after a freeze.

# Croniq Test Harness

This folder contains all automated test suites that back the Croniq quality strategy described in `docs/deep-dive/testing.md`. Use this guide as a quick reference for running the suites locally and diagnosing failures.

## Prerequisites

- .NET SDK `10.0.x` on your PATH (`dotnet --version` should succeed).
- Docker Desktop (or any Docker-compatible engine) for suites that spin up SQL Server or the dev stack.
- PowerShell or CMD for Windows-specific helper scripts (the repo defaults to Windows paths in scripts).

## Quick Commands

```cmd
:: Unit + contract suites (fast)
dotnet test croniq.slnx --filter Category!=E2E

:: Observability smoke test (no Docker required)
dotnet test tests\Croniq.Observability.Tests\Croniq.Observability.Tests.csproj

:: API smoke / E2E (requires Docker)
scripts\test-e2e.cmd
```

## Suite Overview

| Suite / Project                                   | Description                                                                                                             | Trigger                          |
| ------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- | -------------------------------- |
| `tests/Croniq.Core.Tests`                         | Unit tests for scheduler, policies, and hosting extensions.                                                             | Run on every PR + local pre-push |
| `tests/Croniq.JobStore.InMemory.Tests`            | Unit tests covering the in-memory job store implementation.                                                             | Run on every PR                  |
| `tests/Croniq.Providers.Default.Tests`            | Unit/contract tests validating provider defaults and DI wiring.                                                         | Run on every PR                  |
| `tests/Croniq.Observability.Tests`                | Spins up an in-memory OTLP collector and verifies logs/metrics/traces reach it.                                         | Nightly workflow + before merges |
| `tests/Croniq.Persistence.SqlServer.Tests`        | Contract tests using Croniq.TestKit + Testcontainers to validate the SqlServer persistence layer.                       | Run on every PR (Docker needed)  |
| `tests/Croniq.Api.Smoke` + `scripts/test-e2e.cmd` | Docker Compose stack (API + Worker + SQL) plus HTTP smoke tests exercising `/health` + `/tenants/{tenantId}/schedules`. | Nightly + release readiness      |

All suites are regular `dotnet test` projects, so you can target any subset via `dotnet test <path> --filter ...`.

## Environment Variables

- `CRONIQ_API_BASEURL` (default `http://localhost:5080`)
- `CRONIQ_API_KEY` (default `smoke-key`)
- `COMPOSE_FILE`, `COMPOSE_PROFILES`, `COMPOSE_ARGS` (used by CI to layer the dev stack)

`tests/Croniq.Api.Smoke` and `scripts/test-e2e.cmd` respect the `CRONIQ_API_*` variables so you can point the smoke tests at a remote deployment without editing code.

## Troubleshooting Cheatsheet

- **Docker failures**: Ensure Docker Desktop is running, then re-run `docker compose ls` to verify connectivity. Ports `5080`, `9464`, and `4317` must be free before launching the stack.
- **OTLP collector unreachable**: The observability smoke test listens on `http://127.0.0.1:<ephemeral port>/`. Disable VPN software that blocks loopback HTTP calls if the test hangs waiting for telemetry.
- **SQL container reuse**: If contract tests complain about schema drift, run `docker compose -f infra/docker/docker-compose.tests.yml down -v` to wipe old volumes, then rerun the suite.
- **Verbose logs**: Pass `--logger:"console;verbosity=detailed"` to `dotnet test` for more context, or run `docker compose -f infra/docker/docker-compose.tests.yml logs -f` in another terminal while the smoke tests execute.

Need more detail? See `docs/deep-dive/testing.md` for the full strategy and CI wiring.

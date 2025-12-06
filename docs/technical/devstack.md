# Croniq Docker Dev Stack Plan

This document describes the local Docker Compose environment required to satisfy the checklist item "Docker Compose Dev-Stack (API, Worker, Xtraq, OTel/Grafana) bereitstellen". It ties together the API, scheduler workers, SQL/Xtraq database, and observability components for day-to-day development and smoke tests.

## Objectives

- Offer a one-command environment (`docker compose up`) that mirrors the production topology (API + worker + SQL + observability) for developers and CI smoke tests.
- Keep configuration discoverable via `.env` and `appsettings.Development.json`, with secrets injected through `.env.local` or user secrets.
- Reuse the same Compose definitions in nightly CI workflows and local troubleshooting to minimize drift.
- Provide helpers for seeding Xtraq schema/data and exposing dashboards/logs for debugging.

## Target Services

| Service                   | Purpose                                                     | Notes                                                             |
| ------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------------- |
| `api`                     | Hosts `Croniq.Api.SampleHost` (Minimal API + gRPC).         | Mounted source, hot reload via `dotnet watch` or pre-built image. |
| `worker`                  | Runs scheduler worker host (future `Croniq.Worker`).        | Shares code with API; processes triggers/jobs.                    |
| `rpc-sample`              | Optional sample RPC client (demonstrates SDK usage).        | Can be toggled via profile.                                       |
| `mssql-22`                | SQL Server 2022 with Xtraq schema + auth/persistence procs. | Uses persisted volume `croniq-mssql-data`.                        |
| `otel-collector`          | OpenTelemetry collector for logs/metrics/traces.            | Exports to Tempo/Prometheus/Grafana.                              |
| `grafana`                 | Observability dashboard with baked JSON dashboards.         | Depends on `prometheus` + `tempo`.                                |
| `prometheus`              | Scrapes metrics from API/worker and OTel collector.         | Stores TSDB in `prom-data` volume.                                |
| `tempo`                   | Stores traces from collector.                               | Local filesystem volume.                                          |
| `seq` / `loki` (optional) | Log aggregation for JSON logs.                              | Future toggle once log volume grows.                              |

All services share a `croniq-net` bridge network. Ports are exposed via `.env` defaults (e.g., API 5080/5081, Grafana 5601, Prometheus 9000, Tempo 3100).

## Compose Files & Profiles

- `infra/docker/docker-compose.yml`: base SQL container (already present).
- `infra/docker/docker-compose.dev.yml`: adds API, worker, RPC sample, OTel stack, default volumes, health checks.
- `infra/docker/docker-compose.observability.yml`: optional overlay enabling Grafana dashboards + Tempo/Prometheus/Seq (referenced by `observability.md`).
- Use Compose profiles (`api`, `worker`, `obs`) to allow lightweight setups (`docker compose --profile api up`). Nightly CI runs all profiles.

## Configuration & Secrets

- `.env.example` documents required variables (ports, SA password, API keys). Developers copy to `.env` and override sensitive entries in `.env.local` ignored by git.
- `Croniq.Api.SampleHost` reads configuration from `appsettings.Development.json` + environment variables injected via Compose (`CRONIQ__XTRAQ__CONNECTIONSTRING`, `CRONIQ__AUTH__MODE`, etc.).
- Provide helper script `infra/docker/init-xtraq.ps1` (or `.sh`) to apply SQL scripts via `sqlcmd` inside the container, wrapping `infra/sql/xtraq/apply.ps1`.

## Developer Workflow

1. `cd infra/docker`
2. `cp .env.example .env` (first run) and set secrets.
3. `docker compose -f docker-compose.yml -f docker-compose.dev.yml --profile api --profile worker up --build`
4. API available at `https://localhost:5081`, Grafana at `http://localhost:5601` (login `admin/admin`).
5. To tear down and remove volumes: `docker compose ... down -v`.
6. For hot reload, developers can mount source directories and run `dotnet watch` from within the container or locally against the services.

## CI Integration

- Nightly workflow (`ci-nightly.yml`) uses the same compose files with `--profile api --profile worker --profile obs`. Tests (`Croniq.Api.Smoke`) run after health checks pass.
- Logs (`docker compose logs --timestamps`) and metrics snapshots are uploaded as artifacts for debugging.
- Compose env also seeds sample tenants/jobs to support E2E tests.

## Backlog to Finish the Dev Stack Milestone

- [ ] Expand `.env.example` with ports, credentials, and docstrings; add `.env` to `.gitignore` if not already.
- [ ] Create `docker-compose.dev.yml` defining API, worker, RPC sample, and referencing shared build context or published images.
- [ ] Add observability overlay compose file + Grafana dashboards + Tempo/Prometheus volumes, aligning with `observability.md`.
- [ ] Provide helper scripts (`scripts/devstack-up.cmd`, `scripts/devstack-down.cmd`) wrapping the compose commands and health checks.
- [ ] Document workflow in `docs/consumer/quickstart.md` (how to run the dev stack) and link from `README.md`.
- [ ] Update CI workflow (`ci-nightly.yml`) to call the same compose stack for smoke tests.
- [ ] Ensure SQL initialization script runs automatically on first boot (entrypoint or helper container) so developers don't run manual apply steps.

Completing these tasks enables a reproducible dev/test environment and closes the checklist item.

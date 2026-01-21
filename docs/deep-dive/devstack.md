# Croniq Docker Dev Stack Plan

This document describes the local Docker Compose environment required to satisfy the checklist item "Docker Compose Dev-Stack (API, Worker, SqlServer, OTel/Grafana) bereitstellen". It ties together the API, scheduler workers, SQL Server database, and observability components for day-to-day development and smoke tests. It also tracks the transition to an Aspire-based devstack for local development.

## Objectives

- Offer a one-command environment (`docker compose up`) that mirrors the production topology (API + worker + SQL Server (default) + observability) for developers and CI smoke tests.
- Keep configuration discoverable via `.env` (AppHost/Compose) with sample-only overrides in `appsettings.Development.json`, and inject secrets through `.env.local` or user secrets.
- Reuse the same Compose definitions in nightly CI workflows and local troubleshooting to minimize drift.
- Provide helpers for running EF Core migrations (via `Croniq.DbMigrator`) and exposing dashboards/logs for debugging.

## Aspire Devstack (Preferred for Local Development)

- AppHost location: `tools/Croniq.Devstack.AppHost` (net10.0).
- Run locally: `dotnet run --project tools/Croniq.Devstack.AppHost`.
- The AppHost loads `.env` from the repo root when present; process env vars override file values.
- SQL Server runs in a container with host port `CRONIQ_SQL_HOST_PORT` (default 11433); connection strings use `localhost` rather than the Compose host name.
- Enable observability containers with `dotnet run --project tools/Croniq.Devstack.AppHost -- --profile obs`, `CRONIQ_DEVSTACK_PROFILES=--profile obs`, or `CRONIQ_DEVSTACK_OBS=true`.
- When the `obs` profile is enabled, AppHost maps the OTLP endpoint to `http://localhost:${CRONIQ_OTLP_GRPC_PORT}` if `.env` still targets `http://otel-collector:4317`.
- Aspire dashboard defaults to `http://localhost:18888` (`ASPIRE_DASHBOARD_PORT`) and uses `ASPIRE_DASHBOARD_OTLP_ENDPOINT_URL` (default `http://localhost:18889`) so it does not clash with the Grafana stack. Croniq telemetry continues to flow to the collector; point `CRONIQ_OBS_OTLP_ENDPOINT` to the dashboard OTLP endpoint if you want the Aspire UI to show spans/metrics.
- Caddy runs by default as the local TLS proxy for `*.croniq.local` (disable with `CRONIQ_DEVSTACK_CADDY=false`). Defaults: `api.croniq.local`, `dmz.croniq.local`, `hooks.croniq.local`, and `ui.croniq.local` on `CRONIQ_CADDY_HTTPS_PORT` (443). If port 443 is unavailable, set `CRONIQ_CADDY_HTTPS_PORT` (and `CRONIQ_CADDY_HTTP_PORT`) to alternate values such as 8443/8080.
- Caddy proxies to `CRONIQ_CADDY_UPSTREAM_HOST` (default `host.docker.internal`). On Linux, set it to `host-gateway` or your host IP if `host.docker.internal` is unavailable.
- The AppHost can launch the Angular UI via `npm start` (requires Node.js + npm). Disable with `CRONIQ_DEVSTACK_UI=false`.
- CI smoke checks run the Aspire AppHost; Docker Compose remains available as a fallback. Track progress in `CHECKLIST-ASPIRE.md`.

## Target Services

| Service          | Purpose                                              | Notes                                                             |
| ---------------- | ---------------------------------------------------- | ----------------------------------------------------------------- |
| `api`            | Hosts `Croniq.Sample.ApiHost` (Minimal API + gRPC).  | Mounted source, hot reload via `dotnet watch` or pre-built image. |
| `worker`         | Runs scheduler worker host (`Croniq.WorkerHost`).    | Shares code with API; processes triggers/jobs.                    |
| `rpc-sample`     | Optional sample RPC client (demonstrates SDK usage). | Can be toggled via profile.                                       |
| `mssql-22`       | SQL Server 2022 with Croniq schema + EF migrations.  | Uses persisted volume `croniq-mssql-data`.                        |
| `otel-collector` | OpenTelemetry collector for logs/metrics/traces.     | Exports to Tempo/Prometheus/Grafana.                              |
| `grafana`        | Observability dashboard with baked JSON dashboards.  | Depends on `prometheus` + `tempo`.                                |
| `prometheus`     | Scrapes metrics from API/worker and OTel collector.  | Stores TSDB in `prom-data` volume.                                |
| `tempo`          | Stores traces from collector.                        | Local filesystem volume.                                          |
| `loki`           | Log aggregation for OTLP logs routed via collector.  | Enabled with the `obs` profile; Grafana ships a Loki datasource.  |

All services share a `croniq-net` bridge network. Ports are exposed via `.env` defaults (e.g., API 5080, UI 5081, Grafana 5610, Prometheus 9090, Tempo 3200).

> **Logs in Grafana**: With Loki now part of the `obs` profile, the OpenTelemetry Collector's log pipeline exports directly to `loki:3100`. Grafana auto-loads a Loki datasource so the `Croniq Logs` dashboard (or ad hoc Explore views) can show `Croniq.Sample.ApiHost` / worker logs alongside traces and metrics. If Loki is disabled, Serilog still writes to the console, but Grafana will no longer list the datasource.

## Loki Tenant & Labels

- The observability overlay pins all log traffic to the Loki tenant `croniq-devstack`. The OTEL collector sends the tenant via `X-Scope-OrgID`, and Grafana's datasource is provisioned with the same header. If you change the tenant, update **both** `infra/docker/observability/otel-collector-config.yaml` and `infra/docker/observability/grafana/datasources/datasource.yml` (or override via environment variables) to keep Explore queries working.
- The collector now forwards key resource attributes as Loki labels: `service_name`, `service_instance`, `environment`, and `tenant`. They originate from `Croniq:Core` settings (environment, tenant ID, instance IDs) so containerized and host-based runs share the same taxonomy.
- Typical Explore query using the new labels:
  - `{tenant="croniq-devstack", environment="dev", service_name="Croniq.Api"}` for API-only logs.
  - `{tenant="croniq-devstack"} |= "ERROR"` for a quick severity filter.
  - Use the templated **Croniq Log Pulse** dashboard (`infra/docker/observability/grafana/dashboards/logs-overview.json`) for prebuilt panels (per-service volume, level mix, latest WARN/ERROR stream, trigger INFO feed, and failed job errors at a glance).
  - The job execution pipeline now emits structured log scopes (`croniq.job.*`, `croniq.trigger.*`) and Info/Error entries like `Trigger <JobKey> started/completed/failed`, so Loki queries can filter by tenant/environment, namespace/name, trigger id or initiator without string parsing.
- Grafana Explore defaults to the last hour. When onboarding or after restarts, increase the time range (e.g., _Last 6 hours_) to include previously ingested chunks.

## Compose Files & Profiles

- `infra/docker/docker-compose.yml`: base SQL container plus the `croniq-db-migrator` helper that auto-applies EF migrations once SQL is healthy.
- `infra/docker/docker-compose.dev.yml`: adds API + worker hosts (profiles `api` / `worker`) and optional RPC sample/helper containers.
- `infra/docker/docker-compose.observability.yml`: overlay enabling the observability toolchain (OTel Collector, Prometheus, Tempo, Grafana) behind the `obs` profile.
- The API/worker/migrator images are built from a shared multi-target Dockerfile (`infra/docker/Dockerfile.services`). Compose selects the right output with `build.target` (`api`, `worker`, `migrator`) so restore/build layers are reused across services.
- The helper scripts always load all three files, so adding `--profile obs` is enough to wire up Grafana/Tempo/Prometheus without custom compose commands.
- Use Compose profiles (`api`, `worker`, `obs`) to allow lightweight setups (`docker compose --profile api up`) during troubleshooting.

## Configuration & Secrets

- `.env.example` (root) now lists the required ports, database credentials, and Croniq defaults. Copy it to `.env`, adjust secrets, and Compose will pick the variables up automatically. `.env` stays ignored via `.gitignore`.
- Aspire AppHost uses `.env` as an input source and applies the same `CRONIQ_*` values to the API/worker/migrator resources.
- `Croniq.Sample.*` hosts still load `appsettings.Development.json`, but core connectivity (auth mode, persistence, connection strings, webhook routing) is driven by environment variables injected via AppHost/Compose. Keep sample-only tweaks (CORS, DMZ surface/ingress, job registry sync, auth toggles) in appsettings and sensitive overrides in `.env.local` or user secrets when running locally.
- Postgres is supported for local/dev environments by pointing `CRONIQ_DB_PROVIDER=Postgres` and `CRONIQ_POSTGRES_CONNECTION` at an external instance; the default Compose stack still uses SQL Server.
- The `croniq-db-migrator` service (defined in the base compose file) waits for `mssql-22` to report healthy status and then applies EF Core migrations using `CRONIQ_SQL_CONNECTION`. Compose derives that connection string from `CRONIQ_SQL_HOST`, `CRONIQ_SQL_DATABASE`, and `CRONIQ_SQL_PASSWORD` (with `sa` on port 1433). When troubleshooting, you can still run `docker compose run --rm croniq-db-migrator` or `dotnet run --project tools/Croniq.DbMigrator` manually after setting `CRONIQ_SQL_CONNECTION`.

## Developer Workflow

### Aspire (Preferred)

1. `cd <repo-root>`
2. `copy .env.example .env` (first run) and adjust secrets/ports as needed.
3. `dotnet run --project tools/Croniq.Devstack.AppHost` (add `-- --profile obs` to start Grafana/Tempo/Prometheus/Loki)
   - If you do not have Node.js installed, set `CRONIQ_DEVSTACK_UI=false` in `.env`.

### Local TLS (Caddy)

1. Add host entries (Windows: `C:\Windows\System32\drivers\etc\hosts`), or run `scripts\devstack-hosts.ps1` from an elevated PowerShell:
   - `127.0.0.1 api.croniq.local`
   - `127.0.0.1 dmz.croniq.local`
   - `127.0.0.1 hooks.croniq.local`
   - `127.0.0.1 ui.croniq.local`
2. Start the AppHost so the Caddy container creates its local CA.
3. Trust the Caddy root CA on the host (Windows example). If you run Caddy directly on the host, `caddy trust` installs the CA for you. (solves: net::ERR_CERT_AUTHORITY_INVALID)
   - Run an elevated PowerShell (Administrator).
   - `cd <repo-root>`
   - `New-Item -ItemType Directory -Path artifacts -Force`
   - `docker ps --filter "name=caddy" --format "{{.Names}}"`
   - `docker cp <caddy-container>:/data/caddy/pki/authorities/local/root.crt .\\artifacts\\caddy-root.crt`
   - `certutil -addstore -f Root .\\artifacts\\caddy-root.crt`
   - Shortcut: `scripts\devstack-import-caddy-cert.ps1` (also requires an elevated PowerShell).
   - Note: the Caddy container may log that `certutil` is missing; that warning is expected because it cannot update the Windows trust store from inside the container.
4. Update `.env` for UI/DMZ base URLs if you want to go through Caddy:
   - `CRONIQ_UI_API_BASEURL=https://api.croniq.local`
   - `CRONIQ_SAMPLE_DMZ_BASEURL=https://dmz.croniq.local`

### Docker Compose (Fallback)

1. `cd <repo-root>`
2. `copy .env.example .env` (first run) and adjust secrets/ports as needed.
3. `scripts\devstack-up.cmd [--profile obs]` ensures `.env` exists, loads all compose files, and polls `/health`. As soon as `mssql-22` is ready, `croniq-db-migrator` runs automatically so the schema is ready before the API/worker start. The API/worker profiles are implied; pass extra profiles (e.g., `obs`) explicitly.
4. `scripts\devstack-restart.cmd [--profile ...]` first calls `devstack-down --remove-orphans` with the same profiles, then replays `devstack-up` - useful when Docker networks/containers get stuck.
5. Send smoke traffic with `scripts\devstack-trigger-job.cmd [jobKey [initiatorTag]]`, which defaults to `samples:smoke` and tags the metadata `initiator` (default `devstack-script`) while posting to `/jobs/trigger` using `CRONIQ_API_KEY`.
6. Rotate webhook secrets without crafting raw HTTP calls by running `scripts\webhook-rotate-secret.ps1 -TenantId <id> -Environment <tag> -HookKey <key> [-ActivateInSeconds 900] [-GracePeriodSeconds 86400]`. The helper prints the activation window plus the new secret so you can stash it in your vault immediately.
7. The default OTLP target `http://otel-collector:4317` (gRPC) can be overridden via `CRONIQ_OBS_OTLP_ENDPOINT`; set `CRONIQ_OBS_OTLP_PROTOCOL=http` if you need to switch the sample hosts over to OTLP/HTTP. Both API and worker containers read these values via `Croniq:Observability:*` so telemetry can be pointed at alternative collectors. Logs, traces, and metrics all flow through the same OTLP endpoint; the collector now forwards logs to Loki, traces to Tempo, and metrics to Prometheus.
8. API available at `http://localhost:5080`, UI (dev server) at `http://localhost:5081`, Grafana at `http://localhost:5610` (login `admin/admin`) once the `obs` profile is enabled.
9. To tear down and remove volumes: `scripts\devstack-down.cmd [--profile ...] --volumes` (or call `docker compose ... down -v`).
10. For hot reload, developers can mount source directories and run `dotnet watch` from within the container or locally against the services.

## CI Integration

- Nightly + release workflows start `tools/Croniq.Devstack.AppHost` with the `obs` profile and wait for `/health` before running `Croniq.Api.Smoke`.
- AppHost stdout/stderr logs are captured as CI artifacts for debugging.
- Compose remains available for local troubleshooting or parity checks.

## Backlog to Finish the Dev Stack Milestone

- [x] Expand `.env.example` with ports, credentials, and docstrings; add `.env` to `.gitignore` if not already.
- [x] Create `docker-compose.dev.yml` defining API, worker, RPC sample, and referencing shared build context or published images.
- [x] Add observability overlay compose file + Grafana dashboards + Tempo/Prometheus volumes, aligning with `observability.md`.
- [x] Provide helper scripts (`scripts/devstack-up.cmd`, `scripts/devstack-down.cmd`) wrapping the compose commands and health checks.
- [x] Switch CI smoke workflows (nightly + release) to the Aspire AppHost and archive AppHost logs.
- [x] Ensure SQL initialization script runs automatically on first boot (entrypoint or helper container) so developers don't run manual apply steps.

Completing these tasks enables a reproducible dev/test environment and closes the checklist item.

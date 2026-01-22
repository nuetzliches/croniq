# Croniq Aspire Dev Stack

This document describes the Aspire-based dev stack for Croniq. The Aspire AppHost is the single supported entry point for local development and CI smoke checks.

## Goals

- One-command local environment (API + worker + database + optional observability).
- Consistent configuration via `.env` with process env overrides.
- Local TLS proxy for browser-friendly URLs and gRPC-Web.

## Quickstart

1. `copy .env.example .env` (first run) and adjust secrets/ports as needed.
2. `dotnet run --project tools/Croniq.Devstack.AppHost`.
3. Optional:
   - Observability profile: `dotnet run --project tools/Croniq.Devstack.AppHost -- --profile obs`.
   - Disable the UI: set `CRONIQ_DEVSTACK_UI=false` in `.env`.
   - Disable Caddy: set `CRONIQ_DEVSTACK_CADDY=false` in `.env`.

The Aspire dashboard defaults to `http://localhost:18888` (`ASPIRE_DASHBOARD_PORT`).

## Topology

- AppHost: `tools/Croniq.Devstack.AppHost` (net10.0).
- SQL Server container (default) with `Croniq.DbMigrator` run-before dependency.
- Sample hosts: `Croniq.Sample.ApiHost`, `Croniq.Sample.WorkerHost`, `Croniq.Sample.Dmz`.
- Optional UI (Node dev server) controlled by `CRONIQ_DEVSTACK_UI`.
- Optional observability stack (otel-collector, grafana, tempo, prometheus, loki) behind the `obs` profile.
- Caddy local TLS proxy for `api.croniq.local`, `dmz.croniq.local`, `hooks.croniq.local`, and `ui.croniq.local`.

## Configuration

- `.env` in the repo root is loaded automatically by the AppHost when present. Process env vars override file values.
- `.env.example` is the canonical list of required defaults and ports.
- Sample hosts still load `appsettings.Development.json`, but core connectivity (auth, persistence, webhook routing) is driven by env variables injected via the AppHost.
- Postgres is supported by setting `CRONIQ_DB_PROVIDER=Postgres` and `CRONIQ_POSTGRES_CONNECTION` (or `Croniq__Postgres__ConnectionString`).

## Local TLS (Caddy)

1. Add host entries (Windows: `C:\Windows\System32\drivers\etc\hosts`) or run `scripts\devstack-hosts.ps1` from an elevated PowerShell:
   - `127.0.0.1 api.croniq.local`
   - `127.0.0.1 dmz.croniq.local`
   - `127.0.0.1 hooks.croniq.local`
   - `127.0.0.1 ui.croniq.local`
2. Start the AppHost so the Caddy container creates its local CA.
3. Trust the Caddy root CA on the host (Windows example).
   - `New-Item -ItemType Directory -Path artifacts -Force`
   - `docker ps --filter "name=caddy" --format "{{.Names}}"`
   - `docker cp <caddy-container>:/data/caddy/pki/authorities/local/root.crt .\artifacts\caddy-root.crt`
   - `certutil -addstore -f Root .\artifacts\caddy-root.crt`
   - Shortcut: `scripts\devstack-import-caddy-cert.ps1` (elevated PowerShell required).

Caddy proxies to `CRONIQ_CADDY_UPSTREAM_HOST` (default `host.docker.internal`). On Linux, use `host-gateway` or a host IP if `host.docker.internal` is unavailable.

## Observability (obs profile)

- Enable via `--profile obs`, `CRONIQ_DEVSTACK_PROFILES=--profile obs`, or `CRONIQ_DEVSTACK_OBS=true`.
- Containers and configs are sourced from:
  - `infra/docker/observability/otel-collector-config.yaml`
  - `infra/docker/observability/prometheus.yaml`
  - `infra/docker/observability/tempo.yaml`
  - `infra/docker/observability/loki-config.yaml`
  - `infra/docker/observability/grafana/*`
  - `infra/monitoring/rules/*`
- Grafana defaults to `http://localhost:5610` (login `admin/admin`).
- Loki tenant defaults to `croniq-devstack` with labels `service_name`, `service_instance`, `environment`, and `tenant`.

## CI Smoke Checks

- Nightly and release workflows start the AppHost, wait for `/health`, run `tests/Croniq.Api.Smoke`, and upload logs from `artifacts/release-devstack`.


# Croniq Observability Plan

This document captures the logging, metrics, and tracing strategy for Croniq services and libraries. It operationalizes `CONCEPT.md` sections 7, 9, and 15 and defines the work required to close the "Observability" item in `CHECKLIST.md`.

## Objectives

- Use Serilog for structured logs with OpenTelemetry (OTel) export so operators have consistent telemetry across API, scheduler, and workers.
- Emit metrics and traces via `OpenTelemetry` SDK with OTLP exporters by default; allow vendors to plug in alternative exporters.
- Provide an out-of-the-box Docker Compose stack (OTel Collector + Grafana + Tempo/Prometheus) for local testing.
- Surface golden signals (latency, queue depth, misfires, policy events) and share dashboards/alerts as part of the docs.

## Logging

- **Library default**: `Croniq.Providers.Default` registers Serilog as the primary logger. Each app hosts `SerilogLoggerFactory` with sinks:
  - Console (JSON) for dev.
  - OpenTelemetry sink (`Serilog.Sinks.OpenTelemetry`) shipping to the collector.
  - Optional file sink for legacy deployments.
- **Enrichment**: add `TenantId`, `EnvironmentTag`, `JobKey`, and `CallerId` to the log scope when available. Sensitive fields (payloads, API keys) are redacted or hashed.
- **Correlation**: include `TraceId`/`SpanId` in every entry (Serilog `ActivityEnricher`). This aligns with gRPC/REST tracing.

## Metrics

- `Croniq.Core` exposes `Meter Croniq.Core` with instruments:
  - `counter cronijob.executions_total` (labels: tenant, jobKey, result).
  - `histogram cronijob.execution_duration_ms`.
  - `counter cronitrigger.misfires_total`.
  - `updowncounter cronijob.queue_depth` for scheduler backlog.
- API/gRPC layers provide HTTP request duration, active callers, and rate limit rejections via `Croniq.Api` meter.
- Export via OpenTelemetry Metrics (OTLP). Collector relays to Prometheus/Tempo/Grafana stack in dev; production can send to Azure Monitor, Datadog, etc.

## Tracing

- `Croniq.Core` uses `ActivitySource Croniq.Core.Scheduler`. Job execution pipeline creates spans: `TriggerLookup`, `PolicyEvaluation`, `JobExecute`. Propagate context through DI so handlers can log to the same span.
- API and gRPC endpoints start server spans (`Croniq.Api`) and attach attributes like `croniq.tenant_id`, `croniq.job_key`, and `croniq.caller_type`.
- Export via OTel OTLP. For debugging, enable console exporter via configuration.

## Collector & Local Stack

- Compose file `infra/docker/docker-compose.dev-observability.yml` (to be added) spins up:
  - `otel-collector` with pipelines for traces/logs/metrics → Prometheus, Tempo, Loki (optional).
  - `grafana` with preloaded dashboards (`infra/docker/grafana/dashboards/*.json`).
  - `tempo` (traces) and `prometheus` (metrics). Loki is optional for logs if not using OpenTelemetry logs.
- Developer workflow: `docker compose -f infra/docker/docker-compose.dev-observability.yml up -d`, set `OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317`. Provide script `scripts/obs-up.cmd`.

## Dashboards & Alerts

- **Dashboards** (Grafana JSON stored in repo):
  1. Scheduler Health: queue depth, trigger throughput, execution latency p50/p95/p99.
  2. API Gateway: request rate, latency, rate-limit rejections per tenant.
  3. Policy Events: retries, misfires, dead-letter count.
- **Alerts**: Document Prometheus rules (e.g., `cronijob_queue_depth > threshold for 5m`, `rate_limit_rejections > 0`). Include as YAML under `infra/monitoring/rules/`.

## Instrumentation Guidelines

- All Croniq services call `services.AddOpenTelemetry()` with instrumentation for ASP.NET Core, gRPC, and HttpClient. Libraries expose `ActivitySource`/`Meter` instances but avoid auto registration to keep host control.
- Jobs can inject `IJobExecutionContext.ActivitySource` for custom spans; document best practices in consumer docs.
- Provide helper extension `CroniqObservabilityExtensions.AddCroniqTelemetry(this OpenTelemetryBuilder builder)` that registers the default instrumentation, exporters, and resource attributes (service.name, service.version, deployment.environment, tenant when relevant).

## Backlog to finish Observability Milestone

- [ ] Introduce `CroniqObservabilityOptions` (logs, metrics, traces toggles, exporters) + `AddCroniqObservability` service extension.
- [ ] Wire Serilog (JSON console + OTel sink) into `Croniq.Api` and sample hosts; ensure `Croniq.Providers.Default` exposes enrichment hooks.
- [ ] Add `ActivitySource`/`Meter` usage in `Croniq.Core` (trigger worker, policy resolver, job pipeline) and `Croniq.Api` endpoints.
- [ ] Create Docker Compose observability stack with collector + Grafana + Tempo + Prometheus, plus helper scripts.
- [ ] Author Grafana dashboards (JSON) and Prometheus alert rules committed under `infra/monitoring`.
- [ ] Update `docs/consumer` quickstart/configuration with instructions for enabling telemetry exports and viewing dashboards.
- [ ] Add automated smoke tests verifying OTLP export (e.g., assert metrics appear in a test collector during CI).

When this backlog is complete, the "Observability" entry in `CHECKLIST.md` can be marked done.

# Croniq Observability Plan

This document captures the logging, metrics, and tracing strategy for Croniq services and libraries. It operationalizes the guidance recorded in `architecture.md` and defines the work required to close the "Observability" item in `CHECKLIST.md`.

## Objectives

- Use structured logs with OpenTelemetry (OTel) export so operators have consistent telemetry across API, scheduler, and workers (Serilog is enabled via `AddCroniqObservability` when logging is turned on).
- Emit metrics and traces via `OpenTelemetry` SDK with OTLP exporters by default; allow vendors to plug in alternative exporters.
- Provide an out-of-the-box Docker Compose stack (OTel Collector + Grafana + Tempo/Prometheus) for local testing.
- Surface golden signals (latency, queue depth, misfires, policy events) and share dashboards/alerts as part of the docs.

## Logging

- **Library default**: `Croniq.Providers.Default` relies on `ILoggerFactory` and standard `ILogger` scopes. When `AddCroniqObservability` enables logging, it configures Serilog with JSON console output and optional OTLP log export.
- **Enrichment**: add `TenantId`, `EnvironmentTag`, `JobKey`, and `CallerId` to the log scope when available. Sensitive fields (payloads, API keys) are redacted or hashed.
- **Correlation**: include `TraceId`/`SpanId` in every entry (Serilog `ActivityEnricher`). This aligns with gRPC/REST tracing.
- **Hosts**: `AddCroniqObservability` wires OpenTelemetry tracing/metrics and, when logging is enabled, configures Serilog for `Croniq.Api`, the worker, and the sample hosts. `Croniq.Api` and `Croniq.Webhooks` ship convenience wrappers (`AddCroniqApiObservability`, `AddCroniqWebhookObservability`) that call the shared helper with their default tracing/meter wiring.
- **Hosts**: call `services.AddCroniqObservability(configuration, loggingBuilder, "<service>")` (or the service-specific wrappers) to provision OpenTelemetry exporters plus optional Serilog logging; `Croniq.Api` and both sample hosts already use these helpers.
- **Structured job scope**: `DefaultJobExecutionPipeline` wraps every job execution with logging scopes that emit `croniq.job.key`, `.namespace`, `.name`, optional `.variant`, as well as `croniq.tenant_id`, `croniq.environment`, `croniq.trigger.id`, and `croniq.trigger.initiator`. Loki and Grafana queries (Log Pulse dashboard) rely on these fields for tenant-safe filtering and INFO/ERROR panels.
- **Logging defaults and noise suppression**: use `MinimumLevelOverrides` to keep framework noise down while retaining Croniq lifecycle logs at `Information`. Recommended defaults:

  ```jsonc
  {
    "Croniq": {
      "Observability": {
        "MinimumLevelOverrides": {
          "Microsoft.EntityFrameworkCore.Database.Command": "Warning",
          "Microsoft.Hosting.Lifetime": "Warning",
          "Microsoft.AspNetCore.Hosting.Diagnostics": "Warning",
          "Microsoft.AspNetCore.Mvc.Infrastructure.DefaultActionDescriptorCollectionProvider": "Warning"
        }
      }
    }
  }
  ```

  This suppresses verbose EF command and ASP.NET host/controller discovery chatter while keeping Croniq lifecycle logs (job/worker start-stop, policy transitions) on `Information` for operators.

- **Structured logging guidelines**:
  - Always include tenant/environment/instance identifiers and the relevant domain key: `croniq.tenant_id`, `croniq.environment`, `croniq.instance_id` (worker), plus `croniq.job.key` or `croniq.hook.key` depending on context.
  - Keep lifecycle and externally visible state changes on `Information` (job start/complete, worker start/stop, policy retries/circuit transitions). Use `Debug/Trace` for polling/heartbeat noise; reserve `Warning/Error` for degradation and faults.
  - Use structured templates; avoid embedding payloads or secrets. Prefer opaque IDs or hashes for payload-derived values.
  - When adding new loggers, align `SourceContext` with the namespace and ensure the scope carries the standard fields above so Grafana/Loki filters continue to work.
- **Quick noise check (devstack)**: start the devstack with obs profile, hit a sample endpoint, and tail logs. With the overrides above, you should not see `Hosting.Diagnostics`, EF SQL, or MVC action-descriptor info at `Information`; Croniq lifecycle messages should remain visible.

## Metrics

- `Croniq.Core` exposes `Meter Croniq.Core` with instruments:
  - `counter cronijob_executions_total` (labels: tenant, job, result).
  - `histogram cronijob_execution_duration_ms`.
  - `counter cronitrigger_misfires_total` and `counter cronitrigger_quota_reschedules_total`.
  - `updowncounter cronijob_queue_depth` for scheduler backlog.
- `Croniq.Api` publishes `cronigateway_schedule_upserts_total` and `cronigateway_manual_triggers_total` so we can observe consumer activity.
- API/gRPC layers provide HTTP request duration, active callers, and rate limit rejections via `Croniq.Api` meter.
- Export via OpenTelemetry Metrics (OTLP). Collector relays to Prometheus/Tempo/Grafana stack in dev; production can send to Azure Monitor, Datadog, etc.

## Tracing

- `Croniq.Core` uses `ActivitySource Croniq.Core.Scheduler`. Job execution pipeline creates spans: `TriggerLookup`, `PolicyEvaluation`, `JobExecute`. Propagate context through DI so handlers can log to the same span.
- API and gRPC endpoints start server spans (`Croniq.Api`) and attach attributes like `croniq.tenant_id`, `croniq.job_key`, and `croniq.caller_type`.
- Export via OTel OTLP. For debugging, enable console exporter via configuration.

## Collector & Local Stack

- Compose overlay `infra/docker/docker-compose.observability.yml` (loaded automatically by the devstack helper scripts) spins up:
  - `otel-collector` with pipelines for traces/logs/metrics → Prometheus, Tempo, Loki (optional).
  - `grafana` with preloaded dashboards (`infra/docker/observability/grafana/dashboards/*.json`).
  - `tempo` (traces) and `prometheus` (metrics). Loki is optional for logs if not using OpenTelemetry logs.
- Developer workflow: `scripts\devstack-up.cmd --profile obs` (or manual `docker compose` with all three files) and set `OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317` (default gRPC) for local SDK processes that emit telemetry outside of Docker; swap to `http://localhost:4318` + `OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf` only when an HTTP collector is required.

## Verification Checklist (Local)

1. `scripts\devstack-up.cmd --profile obs` ensures the base services (API, worker, SQL) plus `otel-collector`, `prometheus`, `tempo`, and `grafana` are up. Expose the OTLP gRPC endpoint via `CRONIQ_OTLP_GRPC_PORT` when you need to point external processes at the collector.
2. Tail `docker compose -f infra/docker/docker-compose.yml -f infra/docker/docker-compose.dev.yml -f infra/docker/docker-compose.observability.yml logs otel-collector -f` and confirm the pipelines report `Exporter started` with no errors.
3. Generate telemetry:

   - Hit the API health endpoint: `curl http://localhost:5080/health` repeatedly to produce request traces/metrics.
   - Trigger sample jobs via `scripts\devstack-trigger-job.cmd` (defaults to `default:dev:samples:smoke`) so the worker emits spans and Serilog logs.

4. Check Grafana at `http://localhost:5610` (defaults `admin/admin`). The provisioned data sources (`Prometheus`, `Tempo`) should show as healthy; open the Scheduler dashboard to verify `cronijob_executions_total` increments.
5. Switch to the "Croniq Log Pulse" dashboard (from `infra/docker/observability/grafana/dashboards/logs-overview.json`), select tenant `croniq-devstack`, and confirm INFO lines arrive for the triggered jobs while the "Failed Job Errors" panel stays quiet unless you provoke failures.
6. Validate traces in Tempo via the Grafana Explore tab (select Tempo data source, search for `service.name="Croniq.Api"`).
7. Optional: `curl http://localhost:9090/api/v1/targets` should list the OTel collector scrape target as `up == 1`. Use this to ensure Prometheus continues to ingest metrics even before Grafana visualizes them.

### Automated Smoke Tests

- `dotnet test tests/Croniq.Observability.Tests/Croniq.Observability.Tests.csproj` spins up an in-memory OTLP HTTP collector and exercises `AddCroniqObservability`. The test emits a span, metric, and log and fails if any signal fails to reach the collector, catching exporter wiring regressions early. Add it to CI whenever you touch `CroniqObservabilityOptions`/`CroniqObservabilityExtensions`.

## Dashboards & Alerts

- **Dashboards**: Grafana auto-loads JSON from `infra/docker/observability/grafana/dashboards/` via `grafana/provisioning/dashboards/dashboards.yml`. Dashboards refresh every 30s and point at the provisioned `prometheus`, `tempo`, and `loki` data sources.
  1. `logs-overview.json` (Croniq Log Pulse) visualizes Loki logs per tenant: trigger INFO lines, long-running jobs, and the dedicated "Failed Job Errors" panel that highlights `LogError` events emitted by the job pipeline.
  2. `scheduler.json` visualizes execution throughput, p50/p95 latency, queue depth, and trigger anomalies. Use it to confirm scheduler health before promoting releases.
  3. `api-gateway.json` surfaces schedule upserts, manual triggers, and policy outcomes split by tenant so customer usage patterns are obvious.
  4. To enable them outside the devstack, mount the dashboards + provisioning folders into your Grafana deployment and keep the datasource UIDs (`prometheus`, `tempo`, `loki`) consistent or update the JSON accordingly.
- **Alerts**: Prometheus loads rules from `infra/monitoring/rules/scheduler-alerts.yaml` (mounted via docker-compose). The rule file defines:
  - `CroniqDeadLettersHigh`: warning when dead letters are emitted for 2m.
  - `CroniqMisfireBurst`: warning when misfires exceed 5/min.
  - `CroniqJobFailures`: critical for any job failures.
  - `CroniqQueueDepthHigh`: warning when the queue depth average stays above 100 for 5m.
  - `CroniqLatencyP95High`: warning when job execution p95 is higher than 2s for 5m.
    Point Prometheus at `/etc/prometheus/rules/*.yml` (already set in `observability/prometheus.yaml`) or copy the rule file into your existing Prometheus deployment. Alerts include runbook context in the annotations so on-call engineers know how to react.

## Instrumentation Guidelines

- All Croniq services call the shared `AddCroniqObservability` helper (wrapping `AddOpenTelemetry`) with instrumentation for ASP.NET Core, gRPC, and HttpClient. Libraries expose `ActivitySource`/`Meter` instances but avoid auto registration to keep host control. When you host specific surfaces, prefer the package helpers (`AddCroniqApiObservability`, `AddCroniqWebhookObservability`) so Croniq sources/meters (`Croniq.Api.Trigger`, `Croniq.Webhooks.Ingress`, etc.) are registered automatically; both helpers accept an existing `OpenTelemetryBuilder` so mixed hosts (API + Webhooks) reuse a single exporter pipeline.
- Jobs can inject `IJobExecutionContext.ActivitySource` for custom spans; document best practices in consumer docs.
- `CroniqObservabilityExtensions.AddCroniqObservability(...)` registers the default instrumentation, OpenTelemetry exporters, and resource attributes (service.name, version, deployment.environment, tenant). When logging is enabled, Serilog sinks are configured alongside the exporters.
- Sample hosts (`Croniq.Sample.ApiHost`, `Croniq.Sample.WorkerHost`) and `Croniq.Api` already call the helper and respect `Croniq:Observability:*` env overrides (defaulting to the collector at `http://otel-collector:4317`).

## Backlog to finish Observability Milestone

- [x] Introduce `CroniqObservabilityOptions` (logs, metrics, traces toggles, exporters) + `AddCroniqObservability` service extension.
- [x] Wire Serilog (JSON console + OTel sink) into `Croniq.Api` and sample hosts; ensure `Croniq.Providers.Default` exposes enrichment hooks.
- [x] Add `ActivitySource`/`Meter` usage in `Croniq.Core` (trigger worker, policy resolver, job pipeline) and `Croniq.Api` endpoints.
- [x] Create Docker Compose observability stack with collector + Grafana + Tempo + Prometheus, plus helper scripts.
- [x] Author Grafana dashboards (JSON) and Prometheus alert rules committed under `infra/monitoring`.
- [x] Update this document with instructions for enabling telemetry exports and viewing dashboards.
- [x] Add automated smoke tests verifying OTLP export (e.g., assert metrics appear in a test collector during CI).

All backlog items are complete; the "Observability" entry in `CHECKLIST.md` can be marked done.

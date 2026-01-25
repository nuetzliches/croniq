# KPI Checklist (Croniq UI)

> Scope: Define the planned KPI tiles in the shell and dashboard (Cluster Health, Queue Depth, Clock Δ, plus optional additions).
> Owner: UI (Croniq.Ui) in collaboration with API/Hosting.

## 1) KPI Definitions (Target Semantics)

- [ ] **Cluster Health**: Aggregated status across API, Worker, Scheduler, Storage, and Provider subsystems.
  - [ ] Decide aggregation rule (e.g., worst-state wins, weighted severity).
  - [ ] Define status levels (Healthy/Degraded/Unhealthy) and mapping rules.
- [ ] **Queue Depth**: Total pending work across schedulers/runners/dispatchers (not only workers).
  - [ ] Define whether retries/deadletters are included.
  - [ ] Define the time window (instant, last N minutes, or both).
- [ ] **Clock Δ**: Time drift between UI client and API server.
  - [ ] Confirm source of truth (API response header, server time endpoint).
  - [ ] Define threshold bands for warnings (e.g., >100 ms warn, >500 ms critical).

## 2) Optional KPI Candidates (If Feature is Available)

- [ ] **Active Jobs** (running now)
- [ ] **Scheduled Jobs** (next 24h)
- [ ] **Failed Jobs** (last 24h)
- [ ] **Throughput** (jobs/min)
- [ ] **Execution Latency** (p95/p99)
- [ ] **Worker/Node Count**

## 3) Data Sources & API Questions

- [ ] **Health**: Is there a single aggregated health endpoint, or only per-service `/health` endpoints?
  - [ ] If only per-service, which endpoints are authoritative (API, WorkerHost, WebhooksHost, Persistence)?
- [ ] **Queue Depth**: Which component owns queue metrics (Scheduler, Runner, Provider, Webhooks)?
  - [ ] Are there existing counters/metrics (OpenTelemetry) we can read?
- [ ] **Clock Δ**: Is server time exposed? If not, add a lightweight endpoint (e.g., `/api/system/time`).
- [ ] **Metric Cardinality**: Do we need tenant scoping? If yes, which tenant context is required?
- [ ] **Security**: Which auth scopes are needed to read KPI metrics?

## 4) Transport Protocol Decision (Client → API)

**Decision order (fixed):** gRPC → SSE → Polling

- [ ] **gRPC (preferred)**: Use gRPC streaming (gRPC‑Web in browser) for live KPI updates.
  - [ ] Confirm gRPC‑Web availability and gateway.
  - [ ] Define streaming interval / heartbeat and backpressure handling.
- [ ] **SSE (fallback)**: Server‑sent events for push updates when gRPC‑Web is unavailable.
  - [ ] Define event names and payload schema.
  - [ ] Define reconnect/backoff strategy.
- [ ] **Polling (last resort)**: Periodic fetch for KPI snapshot.
  - [ ] Define polling interval and cache behavior.
  - [ ] Define UI loading/empty states.

## 5) UX Requirements

- [ ] Consistent label casing and unit formatting (ms, %, count).
- [ ] Tooltip definitions for each KPI (source, window, thresholds).
- [ ] Status color mapping aligned with system intents (success/warn/neutral/error).

## 6) Dashboard KPI Coverage

- [ ] Define which KPIs appear on the dashboard vs. shell header.
- [ ] Confirm grouping/ordering on the dashboard (overview row vs. detailed sections).
- [ ] Decide whether dashboard KPIs reuse the same data stream or have separate granularity.
- [ ] Ensure dashboard KPI cards have empty/loading/error states consistent with shell.

## 7) Implementation Notes (UI)

- [ ] Replace static `statusCards` data source in `Shell` with a KPI service.
- [ ] Add loading/empty states (skeletons or placeholders).
- [ ] Add tests for KPI formatting and status mapping.

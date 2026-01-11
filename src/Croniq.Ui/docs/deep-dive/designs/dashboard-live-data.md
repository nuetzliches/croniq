# Dashboard Live Data Concept

This note outlines how the dashboard should evolve from polling to live updates without adding heavy client dependencies.

## Goals

- Keep operational views fresh without overwhelming the API.
- Default to polling with explicit refresh; streaming is opt-in once the backend supports it.
- Avoid client-side caching unless explicitly configured per resource (`tenantRxResource` cache hook).

## Phase 1: Polling Baseline

- Use `tenantRxResource` for dashboard slices; no cache by default.
- Suggested cadence (per slice):
  - Presence (workers/runners): 15-30s
  - Dead letters: 60s
  - Schedules summary: 60-120s
  - Heavy grids: manual refresh only
- `dashboard.refresh` fans out to existing endpoints until a single aggregate endpoint exists.
- UI shows a "last updated" timestamp and keeps the last known values on transient failures.

## Phase 2: Server Streaming

- Prefer Server-Sent Events (SSE) as the first streaming transport.
- Proposed endpoint: `GET /tenants/{tenantId}/dashboard/stream?environment={tag}`
- Event envelope:
  - `event`: `presence.updated`, `webhooks.updated`, `deadletters.updated`, `schedules.updated`
  - `data`: JSON patch for the affected section
  - `id`: monotonic sequence for resume (`Last-Event-ID`)
- Client integration:
  - Use `EventSource` (no extra deps), wrap into RxJS `fromEvent`.
  - Tear down on tenant/environment change or logout (`AbortController`).
  - Backoff reconnect with jitter; fall back to polling when the stream is unavailable.

## Security and Observability

- Reuse existing auth headers/cookies; never embed secrets in query params.
- Record stream lifecycle (connect/disconnect/errors) once UI telemetry hooks exist.
- Keep payloads minimal and avoid streaming secrets or sensitive data.

## Open Questions

- Do we introduce a dedicated dashboard aggregate endpoint before streaming?
- Which sections need real-time updates vs low-frequency polling?
- Do we need a "live/paused" toggle per operator session?

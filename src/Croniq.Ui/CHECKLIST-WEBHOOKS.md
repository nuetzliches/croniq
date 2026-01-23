# Croniq UI Webhooks Checklist

Purpose: track the remaining work to expand the Webhooks UI with an activity timeline and chart fed by backend data.

## Scope and Backend Contract

- [x] Create the backend source for timeline data (stream + aggregation endpoints) and document its SLA.
- [x] Define the timeline event schema (timestamp, hookKey, status, latency, requestId, environment, payload size).
- [x] Define the chart aggregation schema using backend standard defaults (bucket size, total count, warning/error counts, p95 latency).
- [x] Add/extend OpenAPI definitions for the timeline + chart endpoints and regenerate UI schemas.

## Remote Mode Visibility (Concept)

- [ ] Surface a "Remote" delivery mode badge/row only when the backend reports Mode = Remote.
- [ ] Display the configured remote base URL in the UI (from the ApiHost contract, not from env vars).
- [ ] Add an ApiHost endpoint that proxies the remote `/health` check (browser must not call the DMZ directly).
- [ ] Show health status + last checked timestamp in the UI, with a manual refresh action.
- [ ] Define UX for failure states (timeout, unreachable, auth error) with concise guidance.

## Transport (Stream vs Poll)

- [x] Decide on transport: prefer gRPC (via gRPC-Web/proxy) -> SSE fallback -> polling fallback.
- [ ] Confirm gRPC-Web/HTTP2 proxy support for browser clients (SSE fallback now available; tracked in `src/Croniq.Ui/BACKLOG.md`).
- [ ] Document reconnect/backoff rules and max staleness acceptable for the UI.
- [x] Ensure auth headers and tenant/environment filters are supported by gRPC/SSE/polling.

## Data Access and State

- [x] Add data-access endpoints for timeline + chart; align response parsing with Zod schemas.
- [x] Drive data fetching from filter signals using `linkedSignal()` and `switchMap()`.
- [x] Recreate/terminate the stream when filters change and on route teardown.
- [ ] Cache only when needed; avoid stale data across tenant/environment switches.

## UI: Timeline

- [x] Drop the virtualized timeline list (chart + detail panel instead).
- [ ] Show status + delivery outcome (success, retry, failure) and surface key metadata.
- [ ] Provide a compact empty state and a clear "filters applied" indicator.

## UI: Chart

- [x] Choose chart library: ECharts with a custom Angular wrapper (no third-party ng\* wrapper); Apache-2.0 license confirmed.

### UX Concept (Activity Timeline)

Goal: make webhook activity trends easier to read, more precise over time ranges, and stable under live updates.

- Default visualization should optimize for precision (trend/health) over composition.
- Bucketing must be stable (aligned to time boundaries) to avoid visual jitter.

Concept decisions:

- Prefer a time axis (`xAxis.type = 'time'`) over category axis labels.
- Default view: **Volume + Error rate**
  - Bar: total deliveries/events per bucket.
  - Line: failed rate per bucket (optionally failed+warning rate).
  - Tooltip: counts per status + totals + rate.
- Optional secondary view: **Distribution** (stacked status bars) for composition analysis.
- Add `dataZoom`:
  - `inside` zoom (scroll/pinch) always.
  - `slider` zoom when bucket count exceeds the comfortable label threshold.

Implementation checklist:

- [x] Switch chart to time axis and ensure tooltip/index mapping still works.
- [x] Align bucket boundaries (`floor/ceil` by bucketMs) to stabilize buckets and labels.
- [x] Add `dataZoom` support in the ECharts wrapper (register component) and in chart options.
- [x] Implement Volume + Error rate as the default series.
- [x] Add a view toggle for Distribution (stacked) vs Volume+Rate.
- [x] Ensure legend and accessible summaries match the active view.
- [x] Keep chart usable in compact height (avoid zoom slider crowding; adjust grid as needed).

## Filters Panel Integration

- [x] Define filter model (time range, hookKey, jobKey, environment); keep status filter local-only if needed.
- [x] Normalize filters into a single query object for both timeline + chart endpoints.
- [ ] Ensure filter changes update the stream without UI flicker.

## Telemetry and Error Handling

- [x] Track stream connection state (connected, retrying, offline).
- [ ] Emit telemetry for reconnects, errors, and dropped events.
- [x] Provide retry UI and "pause live updates" control.

## Testing

- [ ] Unit tests for filter -> query mapping.
- [ ] Unit tests for stream lifecycle (start, reconnect, stop).
- [x] Snapshot or unit tests for chart config generation.

## Docs

- [x] Update `docs/deep-dive/ui.md` with timeline data source, transport, and filters.
- [ ] Record any backend contract decisions in `docs/deep-dive/auth.md` or relevant webhook docs.

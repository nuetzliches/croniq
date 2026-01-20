# Croniq UI Webhooks Checklist

Purpose: track the remaining work to expand the Webhooks UI with an activity timeline and chart fed by backend data.

## Scope and Backend Contract

- [ ] Create the backend source for timeline data (stream + aggregation endpoints) and document its SLA.
- [ ] Define the timeline event schema (timestamp, hookKey, status, latency, requestId, environment, payload size).
- [ ] Define the chart aggregation schema using backend standard defaults (bucket size, total count, error count, p95 latency).
- [ ] Add/extend OpenAPI definitions for the timeline + chart endpoints and regenerate UI schemas.

## Transport (Stream vs Poll)

- [ ] Decide on transport: prefer gRPC (via gRPC-Web/proxy) -> SSE fallback -> polling fallback.
- [ ] Confirm gRPC-Web/HTTP2 proxy support for browser clients; otherwise start with SSE -> polling.
- [ ] Document reconnect/backoff rules and max staleness acceptable for the UI.
- [ ] Ensure auth headers and tenant/environment filters are supported by gRPC/SSE/polling.

## Data Access and State

- [ ] Add data-access endpoints for timeline + chart; align response parsing with Zod schemas.
- [ ] Drive data fetching from filter signals using `linkedSignal()` and `switchMap()`.
- [ ] Recreate/terminate the stream when filters change and on route teardown.
- [ ] Cache only when needed; avoid stale data across tenant/environment switches.

## UI: Timeline

- [ ] Implement a virtualized timeline list (group by time buckets, sticky date labels).
- [ ] Show status + delivery outcome (success, retry, failure) and surface key metadata.
- [ ] Provide a compact empty state and a clear "filters applied" indicator.

## UI: Chart

- [ ] Choose chart library: ECharts with a custom Angular wrapper (no third-party ng* wrapper); verify license compatibility.
- [ ] Build a minimal timeseries chart (line or area) for total vs error counts.
- [ ] Add a tooltip that mirrors the timeline bucket metadata.
- [ ] Provide an accessible summary table for screen readers.

## Filters Panel Integration

- [ ] Define filter model (time range, hookKey, jobKey, environment); keep status filter local-only if needed.
- [ ] Normalize filters into a single query object for both timeline + chart endpoints.
- [ ] Ensure filter changes update the stream without UI flicker.

## Telemetry and Error Handling

- [ ] Track stream connection state (connected, retrying, offline).
- [ ] Emit telemetry for reconnects, errors, and dropped events.
- [ ] Provide retry UI and "pause live updates" control.

## Testing

- [ ] Unit tests for filter -> query mapping.
- [ ] Unit tests for stream lifecycle (start, reconnect, stop).
- [ ] Snapshot or unit tests for chart config generation.

## Docs

- [ ] Update `docs/deep-dive/ui.md` with timeline data source, transport, and filters.
- [ ] Record any backend contract decisions in `docs/deep-dive/auth.md` or relevant webhook docs.

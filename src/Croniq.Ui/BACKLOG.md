# BACKLOG

_Last updated: 2026-01-24_

## Webhooks UI

- Action log panel showing recent admin operations from the UI.
- Telemetry data UX: define status/latency/throughput KPIs, data freshness/staleness rules, and UI fallbacks while telemetry is unavailable.
- Expose delivery attempts in webhook activity API so the UI can show retries.
- Telemetry for activity stream: emit events for reconnects, errors, and dropped updates in the UI.
- Tests: filter → query mapping for webhook activity.
- Tests: stream lifecycle (start, reconnect, stop) for webhook activity.
- Remote mode UI: surface Remote mode badge when backend reports Remote.
- Remote mode UI: show configured remote base URL and proxy health status with refresh + failure guidance.
- Transport: confirm gRPC-Web/HTTP2 proxy support for browser clients.
- Transport: document reconnect/backoff rules and max UI staleness.
- Data access: avoid stale caches across tenant/environment switches.
- Bulk enable/disable endpoints with confirmation and audit context.
- Grafana deep-links or embedded panels where available.
- Audit summary for rotations, IP rule changes, and failed deliveries.

## Webhooks Backend

- Telemetry-backed aggregates for webhook KPIs (Grafana URL or a dedicated API surface).
- Activity warning semantics: start with `delivered after retry` (status=warning); other options to consider are latency-over-threshold, degraded delivery/backlog, or partial acceptance signals.

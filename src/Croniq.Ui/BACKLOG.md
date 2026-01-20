# BACKLOG

_Last updated: 2026-01-18_

## Webhooks UI

- Action log panel showing recent admin operations from the UI.
- Activity timeline: add gRPC-Web/HTTP2 streaming support (true push) to remove SSE poll latency; keep SSE/polling fallback.
- Bulk enable/disable endpoints with confirmation and audit context.
- Grafana deep-links or embedded panels where available.
- Audit summary for rotations, IP rule changes, and failed deliveries.

## Webhooks Backend

- Telemetry-backed aggregates for webhook KPIs (Grafana URL or a dedicated API surface).

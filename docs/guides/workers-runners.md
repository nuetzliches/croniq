# Workers & Runners (HTTP)

Croniq's in-process .NET worker uses a lease-based model to claim and execute due triggers.
The HTTP work endpoints expose the same lease lifecycle so non-.NET workers can participate.
Worker host presence is tracked separately via `/workers`; this guide focuses on runner identities used by the `/work/*` surface.
For gRPC streaming clients, see [`grpc.md`](./grpc.md).

## Authentication & Scoping

All work endpoints:

- Require a least-privilege work scope per endpoint (`work:poll`, `work:renew`, `work:ack`, `work:events`).
- Are tenant-scoped via the route: `/tenants/{tenantId}/...`.
- Require an `environment` (query) or `environmentTag` (body).

Authentication supports both:

- `Authorization: Bearer ...`
- `X-Croniq-Key: <api-key>`

## Runner Identity

`runnerId` is treated as the lease owner and must match the authenticated caller identity (API client id for API keys or subject for bearer tokens). A runner represents a worker process instance and can execute many jobs over time. Use a stable value (for example `hostname + process`) and reuse the same `runnerId` for polling, renewing, and acknowledging work. If the `runnerId` does not match the authenticated caller identity, the server rejects the request with `403 runner-mismatch`.

## Endpoints

### Poll

`POST /tenants/{tenantId}/work/poll?environment=dev`

Claims due trigger leases for the caller.

Scope: `work:poll`

Request body:

- `runnerId` (string, required): stable worker identity (e.g., host + process).
- `batchSize` (int, optional): number of leases to claim. Default `1`.
- `waitForMs` (int, optional): long-poll timeout in milliseconds. Default `0` (immediate).

Response:

- `leases`: array of lease tokens.
  - `executionId`: execution identifier for logs/events.

### Renew

`POST /tenants/{tenantId}/work/renew?environment=dev`

Renews a lease while a work item is still being processed.

Scope: `work:renew`

Request body:

- `runnerId` (string, required)
- `lease` (token, required)

Response:

- `renewed` (bool)
- `lease` (updated token, when renewed)

### Ack

`POST /tenants/{tenantId}/work/ack?environment=dev`

Acknowledges completion and releases the lease.

Scope: `work:ack`

Request body:

- `runnerId` (string, required)
- `lease` (token, required)
- `succeeded` (bool, required)
- `nextFireTimeUtc` (optional): when set, the trigger is rescheduled.
- `deadLetterReason` (optional): set for failed work when no reschedule is requested.

### Events / Logs

`POST /tenants/{tenantId}/work/{executionId}:events?environment=dev`

Pushes execution-scoped events that are persisted via the execution log sink.

Scope: `work:events`

Request body:

- `runnerId` (string, required)
- `lease` (token, required)
- `events` (array):
  - `message` (string, required)
  - `level` (string, optional): `Trace|Debug|Information|Warning|Error|Critical`
  - `eventType` (string, optional)
  - `timestampUtc` (optional)
  - `properties` (optional)

## Sample

A minimal worker loop that polls/renews/acks is available at:

- `samples/worker-sdk-go`
- `samples/worker-sdk-node`
- `samples/worker-sdk-python`

## SDK/Worker Integration (Recommended)

Keep the SDK configuration explicit and stable, and document it for operators:

- Required config (as used in the samples):
  - `CRONIQ_API_BASEURL` (HTTP base URL)
  - `CRONIQ_TENANT_ID`
  - `CRONIQ_ENVIRONMENT`
  - `CRONIQ_API_KEY` or bearer token
  - `CRONIQ_RUNNER_ID` (must match API client id)
- Optional SDK knobs (exposed via config or flags):
  - poll batch size + long-poll wait
  - max inflight (for gRPC stream)
  - lease-renew lead time or renewal interval
  - request timeout + retry backoff/jitter

Failover/offline strategy:

- If polling fails due to transient network errors, back off with jitter and retry; do not spin.
- Keep renewing active leases while work is running; if renew fails with a conflict or missing lease, cancel the job and stop acking (the server may have reassigned).
- Treat ack and event publishing as idempotent. Retry on transient failures; stop on `403 runner-mismatch`, `409 lease-conflict`, or `404` (lease no longer valid).
- If auth fails (`401/403` or runner mismatch), treat it as a fatal configuration error.

Local persistence fallback (outgoing queue):

- Persist outgoing acks/events locally so a worker restart or brief outage does not lose results.
- Replay the queue in order; drop entries that conflict with server state (lease expired/conflict) and move on.
- Do not execute new work offline; only process work that was already leased before the outage.

More detail: see `docs/deep-dive/sdk-worker-integration.md`.

## Issue a Worker API Key (SQL auth)

For SQL-backed auth, you can use the helper script to create an API client and key with the worker scopes:

```powershell
./scripts/issue-worker-api-key.ps1 -TenantId default -ClientId worker-dev -Environment dev -EmitEnv
```

Use the emitted `CRONIQ_API_KEY` and set `CRONIQ_RUNNER_ID` to the same client id.
The helper defaults to the work scopes plus `workers:heartbeat`, `workers:read`, `runners:heartbeat`, and `runners:read`; use `-Scopes` to trim if you only need runner access.

## Runner Presence (Optional)

If you need runner availability for dashboards or ops tooling, use the runner heartbeat endpoints (worker hosts use `/workers` instead):

- `POST /tenants/{tenantId}/runners/heartbeat?environment=dev`
- `GET /tenants/{tenantId}/runners?environment=dev`

Scopes:

- `runners:heartbeat` for posting heartbeats
- `runners:read` for listing runners

Heartbeat payloads accept `runnerId`, optional `seenAtUtc`, and optional `metadataJson` for tags or capabilities. Presence is derived from the configured TTL; it does not affect lease correctness.

## Protocol Roadmap

The longer-term gRPC streaming protocol, work-item schema, and event/log ingestion plan are tracked in `docs/deep-dive/designs/polyglot-worker-protocol.md`.

> **Learn more:** See the deep dives on [persistence & leases](../deep-dive/persistence.md) and the [polyglot worker protocol](../deep-dive/designs/polyglot-worker-protocol.md).

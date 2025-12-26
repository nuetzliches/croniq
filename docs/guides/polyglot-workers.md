# Polyglot Workers (HTTP)

Croniq’s in-process .NET worker uses a **lease-based** model to claim and execute due triggers.
The HTTP work endpoints expose the same lease lifecycle so non-.NET (“polyglot”) workers can participate.

## Authentication & Scoping

All work endpoints:

- Require the `work:execute` scope.
- Are tenant-scoped via the route: `/tenants/{tenantId}/...`.
- Require an `environment` (query) or `environmentTag` (body).

Authentication supports both:

- `Authorization: Bearer ...`
- `X-Croniq-Key: <api-key>`

## Runner Identity

`runnerId` is treated as the lease owner. A runner represents a worker process instance and can execute many jobs over time. Use a stable value (for example `hostname + process`) and reuse the same `runnerId` for polling, renewing, and acknowledging work. If the `runnerId` does not match the active lease owner, the server rejects the request with `409 lease-conflict`.

## Endpoints

### Poll

`POST /tenants/{tenantId}/work/poll?environment=dev`

Claims due trigger leases for the caller.

Request body:

- `runnerId` (string, required): stable worker identity (e.g., host + process).
- `batchSize` (int, optional): number of leases to claim. Default `1`.
- `waitForMs` (int, optional): long-poll timeout in milliseconds. Default `0` (immediate).

Response:

- `leases`: array of lease tokens.

### Renew

`POST /tenants/{tenantId}/work/renew?environment=dev`

Renews a lease while a work item is still being processed.

Request body:

- `runnerId` (string, required)
- `lease` (token, required)

Response:

- `renewed` (bool)
- `lease` (updated token, when renewed)

### Ack

`POST /tenants/{tenantId}/work/ack?environment=dev`

Acknowledges completion and releases the lease.

Request body:

- `runnerId` (string, required)
- `lease` (token, required)
- `succeeded` (bool, required)
- `nextFireTimeUtc` (optional): when set, the trigger is rescheduled.
- `deadLetterReason` (optional): set for failed work.

## Sample

A minimal worker loop that polls/renews/acks is available at:

- `samples/worker-sdk-go`
- `samples/worker-sdk-node`
- `samples/worker-sdk-python`

## Runner Presence (Optional)

If you need runner availability for dashboards or ops tooling, use the runner heartbeat endpoints:

- `POST /tenants/{tenantId}/runners/heartbeat?environment=dev`
- `GET /tenants/{tenantId}/runners?environment=dev`

Heartbeat payloads accept `runnerId`, optional `seenAtUtc`, and optional `metadataJson` for tags or capabilities. Presence is derived from the configured TTL; it does not affect lease correctness.

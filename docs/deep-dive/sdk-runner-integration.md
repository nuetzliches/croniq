# SDK/Runner Integration (HTTP + gRPC)

This guide defines the expected runner SDK behavior so polyglot runners are consistent
across Go/Node/Python and remain safe under retries, restarts, and outages.

## Configuration contract

Required (consistent names across SDKs):

- `CRONIQ_API_BASEURL` (HTTP base URL)
- `CRONIQ_TENANT_ID`
- `CRONIQ_ENVIRONMENT`
- `CRONIQ_API_KEY` or `CRONIQ_BEARER_TOKEN`
- `CRONIQ_RUNNER_ID` (must match API client id; unique per live process)

Optional (recommended defaults shown):

- `CRONIQ_GRPC_BASEURL` (gRPC base URL; default: derived from API base)
- `CRONIQ_TRANSPORT_MODE` (`auto|grpc|polling`, default: `auto`)
- `CRONIQ_ALLOW_TEST_EXECUTIONS` (default: `false`)
- `CRONIQ_MAX_INFLIGHT` (default: 10)
- `CRONIQ_CAPABILITIES` (comma-separated tags)
- `CRONIQ_POLL_BATCH_SIZE` (default: 1)
- `CRONIQ_POLL_WAIT_MS` (default: 25000)
- `CRONIQ_REQUEST_TIMEOUT_MS` (default: 60000)
- `CRONIQ_RENEW_LEAD_MS` (default: 10000)
- `CRONIQ_RETRY_MAX_ATTEMPTS` (default: 5)
- `CRONIQ_RETRY_BASE_MS` (default: 250)
- `CRONIQ_RETRY_MAX_MS` (default: 5000)
- `CRONIQ_RUNNER_INSTANCE_ID` (optional; default: generated per process)
- `CRONIQ_RUNNER_REGISTER_JOBS` (default: `true`)

Note: not all SDKs implement these env vars yet; this is the target contract.

## Runner identity collisions (fail fast)

- SDKs generate a `runnerInstanceId` (UUID) and include it in gRPC hello/poll/heartbeat metadata.
- If the API host responds with `409 runner-id-in-use` (HTTP) or an `AlreadyExists` gRPC status with `runner-id-in-use` details, stop the runner immediately and surface a fatal configuration error.
- Treat `403 runner-mismatch` as fatal (the runner id does not match the caller identity).

## Execution intent fields

Lease payloads include execution intent metadata:

- `execution_mode`: `normal|test`
- `invocation_source`: `schedule|manual|api|webhook-ingress|webhook-invoke` (reserved: `system|replay|backfill`)

SDKs should treat these fields as read-only metadata, surface them in logs/telemetry, and avoid inventing new values.

## Handler registration and dispatch

- SDKs should expose per-job handlers (`runner.onExecute(jobKey, handler)`), with an optional default handler.
- The SDK owns lease, renew, ack, outbox, and transport behavior so client code stays minimal.
- If a lease arrives for an unknown `job_key`, reject it with a non-retryable failure reason and a clear log entry.

## Job self-registration

- SDKs should register jobs on startup via `POST /tenants/{tenantId}/jobs:register`.
- The request requires the `jobs:register` scope.
- The request should include `environmentTag`, `runnerId`, `runnerInstanceId`, `jobKey`, and optional `description`/`metadata`.
- If the API responds with `runner-registration-denied` (403), treat it as a fatal configuration error and stop the runner.
- Pending jobs (`isActive=false`) must never be dispatched; SDKs should log that approval is required.
- Allow users to disable auto-registration via `CRONIQ_RUNNER_REGISTER_JOBS=false`.

## Poll loop

- Use long-polling with `waitForMs`.
- If the response is empty, continue immediately.
- On failure, back off with jitter and retry.

## Lease renewal

- Start a renewal loop when a job begins.
- Renew when `(lease_expires_at_utc - now) <= renew_lead_ms`.
- If renewal returns `not found` or `lease-conflict`, cancel the job and stop acking.

## Ack and events

- `Ack` and `Events` are idempotent.
- Retry on transient network or 5xx errors.
- Stop retrying and drop the entry on `409 lease-conflict` or `404 not found`.
- Treat `401/403` as fatal configuration errors.
- For test executions rejected by policy, send `deadLetterReason: "test-not-allowed"` (non-retryable).

## Failover and backoff

- Use exponential backoff with jitter for poll/renew/ack/events.
- Reset backoff after a successful call.
- Do not spin when the server is unavailable.

## Graceful shutdown (drain)

- Expose a `drain`/`stop` option that stops claiming new work (close gRPC stream, stop polling).
- Continue renewing and acking in-flight leases until completion.
- After `shutdownTimeout`, cancel local execution and stop renewing; do not ack success after lease loss.

## Local persistence fallback (outbox)

- Persist outgoing acks/events to a local queue before sending.
- Replay the queue on restart or after network recovery.
- If a replayed entry fails with `409`/`404`, drop it and continue.
- Do not execute new work while offline; only complete already leased work.

Suggested queue record (JSONL):

```json
{ "type": "ack", "runner_id": "...", "lease_id": "...", "execution_id": "...", "payload": { ... }, "created_at_utc": "..." }
{ "type": "events", "runner_id": "...", "lease_id": "...", "execution_id": "...", "payload": { ... }, "created_at_utc": "..." }
```

## Error handling matrix

- `200/204`: success, clear local queue item.
- `401/403`: fatal, stop runner and fix credentials.
- `404/409`: lease invalid, drop local queue item.
- `429/5xx`: retry with backoff.

## Scaling guidance

- Horizontal scale-out is achieved by running multiple runners with distinct `CRONIQ_RUNNER_ID` values.
- Job-level concurrency limits remain the primary control for parallelism.

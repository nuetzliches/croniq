# SDK/Worker Integration (HTTP + gRPC)

This guide defines the expected worker SDK behavior so polyglot workers are consistent
across Go/Node/Python and remain safe under retries, restarts, and outages.

## Configuration contract

Required (consistent names across SDKs):

- `CRONIQ_API_BASEURL` (HTTP base URL)
- `CRONIQ_TENANT_ID`
- `CRONIQ_ENVIRONMENT`
- `CRONIQ_API_KEY` or `CRONIQ_BEARER_TOKEN`
- `CRONIQ_RUNNER_ID` (must match API client id)

Optional (recommended defaults shown):

- `CRONIQ_POLL_BATCH_SIZE` (default: 1)
- `CRONIQ_POLL_WAIT_MS` (default: 25000)
- `CRONIQ_REQUEST_TIMEOUT_MS` (default: 60000)
- `CRONIQ_RENEW_LEAD_MS` (default: 10000)
- `CRONIQ_RETRY_MAX_ATTEMPTS` (default: 5)
- `CRONIQ_RETRY_BASE_MS` (default: 250)
- `CRONIQ_RETRY_MAX_MS` (default: 5000)

Note: not all SDKs implement these env vars yet; this is the target contract.

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

## Failover and backoff

- Use exponential backoff with jitter for poll/renew/ack/events.
- Reset backoff after a successful call.
- Do not spin when the server is unavailable.

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
- `401/403`: fatal, stop worker and fix credentials.
- `404/409`: lease invalid, drop local queue item.
- `429/5xx`: retry with backoff.

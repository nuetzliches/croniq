# Polyglot Worker Protocol (gRPC + HTTP)

::: info Status
Draft (HTTP work endpoints shipped; gRPC streaming remains a target). Last verified: 2026-01-18.
:::

## Goals

- Enable non-.NET workers (Go/Node/Python) to execute Croniq jobs directly (not only trigger them).
- Offer two transports with identical semantics:
  - **gRPC** for efficient typed streaming.
  - **HTTP** for broad compatibility (proxies, simpler SDKs).
- Avoid a "global heartbeat API" as a first-class requirement.
  - Prefer **work-scoped leases with deadlines** and/or **connection presence** (for streaming).
- Preserve tenant isolation and environment scoping.
- Provide at-least-once execution with idempotent claim/ack semantics.

## Non-Goals

- Exactly-once guarantees.
- A full workflow engine protocol.
- A complete UI/ops model for runner fleets (that is tracked separately).

## Current State

- Croniq has tenant/env scoping (`TenantId`, `EnvironmentTag`) and a worker host that executes jobs.
- Worker host presence is tracked separately via `POST /tenants/{tenantId}/workers/heartbeat` and `GET /tenants/{tenantId}/workers`.
- Worker heartbeat metadata can include dispatch status (`dispatch.grpcConnected`, `dispatch.lastConnectedAtUtc`, `dispatch.lastFallbackAtUtc`) for UI/ops visibility.
- gRPC exists for scheduler-facing operations (e.g. `Scheduler` service).
- Long-running execution already uses a lease/extend model internally.
- The HTTP work endpoints (`/work/poll`, `/work/renew`, `/work/ack`, `/work/{executionId}:events`) expose the lease lifecycle for polyglot workers.
- A gRPC worker service skeleton (`Worker.Connect`) exists for the streaming handshake.
- Minimal worker SDK samples exist for Go/Node/Python under `samples/worker-sdk-*`.

## Core Concepts

### Runner

A **Runner** is a worker process instance that can claim and execute jobs.

- `runner_id`: stable identifier chosen by the operator (not instance-id based).
- `capabilities`: optional tags (language, os, queue, custom) used for routing.

### Work Item

A **Work Item** is an assignment representing a single execution attempt.

- `execution_id`: stable identifier for the execution (server-generated) and included in HTTP lease tokens for log/event correlation.
- `job_key`: identifies the job.
- `attempt`: monotonic attempt number for retry cycles.
- `lease_token`: opaque token proving current ownership.
- `lease_expires_at_utc`: deadline after which the server may reassign.

### Lease (No Global Heartbeats)

- A Runner does **not** send a periodic "I am alive" heartbeat.
- Liveness and ownership are derived from:
  - The **lease deadline** (server reassigns after expiration).
  - **Acks/events** while executing (optional; work-scoped).
  - For streaming: the **open stream** plus transport keepalive.

Croniq still supports optional runner presence via `POST /tenants/{tenantId}/runners/heartbeat` and `GET /tenants/{tenantId}/runners`, but correctness does not depend on those heartbeats.

## Semantics

### Delivery Guarantees

- **At-least-once** delivery.
- Duplicate delivery can occur (retries, lease expiry, network errors).

### Idempotency

All state transitions must be idempotent using:

- `(execution_id, attempt)` as the idempotency key for acknowledgements when available.
- `lease_token` as the authorization guard for "current owner" (current HTTP API).

Rules:

- `AckSuccess` / `AckFailure` must be safe to retry.
- If a Runner acks with a **stale** `lease_token`, the server rejects with a conflict (or treats as no-op).
- Events/log pushes should accept duplicates; the server may dedupe by `(execution_id, sequence)` if provided.

### Backpressure

- Runner declares a maximum in-flight limit (`max_inflight`).
- Server should not assign more than this to that runner.

## HTTP API (Current Shape)

### Poll for work (long-poll)

`POST /tenants/{tenantId}/work/poll?environment={environmentTag}`

Request:

- `runnerId` (string, required)
- `batchSize` (int, default 1; max 250)
- `waitForMs` (int, default 0; max 30000)
- `environmentTag` (string, optional, overrides query)

Response:

- `leases`: array of lease tokens (possibly empty)
  - `executionId`
  - `leaseId`
  - `jobKey`
  - `triggerId`
  - `fireAtUtc`
  - `leaseExpiresAtUtc`
  - `payload` (optional; job input)

Notes:

- If no work is available, the server may return an empty list after `waitForMs`.
- This is compatible with simple SDK loops and avoids WebSocket requirements.

### Renew

`POST /tenants/{tenantId}/work/renew?environment={environmentTag}`

Request:

- `runnerId` (string, required)
- `lease` (object, required)

Response:

- `renewed` (bool)
- `lease` (updated token when renewed)

### Ack (success or failure)

`POST /tenants/{tenantId}/work/ack?environment={environmentTag}`

Request:

- `runnerId` (string, required)
- `lease` (object, required)
- `succeeded` (bool, required)
- `nextFireTimeUtc` (optional)
- `deadLetterReason` (optional)

Notes:

- If `runnerId` does not match the lease owner, the server returns a conflict.
- When `nextFireTimeUtc` is set, the server reschedules the trigger and ignores `deadLetterReason`.
- Ack is idempotent; clients may retry on transient failures.

### Push events/logs (optional)

`POST /tenants/{tenantId}/work/{executionId}:events?environment={environmentTag}`

Request:

- `runnerId` (string)
- `lease` (token)
- `events` (array)

## gRPC API (Reference Shape)

### Preferred: bidi streaming session

`rpc Connect (stream RunnerMessage) returns (stream ServerMessage);`

- Runner opens a single stream.
- First message is `Hello` (runner id, capabilities, max inflight).
- Server sends `WorkAssigned` messages.
- Runner sends `AckSuccess`/`AckFailure` and optionally `Events`.
- `AckFailure` may include `next_fire_time_utc` to reschedule without dead-lettering.

Disconnect handling:

- Stream termination indicates the Runner is no longer connected.
- Transport keepalive can be used to detect broken connections faster.
- Ownership is still governed by lease deadlines (disconnect does not automatically mean the work is lost).

### Optional: unary fallback

Unary RPCs can mirror HTTP endpoints if needed, but the key goal is identical semantics.

## Transport Mapping

- **Claim/Poll**:
  - HTTP: `work:poll` long-poll.
  - gRPC: server pushes `WorkAssigned` on the stream.
- **Ack**:
  - HTTP: `ack-success` / `ack-failure`.
  - gRPC: `AckSuccess` / `AckFailure` messages.
- **Logs/events**:
  - HTTP: `:events`.
  - gRPC: `Events` message.

## Authentication & Scoping (High Level)

- Runners authenticate using an access token (e.g. bearer) that encodes:
  - tenant scope (single-tenant)
  - allowed environments (or environment passed explicitly)
  - least-privilege scopes (e.g. `work:poll`, `work:renew`, `work:ack`, `work:events`)

The API must enforce that:

- a runner cannot access other tenants.
- a runner cannot ack work it does not own (`lease_token` guard).
- `runner_id` must match the authenticated caller identity (API client id for API keys or subject for bearer tokens).

## Operational Notes

- Without global heartbeats, "Runner availability" becomes an ops/UI concern:
  - streaming transport provides presence information naturally.
  - HTTP long-poll provides "recently active" signals but is not strict presence.
- Correctness does not depend on presence; leases + idempotent acks are the source of truth.
  - Croniq also tracks worker host presence separately via `/workers` for dashboard/ops use.

## Persistence & Schema

WorkItems/WorkClaims/RunnerCapabilities are part of the SqlServer/Postgres schema and are updated by the HTTP/gRPC work endpoints when assignments are claimed, renewed, and acknowledged. WorkEvents are still optional; events are currently streamed into the execution log store.

Current tables (SqlServer/Postgres):

- `croniq.WorkItems`
  - `WorkItemId` (PK)
  - `ExecutionId` (unique)
  - `TenantId`, `EnvironmentTag`
  - `JobKey`, `TriggerId`
  - `Attempt`, `PayloadJson`
  - `Status` (queued, leased, succeeded, failed, deadletter)
  - `CreatedAtUtc`, `UpdatedAtUtc`
- `croniq.WorkClaims`
  - `WorkItemId` (FK), `LeaseId`
  - `RunnerId` (owner)
  - `LeaseExpiresAtUtc`
  - `LastHeartbeatAtUtc` (optional, only if we add work-scoped heartbeats)
- `croniq.RunnerCapabilities`
  - `RunnerId`, `TenantId`, `EnvironmentTag`
  - `CapabilitiesJson` (tags or kv pairs)
  - `UpdatedAtUtc`
- `croniq.WorkEvents` (optional; not yet materialized in SqlServer/Postgres)
  - `ExecutionId`, `Attempt`, `Sequence`
  - `EventType`, `PayloadJson`
  - `OccurredAtUtc`

The existing `croniq.Runners` table remains the runner availability view (TTL-based) and does not gate leasing. Worker host presence is stored in `croniq.WorkerInstances` and is also informational.

## Integration Plan

1. Add `executionId` to the lease token and propagate it through Acquire/Renew/Ack so events/logs can correlate reliably. (Implemented for HTTP.)
2. Introduce a work event endpoint (`/work/{executionId}:events`) that writes to the execution log store or a dedicated `WorkEvents` table. (Implemented against the execution log store.)
3. Add a gRPC `WorkerService` with the same semantics as HTTP (Connect stream + Ack + Events). (Skeleton + hello handshake implemented.)
4. Add optional runner capability routing (filter by capability tags during poll).
5. Unify internal worker host and external workers on the same work item model (server creates work items, workers claim/ack them).

## Testing Plan

- Contract tests for HTTP work endpoints (poll/renew/ack/events), including invalid runner, stale lease, and idempotent retry cases.
- gRPC streaming contract tests: Hello -> WorkAssigned -> AckSuccess/AckFailure, reconnect behavior.
- Concurrency tests: multiple runners polling, lease expiration, duplicate claim prevention.
- Persistence tests: WorkItems/WorkClaims CRUD, lease expiry, and dead-letter transitions.

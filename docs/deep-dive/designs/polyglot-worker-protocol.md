# Polyglot Worker Protocol (gRPC + HTTP)

## Goals

- Enable non-.NET workers (Go/Node/Python) to execute Croniq jobs directly (not only trigger them).
- Offer two transports with identical semantics:
  - **gRPC** for efficient typed streaming.
  - **HTTP** for broad compatibility (proxies, simpler SDKs).
- Avoid a “global heartbeat API” as a first-class requirement.
  - Prefer **work-scoped leases with deadlines** and/or **connection presence** (for streaming).
- Preserve tenant isolation and environment scoping.
- Provide at-least-once execution with idempotent claim/ack semantics.

## Non-Goals

- Exactly-once guarantees.
- A full workflow engine protocol.
- A complete UI/ops model for runner fleets (that is tracked separately).

## Current State

- Croniq has tenant/env scoping (`TenantId`, `EnvironmentTag`) and a worker host that executes jobs.
- gRPC exists for scheduler-facing operations (e.g. `Scheduler` service).
- Long-running execution already uses a lease/extend model internally.
- There is no public protocol that allows external processes to claim and execute work items.

## Core Concepts

### Runner

A **Runner** is a worker process instance that can claim and execute jobs.

- `runner_id`: stable identifier chosen by the operator (not instance-id based).
- `capabilities`: optional tags (language, os, queue, custom) used for routing.

### Work Item

A **Work Item** is an assignment representing a single execution attempt.

- `execution_id`: stable identifier for the execution (server-generated).
- `job_key`: identifies the job.
- `attempt`: monotonic attempt number for retry cycles.
- `lease_token`: opaque token proving current ownership.
- `lease_expires_at_utc`: deadline after which the server may reassign.

### Lease (No Global Heartbeats)

- A Runner does **not** send a periodic “I am alive” heartbeat.
- Liveness and ownership are derived from:
  - The **lease deadline** (server reassigns after expiration).
  - **Acks/events** while executing (optional; work-scoped).
  - For streaming: the **open stream** plus transport keepalive.

This keeps the protocol focused on correctness (ownership) rather than presence.

## Semantics

### Delivery Guarantees

- **At-least-once** delivery.
- Duplicate delivery can occur (retries, lease expiry, network errors).

### Idempotency

All state transitions must be idempotent using:

- `(execution_id, attempt)` as the idempotency key for acknowledgements.
- `lease_token` as the authorization guard for “current owner”.

Rules:

- `AckSuccess` / `AckFailure` must be safe to retry.
- If a Runner acks with a **stale** `lease_token`, the server rejects with a conflict (or treats as no-op).
- Events/log pushes should accept duplicates; the server may dedupe by `(execution_id, sequence)` if provided.

### Backpressure

- Runner declares a maximum in-flight limit (`max_inflight`).
- Server should not assign more than this to that runner.

## HTTP API (Reference Shape)

### Poll for work (long-poll)

`POST /tenants/{tenantId}/work:poll?environment={environmentTag}`

Request:

- `runnerId` (string, required)
- `capabilities` (map<string,string> or list<string>, optional)
- `maxItems` (int, default 1)
- `maxInflight` (int, optional)
- `waitSeconds` (int, default e.g. 25; server may cap)

Response:

- `items`: array of work items (possibly empty)
  - `executionId`
  - `jobKey`
  - `attempt`
  - `leaseToken`
  - `leaseExpiresAtUtc`
  - `payload` (optional; job input)

Notes:

- If no work is available, server may return an empty list after `waitSeconds`.
- This is compatible with simple SDK loops and avoids WebSocket requirements.

### Ack success

`POST /tenants/{tenantId}/work/{executionId}:ack-success?environment={environmentTag}`

Request:

- `attempt` (int, required)
- `leaseToken` (string, required)
- `result` (optional)

### Ack failure

`POST /tenants/{tenantId}/work/{executionId}:ack-failure?environment={environmentTag}`

Request:

- `attempt` (int, required)
- `leaseToken` (string, required)
- `errorType` / `errorMessage` (optional)
- `retryHint` (optional)

### Push events/logs (optional)

`POST /tenants/{tenantId}/work/{executionId}:events?environment={environmentTag}`

Request:

- `attempt` (int)
- `leaseToken` (string)
- `sequence` (long, optional)
- `events` (array)

## gRPC API (Reference Shape)

### Preferred: bidi streaming session

`rpc Connect (stream RunnerMessage) returns (stream ServerMessage);`

- Runner opens a single stream.
- First message is `Hello` (runner id, capabilities, max inflight).
- Server sends `WorkAssigned` messages.
- Runner sends `AckSuccess`/`AckFailure` and optionally `Events`.

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
  - least-privilege scopes (e.g. `work:poll`, `work:ack`, `work:events`)

The API must enforce that:

- a runner cannot access other tenants.
- a runner cannot ack work it does not own (`lease_token` guard).

## Operational Notes

- Without global heartbeats, “Runner availability” becomes an ops/UI concern:
  - streaming transport provides presence information naturally.
  - HTTP long-poll provides “recently active” signals but is not strict presence.
- Correctness does not depend on presence; leases + idempotent acks are the source of truth.

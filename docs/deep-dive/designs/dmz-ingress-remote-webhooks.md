# DMZ Ingress-Only Remote Webhooks

## Goals

- Accept public webhooks in a DMZ without any outbound connections to the internal network.
- Keep the UI and management API on the internal network only.
- Allow internal operators to configure webhooks and have those definitions synced to the DMZ.
- Deliver webhook events to internal execution via stream/queue pull (polling optional).

## Constraints

- DMZ hosts must not open connections into the internal network.
- Internal UI must only connect to the internal Croniq.Api.
- DMZ storage uses the existing SqlServer migrations from `Croniq.Data.SqlServer`.
- `Croniq.Persistence.Abstractions` contains contracts only and does not define schema.

## Proposed Topology

DMZ:

- `Croniq.Webhooks` for ingress (`POST /tenants/{tenantId}/environments/{environmentTag}/webhooks/{hookKey}`).
- `Croniq.Api` in a restricted "webhook admin" mode (only webhook CRUD + health).
- Dedicated SqlServer instance seeded by `Croniq.Data.SqlServer` migrations.

Internal network:

- `Croniq.Api` + WorkerHost (job execution).
- Webhook relay worker (pulls DMZ events and triggers jobs).
- UI connects only to the internal API.

## Mode Semantics

DMZ remains `Croniq:Webhooks:Mode=SqlServer` and stores webhook definitions locally.
Internal API uses a new `Croniq:Webhooks:Mode=Remote` to call the DMZ admin API.

Example (internal API):

```yaml
Croniq:
  Webhooks:
    Mode: Remote
    Remote:
      BaseUrl: https://dmz-croniq.example
      ApiKey: <dmz-admin-key>
      TimeoutSeconds: 10
```

## Event Delivery (Ingress -> Internal Execution)

1) DMZ ingress validates signature and writes a `WebhookIngressEvent` (DB or queue).
2) Internal relay worker connects outward (stream/queue pull).
3) Relay worker triggers jobs internally (`IJobTrigger` or `/jobs/trigger`).

Streaming is preferred; polling is a fallback with cursor-based paging.

## Transport (gRPC First)

Croniq already ships gRPC for workers, so the remote ingress stream should be gRPC-first:

- Internal relay worker opens an outbound gRPC stream to the DMZ (HTTP/2).
- DMZ never opens connections to the internal network.
- gRPC stays internal-only (not exposed on the public ingress).
- Provide an SSE or long-poll fallback for environments that cannot route gRPC.

The stream should be at-least-once with explicit ack/lease semantics to avoid event loss.

## gRPC Contract (Draft)

Draft proto file: `src/Croniq.Rpc.Client/Protos/webhook_ingress.proto`.

```proto
syntax = "proto3";

package croniq.rpc;

option csharp_namespace = "Croniq.Rpc";

service WebhookIngress {
  rpc Connect (stream WebhookIngressClientMessage) returns (stream WebhookIngressServerMessage);
}

message WebhookIngressClientMessage {
  oneof payload {
    WebhookConsumerHello hello = 1;
    WebhookEventAck ack = 2;
    WebhookEventNack nack = 3;
    WebhookEventExtend extend = 4;
  }
}

message WebhookIngressServerMessage {
  oneof payload {
    WebhookServerHello hello = 1;
    WebhookIngressEvent event = 2;
  }
}

message WebhookConsumerHello {
  string consumer_id = 1;
  int32 max_inflight = 2;
  string tenant_id = 3;
  string environment_tag = 4;
}

message WebhookServerHello {
  string server_id = 1;
  string tenant_id = 2;
  string environment_tag = 3;
  int64 server_time_utc = 4;
}

message WebhookIngressEvent {
  string event_id = 1;
  string lease_id = 2;
  int64 lease_expires_at_utc = 3;
  string hook_key = 4;
  string job_key = 5;
  string payload = 6;
  map<string, string> headers = 7;
  int64 received_at_utc = 8;
  map<string, string> metadata = 9;
}

message WebhookEventAck {
  string event_id = 1;
  string lease_id = 2;
  bool succeeded = 3;
  string error_message = 4;
}

message WebhookEventNack {
  string event_id = 1;
  string lease_id = 2;
  string reason = 3;
}

message WebhookEventExtend {
  string event_id = 1;
  string lease_id = 2;
  int64 lease_expires_at_utc = 3;
}
```

## HTTP Fallback Endpoints

When gRPC is blocked, the relay can use SSE or polling against the DMZ API:

- `GET /tenants/{tenantId}/webhooks/ingress/stream?environment=...&consumerId=...&maxInflight=...&maxBatchSize=...`
  - Server-sent events with `data:` payloads matching the `WebhookIngressEvent` shape (unix ms timestamps).
- `GET /tenants/{tenantId}/webhooks/ingress/poll?environment=...&maxBatchSize=...&waitMs=...`
  - Returns `{ events: [...], serverTimeUtc: <unix ms> }`.
- `POST /tenants/{tenantId}/webhooks/ingress/ack?environment=...`
- `POST /tenants/{tenantId}/webhooks/ingress/nack?environment=...`
- `POST /tenants/{tenantId}/webhooks/ingress/extend?environment=...`

All ingress stream endpoints require the `webhooks:ingress` scope and the same API key used for the gRPC relay.

Notes:

- DMZ validates signatures and only streams verified ingress events.
- `lease_id` + `lease_expires_at_utc` allow retry without duplicating events.
- `metadata` mirrors webhook metadata and payload hints.

## Configuration (Draft)

Internal API (remote persistence + relay):

```yaml
Croniq:
  Webhooks:
    Mode: Remote
    Remote:
      BaseUrl: https://dmz-croniq.example
      ApiKey: <dmz-admin-key>
      TimeoutSeconds: 10
      StreamMode: Grpc
      MaxInflight: 100
      ReconnectDelaySeconds: 5
      EnableRelay: true
      StreamFallback: Sse
```

DMZ (ingress-only, no outbound):

```yaml
Croniq:
  Webhooks:
    Mode: SqlServer
    Ingress:
      DispatchMode: StoreOnly
      LeaseSeconds: 30
      MaxBatchSize: 100
      PollingIntervalMilliseconds: 250
    Security:
      AllowUnsignedHooks: false
  Api:
    Surface: WebhookAdminOnly
```

## Event Store Schema (Draft)

`WebhookIngressEvent` (DMZ SqlServer):

- `Id` (bigint identity)
- `EventId` (nvarchar(64), unique)
- `TenantId`, `EnvironmentTag` (nvarchar(64))
- `HookKey` (nvarchar(128)), `JobKey` (nvarchar(256))
- `Payload` (nvarchar(max)), `HeadersJson` (nvarchar(max)), `MetadataJson` (nvarchar(max))
- `ReceivedAtUtc` (datetime2)
- `LeaseId` (nvarchar(64)), `LeaseExpiresAtUtc` (datetime2)
- `Status` (nvarchar(32): Pending|Leased|Delivered|Failed)
- `AttemptCount` (int), `LastError` (nvarchar(1024))
- `CreatedAtUtc`, `UpdatedAtUtc` (datetime2)

Indexes:

- Unique: `EventId`
- Lookup: `(TenantId, EnvironmentTag, Status, LeaseExpiresAtUtc)`
- Ordering: `(TenantId, EnvironmentTag, ReceivedAtUtc)`

## Streaming Service Outline

- `Connect` validates caller scope (tenant/environment) and sends `ServerHello`.
- Scope requirement: `webhooks:ingress`.
- Server pulls pending events, assigns leases, and streams `WebhookIngressEvent`.
- Client acks with `WebhookEventAck` (success/failure), or `WebhookEventNack` for retry (clears the lease and re-queues).
- Client can extend leases via `WebhookEventExtend`.
- Server expires leases and re-queues events when `LeaseExpiresAtUtc` passes.
- Updates are idempotent by `(EventId, LeaseId)` to tolerate retries.

## Relay Worker Loop (Sketch)

```text
connect -> hello(max_inflight)
while stream open:
  receive event
  try trigger job
  if success: ack(succeeded=true)
  else: ack(succeeded=false, error_message)
  on transient error: nack(reason) and backoff
  periodically extend leases if processing > lease
```

## Required Components

- `RemoteWebhookPersistenceProvider` (HTTP client over the DMZ admin API).
- DMZ admin hardening: scope restriction, allowlist, rate limits, and disabling non-webhook endpoints.
- `WebhookIngressEvent` store + streaming endpoint (gRPC, SSE fallback) with ack/lease.
- Internal relay worker (stream client + retry/backoff).
- New sample host `samples/Croniq.Sample.Dmz` (DMZ API + Webhooks + DB).

## Implementation Slices

1) Remote persistence provider + config options + tests.
2) DMZ event store + streaming endpoint (gRPC default, SSE fallback).
3) Internal relay worker + end-to-end tests.
4) DMZ sample + docs update for the topology.

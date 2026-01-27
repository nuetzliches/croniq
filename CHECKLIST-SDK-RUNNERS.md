# Checklist: Polyglot Runner SDKs (gRPC -> polling)

Goal: deliver runner SDKs that execute Croniq jobs via the `/work` endpoints with gRPC streaming as the primary transport and HTTP polling as the fallback. Keep the runner concept aligned with the lease model (no SSE for runner transport).

## Sources reviewed

- docs/guides/workers-runners.md
- docs/deep-dive/designs/polyglot-runner-protocol.md
- docs/deep-dive/sdk-runner-integration.md
- docs/deep-dive/architecture.md
- src/Croniq.Ui/docs/deep-dive/ui.md (manual invoke roadmap + activity source flags)

## A. Concept guardrails (baseline)

- [x] Runners are worker process instances using `/work/*`; the .NET WorkerHost dispatch path is separate.
- [x] Correctness is lease-based; runner presence/heartbeats are optional and never gate work assignment.
- [x] `runnerId` must match the authenticated caller identity; treat mismatches as fatal configuration errors.
- [x] Transport chain is gRPC streaming -> HTTP polling with identical semantics and explicit fallback triggers.
- [x] Active leases must keep renewing/acking regardless of transport state.
- [x] Naming: use "Runner" for polyglot SDKs and `/work` clients; reserve "WorkerHost" for the .NET host.
- [x] Cleanup: align runner SDK naming (Python `croniq_runner` + `RunnerClient`, Go package `croniqrunner`).

## B. Design gaps to resolve (docs vs goal)

- [x] Confirm gRPC-first with HTTP polling fallback for runners (SSE is UI-only per `architecture.md`).
- [x] Align `docs/deep-dive/architecture.md` and `docs/deep-dive/designs/polyglot-runner-protocol.md` with the gRPC + polling transport mapping and fallback rules.
- [x] Remove/replace the SSE fallback mention in `docs/deep-dive/designs/samples-to-aspire-hosts.md` (runner transport should be gRPC + polling only).

## C. Contract changes (API + protocol)

- [x] Add execution intent fields to work items and lease payloads:
  - `executionMode`: `normal|test` (no test value in `invocationSource`).
  - `invocationSource` (extensible): `schedule|manual|api|webhook-ingress|webhook-invoke`.
  - Reserved for future: `system|replay|backfill`.
- [x] Add runner capability flags to poll + gRPC Hello (`allowTestExecutions`, `maxInflight`, optional `capabilities` tags).
- [x] Define how a runner rejects a test execution (e.g., `AckFailure` with reason `test-not-allowed`, `retryable=false`, or a dedicated `Reject` message).
- [x] Specify server-side behavior when a test call is rejected:
  - log a Warning on the initiating API call with runner + execution identifiers.
  - surface the rejection in UI activity timelines as a warning event.
- [x] Update OpenAPI and gRPC schemas to include new fields and rejection reasons (backward-compatible defaults).
- [x] Extend gRPC/HTTP schemas and models to include `executionMode` and `invocationSource`.

## D. SDK behavior (shared requirements)

- [ ] Implement transport chain: gRPC streaming -> HTTP polling; allow explicit `transportMode` override (`auto|grpc|polling`).
  - [x] Node SDK: `CroniqRunner` supports gRPC + polling fallback with `transportMode`.
  - [x] Python SDK: async `CroniqRunner` with gRPC + polling fallback.
  - [x] Go SDK: transport chain (gRPC + polling fallback).
- [ ] Standardize reconnect/backoff with jitter for all transports; keep gRPC reconnect attempts running while polling.
  - [x] Node SDK: gRPC reconnect loop + polling fallback delay.
  - [x] Python SDK: gRPC reconnect loop + polling fallback delay.
- [ ] Ensure lease renewals keep running regardless of transport (in-flight work must not depend on an active stream).
  - [x] Node SDK: HTTP renew loop per lease.
  - [x] Go SDK: HTTP renew loop per lease (polling runner).
- [ ] Honor `executionMode` and runner policy (reject tests when disallowed, before running payload).
- [ ] Support outbox persistence for ack/events (per `sdk-runner-integration.md`); bound disk usage and replay on startup.
  - [x] Node SDK: file-backed outbox for ack/events with replay + size bounds.
  - [x] Python SDK: file-backed outbox for ack/events with replay + size bounds.
  - [x] Go SDK: file-backed outbox for ack/events with replay + size bounds.
- [ ] Optional: runner heartbeat support for ops (`/runners/heartbeat`) with metadata (capabilities, transport state).
- [ ] Provide a uniform configuration contract across SDKs:
  - Required: `CRONIQ_API_BASEURL`, `CRONIQ_TENANT_ID`, `CRONIQ_ENVIRONMENT`, `CRONIQ_API_KEY|CRONIQ_BEARER_TOKEN`, `CRONIQ_RUNNER_ID`
  - Optional (transport): `CRONIQ_GRPC_BASEURL`, `CRONIQ_TRANSPORT_MODE` (`auto|grpc|polling`), `CRONIQ_ALLOW_TEST_EXECUTIONS`
  - Optional (standard knobs): `CRONIQ_POLL_BATCH_SIZE`, `CRONIQ_POLL_WAIT_MS`, `CRONIQ_REQUEST_TIMEOUT_MS`, `CRONIQ_RENEW_LEAD_MS`, `CRONIQ_RETRY_MAX_ATTEMPTS`, `CRONIQ_RETRY_BASE_MS`, `CRONIQ_RETRY_MAX_MS`, `CRONIQ_MAX_INFLIGHT`, `CRONIQ_CAPABILITIES`
  - Validation: fail fast if neither/both API key and bearer token are set; treat `403 runner-mismatch` as fatal.

## E. SDK decisions (recommended defaults)

- [x] .NET: ship a dedicated `Croniq.Runner.Sdk` package (keep `Croniq.Sdk` focused on job authoring).
- [x] .NET: provide `AddCroniqRunner` + `BackgroundService` integration plus a lightweight `CroniqRunner` for custom loops.
- [x] Go: one module under `sdk/runner-go` (module path `github.com/croniq/croniq/sdk/runner-go`), Go 1.22+, gRPC `google.golang.org/grpc` + `protobuf` latest stable.
- [x] Go: honor context cancellation immediately; do not retry on canceled/deadline contexts.
- [x] Node: support Node LTS only; Bun experimental and polling-only until gRPC is proven stable.
- [x] Node: dual ESM/CJS via `exports`; gRPC via `@grpc/grpc-js` + `@grpc/proto-loader`.
- [x] Python: require 3.11+; async-first (`grpc.aio`) with an optional sync wrapper.
- [x] Python: pin `grpcio`/`protobuf` with upper bounds to avoid silent breaking upgrades.

## F. Server-side implementation

- [x] Implement gRPC `Runner.Connect` semantics for streaming assignments and ensure parity with HTTP work endpoints.
- [x] Persist `executionMode` + `invocationSource` in work items and propagate to logs/metrics.
- [x] Enforce runner test policy server-side (do not dispatch tests to runners without `allowTestExecutions`).
- [x] Emit structured logs/metrics for:
  - transport selection + fallback transitions
  - test execution acceptance/rejection
  - warning log on rejection for the initiating API call

## G. UI requirements (manual invoke visibility)

- [x] Clearly label manual invocations for webhooks/schedules/jobs in UI.
- [x] Distinguish test vs normal invoke in UI activity timelines and execution detail.
- [x] When a runner rejects a test execution, show a warning badge or toast in the initiating UI flow.
- [x] Update UI activity sources to include `invoke:test` or equivalent mapping (align with `invocationSource`/`executionMode`).

## H. Samples & documentation

- [x] Move runner SDKs out of samples into a dedicated SDK folder (e.g., `sdk/runner-go`, `sdk/runner-node`, `sdk/runner-python`, `sdk/runner-dotnet`).
- [x] Place runner samples under `samples/runners/<language>/<name>` and wire them into the AppHost via opt-in profiles (per `docs/deep-dive/designs/samples-to-aspire-hosts.md`).
- [x] Register one runner per language in the Aspire Devstack (AppHost profiles) so each SDK has a runnable dev example (P0/blocker).
- [x] Expand `docs/guides/workers-runners.md` with transport fallback behavior and test execution semantics.
- [x] Update `docs/deep-dive/sdk-runner-integration.md` with new env vars and rejection rules.
- [x] Update `docs/deep-dive/designs/polyglot-runner-protocol.md` to include `executionMode` and the gRPC + polling fallback.
- [x] Cross-link changes in `docs/index.md` and `docs/feature-map.md` if needed.

## I. Testing checklist

- [ ] Contract tests for gRPC/polling parity (claim/ack/events) including runner mismatch and lease conflicts.
- [ ] Test rejection path: test invoke rejected -> warning logged on initiator + UI warning surfaced.
  - [x] gRPC: test rejection writes warning log entry.
- [ ] Idempotency and lease-conflict scenarios across transports.
  - [x] gRPC ack idempotency (second ack ignored).
- [ ] Fallback chain e2e test: gRPC down -> polling.
  - [x] API fallback: gRPC unavailable -> HTTP polling succeeds.
- [ ] Outbox durability: restart with pending ack/events -> replay without duplicates.
- [x] HTTP poll coverage: `AllowTestExecutions` gating, `executionMode`/`invocationSource` propagation, and `MaxInflight` fallback.
- [x] Add gRPC parity for execution intent + test gating.
- [x] gRPC events append execution logs.

## Open questions

- Which SDK language should be implemented first for the full gRPC-first + polling fallback runner (Go, Node, Python, .NET)?
- Do we want transport selection/fallback metrics emitted in the SDKs or in the API host (and which metric names)?
- Should UI warning surfacing for test rejection be implemented in the schedules logs view only, or also in webhook timelines and trigger details?
- Should runner samples be wired into AppHost profiles immediately, or after SDKs are finalized?
- Decision: There are no external consumers (including Python). We can rename freely to achieve consistent Runner naming without compatibility shims.

## J. Node consumer example (script)

```ts
// Example consumer script for the Node runner SDK.
// This assumes a proposed SDK shape; adjust names once the SDK is finalized.

import { CroniqRunner } from "@croniq/runner-sdk";

const config = {
  apiBaseUrl: process.env.CRONIQ_API_BASEURL,
  grpcBaseUrl: process.env.CRONIQ_GRPC_BASEURL,
  tenantId: process.env.CRONIQ_TENANT_ID,
  environment: process.env.CRONIQ_ENVIRONMENT,
  apiKey: process.env.CRONIQ_API_KEY,
  bearerToken: process.env.CRONIQ_BEARER_TOKEN,
  runnerId: process.env.CRONIQ_RUNNER_ID,
  transportMode: process.env.CRONIQ_TRANSPORT_MODE ?? "auto",
  allowTestExecutions: process.env.CRONIQ_ALLOW_TEST_EXECUTIONS === "true",
  maxInflight: process.env.CRONIQ_MAX_INFLIGHT
    ? Number(process.env.CRONIQ_MAX_INFLIGHT)
    : undefined,
  capabilities: process.env.CRONIQ_CAPABILITIES
    ? process.env.CRONIQ_CAPABILITIES.split(",")
        .map((value) => value.trim())
        .filter(Boolean)
    : undefined,
};

const runner = new CroniqRunner(config);

runner.onExecute(async (context, payload, logger) => {
  logger.info("execution started", {
    executionId: context.executionId,
    jobKey: context.jobKey,
    triggerId: context.triggerId,
    executionMode: context.executionMode,
  });

  // TODO: replace with real job logic.
  await doWork(payload);

  logger.info("execution completed", {
    executionId: context.executionId,
    durationMs: context.durationMs,
  });
});

runner.start().catch((err) => {
  console.error("runner failed to start", err);
  process.exit(1);
});

async function doWork(payload: unknown) {
  if (payload) {
    console.log("payload received", payload);
  }
}
```

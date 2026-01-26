# Checklist: Polyglot Runner SDKs (gRPC -> polling)

Goal: deliver runner SDKs that execute Croniq jobs via the `/work` endpoints with gRPC streaming as the primary transport and HTTP polling as the fallback. Keep the runner concept aligned with the lease model (no SSE for runner transport).

## Sources reviewed

- docs/guides/workers-runners.md
- docs/deep-dive/designs/polyglot-worker-protocol.md
- docs/deep-dive/sdk-worker-integration.md
- docs/deep-dive/architecture.md
- src/Croniq.Ui/docs/deep-dive/ui.md (manual invoke roadmap + activity source flags)

## A. Concept guardrails (baseline)

- [ ] Runners are worker process instances using `/work/*`; the .NET WorkerHost dispatch path is separate.
- [ ] Correctness is lease-based; runner presence/heartbeats are optional and never gate work assignment.
- [ ] `runnerId` must match the authenticated caller identity; treat mismatches as fatal configuration errors.
- [ ] Transport chain is gRPC streaming -> HTTP polling with identical semantics and explicit fallback triggers.
- [ ] Active leases must keep renewing/acking regardless of transport state.

## B. Design gaps to resolve (docs vs goal)

- [ ] Confirm gRPC-first with HTTP polling fallback for runners (SSE is UI-only per `architecture.md`).
- [ ] Align `docs/deep-dive/architecture.md` and `docs/deep-dive/designs/polyglot-worker-protocol.md` with the gRPC + polling transport mapping and fallback rules.
- [ ] Remove/replace the SSE fallback mention in `docs/deep-dive/designs/samples-to-aspire-hosts.md` (runner transport should be gRPC + polling only).

## C. Contract changes (API + protocol)

- [ ] Add execution intent fields to work items and lease payloads:
  - `executionMode`: `normal|test` (no test value in `invocationSource`).
  - `invocationSource`: `schedule|manual|webhook-ingress|webhook-invoke|api` (verify naming against UI activity sources).
- [ ] Add runner capability flags to poll + gRPC Hello (`allowTestExecutions`, `maxInflight`, optional `capabilities` tags).
- [ ] Define how a runner rejects a test execution (e.g., `AckFailure` with reason `test-not-allowed`, `retryable=false`, or a dedicated `Reject` message).
- [ ] Specify server-side behavior when a test call is rejected:
  - log a Warning on the initiating API call with runner + execution identifiers.
  - surface the rejection in UI activity timelines as a warning event.
- [ ] Update OpenAPI and gRPC schemas to include new fields and rejection reasons (backward-compatible defaults).

## D. SDK behavior (shared requirements)

- [ ] Implement transport chain: gRPC streaming -> HTTP polling; allow explicit `transportMode` override (`auto|grpc|polling`).
- [ ] Standardize reconnect/backoff with jitter for all transports; keep gRPC reconnect attempts running while polling.
- [ ] Ensure lease renewals keep running regardless of transport (in-flight work must not depend on an active stream).
- [ ] Honor `executionMode` and runner policy (reject tests when disallowed, before running payload).
- [ ] Support outbox persistence for ack/events (per `sdk-worker-integration.md`); bound disk usage and replay on startup.
- [ ] Optional: runner heartbeat support for ops (`/runners/heartbeat`) with metadata (capabilities, transport state).
- [ ] Provide a uniform configuration contract across SDKs:
  - Required: `CRONIQ_API_BASEURL`, `CRONIQ_TENANT_ID`, `CRONIQ_ENVIRONMENT`, `CRONIQ_API_KEY|CRONIQ_BEARER_TOKEN`, `CRONIQ_RUNNER_ID`
  - Optional (transport): `CRONIQ_GRPC_BASEURL`, `CRONIQ_TRANSPORT_MODE` (`auto|grpc|polling`), `CRONIQ_ALLOW_TEST_EXECUTIONS`
  - Optional (standard knobs): `CRONIQ_POLL_BATCH_SIZE`, `CRONIQ_POLL_WAIT_MS`, `CRONIQ_REQUEST_TIMEOUT_MS`, `CRONIQ_RENEW_LEAD_MS`, `CRONIQ_RETRY_MAX_ATTEMPTS`, `CRONIQ_RETRY_BASE_MS`, `CRONIQ_RETRY_MAX_MS`, `CRONIQ_MAX_INFLIGHT`, `CRONIQ_CAPABILITIES`
  - Validation: fail fast if neither/both API key and bearer token are set; treat `403 runner-mismatch` as fatal.

## E. Open questions per SDK

- [ ] .NET: should the runner SDK live inside `Croniq.Sdk` or ship as a separate package (e.g., `Croniq.Worker.Sdk`)?
- [ ] .NET: expected DI + logging integration surface (host builder extensions vs lightweight client).
- [ ] .NET: hosted service shape (`BackgroundService`) vs pull-based API for custom host loops.
- [ ] Go: module path and release cadence (single module vs submodules), and the gRPC dependency baseline.
- [ ] Go: required Go version and policy for context cancellation vs retries in poll/renew/ack.
- [ ] Node: runtime targets (Node LTS only vs Node + Bun) and ESM/CJS packaging strategy.
- [ ] Node: gRPC stack choice (`@grpc/grpc-js`) and minimum supported version.
- [ ] Python: minimum supported version (3.10+?) and sync vs async surface.
- [ ] Python: gRPC dependency pinning strategy (grpcio + protobuf).

## F. Server-side implementation

- [ ] Implement gRPC `Worker.Connect` semantics for streaming assignments and ensure parity with HTTP work endpoints.
- [ ] Persist `executionMode` + `invocationSource` in work items and propagate to logs/metrics.
- [ ] Enforce runner test policy server-side (do not dispatch tests to runners without `allowTestExecutions`).
- [ ] Emit structured logs/metrics for:
  - transport selection + fallback transitions
  - test execution acceptance/rejection
  - warning log on rejection for the initiating API call

## G. UI requirements (manual invoke visibility)

- [ ] Clearly label manual invocations for webhooks/schedules/jobs in UI.
- [ ] Distinguish test vs normal invoke in UI activity timelines and execution detail.
- [ ] When a runner rejects a test execution, show a warning badge or toast in the initiating UI flow.
- [ ] Update UI activity sources to include `invoke:test` or equivalent mapping (align with `invocationSource`/`executionMode`).

## H. Samples & documentation

- [ ] Move runner SDKs out of samples into a dedicated SDK folder (e.g., `sdk/worker-go`, `sdk/worker-node`, `sdk/worker-python`, `sdk/worker-dotnet`).
- [ ] Place runner samples under `samples/runners/<language>/<name>` and wire them into the AppHost via opt-in profiles (per `docs/deep-dive/designs/samples-to-aspire-hosts.md`).
- [ ] Register one runner per language in the Aspire Devstack (AppHost profiles) so each SDK has a runnable dev example (P0/blocker).
- [ ] Expand `docs/guides/workers-runners.md` with transport fallback behavior and test execution semantics.
- [ ] Update `docs/deep-dive/sdk-worker-integration.md` with new env vars and rejection rules.
- [ ] Update `docs/deep-dive/designs/polyglot-worker-protocol.md` to include `executionMode` and the gRPC + polling fallback.
- [ ] Cross-link changes in `docs/index.md` and `docs/feature-map.md` if needed.

## I. Testing checklist

- [ ] Contract tests for gRPC/polling parity (claim/ack/events) including runner mismatch and lease conflicts.
- [ ] Test rejection path: test invoke rejected -> warning logged on initiator + UI warning surfaced.
- [ ] Idempotency and lease-conflict scenarios across transports.
- [ ] Fallback chain e2e test: gRPC down -> polling.
- [ ] Outbox durability: restart with pending ack/events -> replay without duplicates.

## J. Node consumer example (script)

```ts
// Example consumer script for the Node runner SDK.
// This assumes a proposed SDK shape; adjust names once the SDK is finalized.

import { CroniqRunner } from "@croniq/worker-sdk";

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

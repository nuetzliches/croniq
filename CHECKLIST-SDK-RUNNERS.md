# Checklist: Polyglot Runner SDKs (gRPC → Polling)

Goal: deliver smart Runner SDKs that act as job executors with a fallback chain (gRPC, polling) across .NET, Go, Node, and Python. Align with existing guidance in docs and close the gaps noted below.

## Sources reviewed

- docs/guides/workers-runners.md
- docs/deep-dive/designs/polyglot-worker-protocol.md
- docs/deep-dive/sdk-worker-integration.md
- docs/deep-dive/architecture.md
- src/Croniq.Ui/docs/deep-dive/ui.md (manual invoke roadmap + activity source flags)

## A. Design gaps to resolve (docs vs goal)

- [ ] Confirm gRPC-first with HTTP polling fallback for runners (SSE is UI-only per current architecture).
- [ ] Align docs/deep-dive/architecture.md and docs/deep-dive/designs/polyglot-worker-protocol.md with the gRPC + polling transport mapping and fallback rules.

## B. Contract changes (API + protocol)

- [ ] Add explicit execution intent to work items (e.g., `executionMode: normal|test`) so runners can distinguish test runs.
- [ ] Extend work lease payload and gRPC messages with `executionMode` and `invocationSource` (schedule, webhook ingress, manual invoke, test invoke).
- [ ] Add a runner capability/setting for `allowTestExecutions` (default: false or explicit opt-in).
- [ ] Define how a runner rejects a test execution (e.g., `AckFailure` with reason `test-not-allowed` or a dedicated reject message).
- [ ] Specify server-side behavior when a test call is rejected:
  - log a Warning on the triggering side (API/relay) with the rejection reason and target runner.
  - surface it in UI activity timelines as a warning event.
- [ ] Update OpenAPI and gRPC schemas to include new fields and rejection reasons.

## C. SDK behavior (shared requirements)

- [ ] Implement transport chain: gRPC streaming → HTTP polling.
- [ ] Standardize retry/backoff and reconnect with jitter for all transports.
- [ ] Ensure lease renewals keep running regardless of transport (in-flight work must not depend on active stream).
- [ ] Honor `executionMode` and runner policy (reject tests when disallowed).
- [ ] Support outbox persistence for ack/events (per sdk-worker-integration.md).
- [ ] Provide uniform configuration contract across SDKs:
  - Required: CRONIQ_API_BASEURL, CRONIQ_TENANT_ID, CRONIQ_ENVIRONMENT, CRONIQ_API_KEY|CRONIQ_BEARER_TOKEN, CRONIQ_RUNNER_ID
  - Optional: CRONIQ*POLL_BATCH_SIZE, CRONIQ_POLL_WAIT_MS, CRONIQ_REQUEST_TIMEOUT_MS, CRONIQ_RENEW_LEAD_MS, CRONIQ_RETRY*\* parameters
  - New: CRONIQ_TRANSPORT_MODE (auto/grpc/polling), CRONIQ_ALLOW_TEST_EXECUTIONS

## D. Open questions per SDK

- [ ] .NET: should the runner SDK live inside `Croniq.Sdk` or ship as a separate package (e.g., `Croniq.Worker.Sdk`)?
- [ ] .NET: expected DI + logging integration surface (host builder extensions vs lightweight client).
- [ ] Go: module path and release cadence (single module vs submodules), and the gRPC dependency baseline.
- [ ] Go: required Go version and policy for context cancellation vs retries in poll/renew/ack.
- [ ] Node: runtime targets (Node LTS only vs Node + Bun) and ESM/CJS packaging strategy.
- [ ] Node: gRPC stack choice (`@grpc/grpc-js`) and minimum supported version.
- [ ] Python: minimum supported version (3.10+?) and sync vs async surface.
- [ ] Python: gRPC dependency pinning strategy (grpcio + protobuf).

## E. Server-side implementation

- [ ] Implement gRPC `Worker.Connect` semantics for streaming assignments and ensure parity with HTTP work endpoints.
- [ ] Enforce runner test policy at the server if client-side rejection is not sufficient.
- [ ] Emit structured logs/metrics for:
  - transport selection + fallback transitions
  - test execution acceptance/rejection
  - warning log on rejection for the initiating API call

## F. UI requirements (manual invoke visibility)

- [ ] Clearly label manual invocations for webhooks/schedules/jobs in UI.
- [ ] Distinguish test vs normal invoke in UI activity timelines and execution detail.
- [ ] When a runner rejects a test execution, show a warning badge or toast in the initiating UI flow.
- [ ] Update UI activity sources to include `invoke:test` or similar (already supports `source=ingress|invoke` in webhook activity).

## G. Samples & documentation

- [ ] Move runner SDKs out of samples into a dedicated SDK folder (e.g., sdk/worker-go, sdk/worker-node, sdk/worker-python, sdk/worker-dotnet).
- [ ] Update SDK-owned examples alongside the SDK releases (per docs/deep-dive/designs/samples-to-aspire-hosts.md).
- [ ] Expand docs/guides/workers-runners.md with transport fallback behavior and test execution semantics.
- [ ] Update docs/deep-dive/sdk-worker-integration.md with new env vars and rejection rules.
- [ ] Update docs/deep-dive/designs/polyglot-worker-protocol.md to include executionMode and the gRPC + polling fallback.
- [ ] Cross-link changes in docs/index.md and docs/feature-map.md if needed.

## H. Testing checklist

- [ ] Contract tests for gRPC/polling parity (claim/ack/events).
- [ ] Test rejection path: test invoke rejected → warning logged on initiator.
- [ ] Idempotency and lease-conflict scenarios across transports.
- [ ] Fallback chain e2e test: gRPC down → polling.

## I. Node consumer example (script)

```ts
// Example consumer script for the Node runner SDK.
// This assumes a proposed SDK shape; adjust names once the SDK is finalized.

import { CroniqRunner } from "@croniq/worker-sdk";

const config = {
  apiBaseUrl: process.env.CRONIQ_API_BASEURL,
  tenantId: process.env.CRONIQ_TENANT_ID,
  environment: process.env.CRONIQ_ENVIRONMENT,
  apiKey: process.env.CRONIQ_API_KEY,
  bearerToken: process.env.CRONIQ_BEARER_TOKEN,
  runnerId: process.env.CRONIQ_RUNNER_ID,
  transportMode: process.env.CRONIQ_TRANSPORT_MODE ?? "auto",
  allowTestExecutions: process.env.CRONIQ_ALLOW_TEST_EXECUTIONS === "true",
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

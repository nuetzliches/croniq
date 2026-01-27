# Checklist: Open Question Decisions Implementation

## Metrics in API host

- [x] Rename runner transport selection metric to `croniq.runner.transport.selection_total`.
- [x] Rename runner transport fallback metric to `croniq.runner.transport.fallback_total`.
- [x] Emit test rejection metric as `croniq.runner.test.reject_total`.
- [x] Add polling active gauge `croniq.runner.transport.polling_active`.
- [x] Emit `croniq.runner.transport.grpc_unavailable_total` when a runner gRPC stream ends with `StatusCode.Unavailable`.

## Samples wiring

- [x] Runner samples are already wired into AppHost profiles.
- [x] Remove legacy samples under samples/grpc-client-_ and samples/worker-sdk-_.
- [x] Remove empty legacy sample directories.

## UI warning surfacing

- [x] Verify schedules logs and webhook timelines/trigger details show test rejection warnings.

## Documentation follow-ups

- [x] Update docs to reflect metric names and UI warning surfacing decisions.

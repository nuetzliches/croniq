# Croniq Runner SDK for Go

[![Go Reference](https://pkg.go.dev/badge/github.com/nuetzliches/croniq/sdks/go.svg)](https://pkg.go.dev/github.com/nuetzliches/croniq/sdks/go)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Build job execution runners for [Croniq](https://github.com/nuetzliches/croniq) in Go. The SDK polls a Croniq server for work, dispatches typed handlers, streams structured logs back, and reports completion — with `context.Context` propagation throughout and zero dependencies in the core (single yaml import in the conformance suite).

## Install

```sh
go get github.com/nuetzliches/croniq/sdks/go@latest
# optional: opt-in OpenTelemetry adapter (separate module)
go get github.com/nuetzliches/croniq/sdks/go/otel@latest
```

Minimum Go version: **1.22** (for `log/slog`, `for range over int`, and modern stdlib affordances).

## Quick start

```go
package main

import (
    "context"
    "log/slog"
    "os"
    "os/signal"
    "syscall"

    croniq "github.com/nuetzliches/croniq/sdks/go"
)

func main() {
    ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
    defer stop()

    r := croniq.NewRunner(
        "http://localhost:4000",
        croniq.ResolveRunnerID("my-runner"),
        croniq.WithAPIKey(os.Getenv("CRONIQ_API_KEY")),
        croniq.WithCapabilities("billing"),
        croniq.WithMaxInflight(5),
    )

    r.Register("billing:invoice", func(ctx context.Context, ec *croniq.ExecutionContext) error {
        slog.InfoContext(ctx, "processing", "execution_id", ec.ExecutionID, "attempt", ec.Attempt)
        return nil
    })

    if err := r.Run(ctx); err != nil {
        slog.Error("runner exited", "error", err)
        os.Exit(1)
    }
}
```

See [`examples/quickstart`](examples/quickstart/main.go) for the full template.

## Features

- **Idiomatic Go** — goroutines + `chan` (1:1 mapping to the Rust SDK's `mpsc`), `context.Context` propagation, `log/slog` for structured logs.
- **Server-side cancellation** — `PollResponse.cancel` is wired into per-execution `context.CancelFunc` so the handler's ctx fires on server-initiated cancel.
- **Streaming log writer** — `ec.LogWriter()` buffers events into a bounded channel; a flusher goroutine batches them (32 events / 200 ms / max 100 per POST) and drains before ack. Use this when a handler wraps a chatty subprocess.
- **Lease renewal** — per-execution ticker posts to `/v1/work/renew` until the handler returns.
- **Self-registration** — `RegisterWithSchedule("billing:invoice", "5m", fn)` calls `POST /v1/jobs/register` at startup; Croniqfile-managed jobs take precedence.
- **Catch-all handler** — `r.SetDefaultHandler(...)` for runners that handle any job_key.
- **Middleware** — `croniq.WithMiddleware(...)` for tracing, recovery, metrics, etc.
- **Persistent runner identity** — `ResolveRunnerID(prefix)` reads `RUNNER_ID` / `${CRONIQ_RUNNER_DATA_DIR}/runner-id` / generates and persists, matching the Rust shell-runner's volume behaviour.
- **Drain-on-shutdown** — cancelling `Run`'s context stops polling but lets in-flight handlers finish naturally up to `WithDrainTimeout`; past the budget remaining handlers are cancelled.
- **On-demand triggering (producer)** — `croniq.NewTriggerClient(...)` wraps `POST /v1/trigger` with its own credentials (the `jobs:trigger` scope), independent of the runner. See [Triggering jobs](#triggering-jobs-producer).

## Capabilities vs Tags

A common pitfall: **don't put implementation details into capabilities**. Capabilities drive job routing (`require`/`prefer` in the Croniqfile). Tags are filter-only — for UI and operations, not routing.

| Good capability | Bad capability |
|---|---|
| `billing`, `reporting`, `gpu`, `sandboxed` | `go`, `linux-amd64`, `kubernetes` |

If your runner is Go-based, put that into **tags** (`lang=go`, `platform=linux-amd64`) so a future Rust- or .NET-runner with the same business capabilities can take over without rewriting Croniqfile entries.

## Triggering jobs (producer)

Runners are the *consumer* side of Croniq. The *producer* side — firing a job on demand, e.g. in response to an application event — is a separate, first-class client that wraps `POST /v1/trigger`:

```go
tc := croniq.NewTriggerClient("http://localhost:4000").
    WithAPIKey(os.Getenv("CRONIQ_TRIGGER_KEY"))

resp, err := tc.Trigger(ctx, &croniq.TriggerRequest{
    JobKey:         "billing:invoice",
    Metadata:       map[string]any{"invoice_id": "inv_42"},
    Require:        []string{"billing"},
    Timeout:        "10m",
    IdempotencyKey: "evt-2026-07-14-001", // optional dedup key
})
if err != nil {
    // A *croniq.ServerError carries the HTTP status — e.g. 429 when the
    // job is at its per-job queue-overflow cap (max_queue_depth).
    var se *croniq.ServerError
    if errors.As(err, &se) && se.Status == http.StatusTooManyRequests {
        // observe backpressure: back off / retry later
    }
    return err
}
slog.Info("triggered",
    "execution_id", resp.ExecutionID,
    "queued", resp.Queued,
    "deduplicated", resp.Deduplicated,
)
```

- **Separate credentials.** Triggering requires the `jobs:trigger` (or `admin`) scope, which runner poll keys typically don't carry — so `TriggerClient` takes its own API key / bearer token (`WithAPIKey` / `WithBearer`) and is fully independent of `NewRunner`.
- **Unset optionals are omitted.** `Metadata`, `Require`, `Prefer`, `Timeout`, and `IdempotencyKey` are left out of the request body entirely when unset — the server applies its own defaults; the producer never fabricates them on the wire.
- **Metadata is arbitrary JSON.** `map[string]any` is forwarded to the handler verbatim, nested objects and non-string values preserved (not flattened or stringified).
- **Idempotency.** Supply `IdempotencyKey` to dedup at-least-once producers (event redelivery, client retries, concurrent publishers). A repeat trigger with the same `(JobKey, IdempotencyKey)` returns the existing `ExecutionID` with `Deduplicated: true` instead of enqueuing again. Servers without idempotency support omit the flag; it then reads as `false`.
- **Backpressure.** A non-2xx response is returned as `*croniq.ServerError`; inspect `.Status` (e.g. `429`) to observe the per-job queue-overflow cap rather than piling work up unbounded.

See [`examples/trigger`](examples/trigger/main.go) for a runnable producer.

## Streaming logs

For long-running handlers that emit a lot of output, use the writer. It buffers events in a bounded channel; a background goroutine batches and POSTs them. The runner drains the writer before sending the ack, so every queued event is server-side by the time the execution is marked complete.

```go
r.Register("backup:nightly", func(ctx context.Context, ec *croniq.ExecutionContext) error {
    w := ec.LogWriter()

    cmd := exec.CommandContext(ctx, "pg_dump", "app")
    stdout, _ := cmd.StdoutPipe()
    _ = cmd.Start()

    scanner := bufio.NewScanner(stdout)
    for scanner.Scan() {
        w.Send(ctx, "info", scanner.Text())
    }
    return cmd.Wait()
})
```

## OpenTelemetry

The OTel adapter is a separate module so the core stays dependency-light:

```go
import (
    "go.opentelemetry.io/otel"
    croniq "github.com/nuetzliches/croniq/sdks/go"
    croniqotel "github.com/nuetzliches/croniq/sdks/go/otel"
)

tracer := otel.Tracer("my-service")
r := croniq.NewRunner(serverURL, runnerID,
    croniq.WithMiddleware(croniqotel.TracingMiddleware(tracer)),
)
```

Span name: `croniq.execute {job_key}`. Attributes: `croniq.job.key`, `croniq.execution.id`, `croniq.execution.attempt`, `croniq.runner.id`, `croniq.execution.outcome`.

## Wire-protocol conformance

The SDK is validated against the shared, language-agnostic conformance suite at [`sdks/conformance/`](../conformance/) — the same YAML cases that gate the .NET SDK. Run them locally:

```sh
cd sdks/go && go test ./conformance/...
```

When the wire protocol gains a new behaviour the YAML case is added to `sdks/conformance/cases/` first — every SDK author then sees the same definition-of-done. Producer (trigger) cases live alongside in `sdks/conformance/cases-trigger/` and drive the `TriggerClient` instead of a runner loop.

## Compatibility matrix

| SDK Version | Croniq Server (min) | Croniq Server (max tested) |
|-------------|---------------------|----------------------------|
| 0.1.x       | 0.14.0              | 0.14.0                     |

## Releasing

Go modules are released by pushing git tags — there is no central registry. The two modules ship independently:

- Core SDK: `sdks/go/vX.Y.Z`
- OTel adapter: `sdks/go/otel/vX.Y.Z`

The [`Go SDK Release`](../../.github/workflows/go-sdk-release.yml) workflow validates, warms `proxy.golang.org`, and creates a GitHub Release on tag push. See [`RELEASING.md`](RELEASING.md) for the bootstrap procedure.

## License

Dual-licensed under MIT OR Apache-2.0. See [LICENSE-MIT](../../LICENSE-MIT) and [LICENSE-APACHE](../../LICENSE-APACHE).

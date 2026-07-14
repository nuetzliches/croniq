// Package croniq is the official Go SDK for building [Croniq] runners.
//
// A runner polls the Croniq server for work, dispatches typed handlers,
// streams structured logs back, and reports completion. Schedules and
// retry policies live in the Croniqfile on the server — the SDK only
// concerns itself with execution.
//
// # Quick start
//
//	package main
//
//	import (
//	    "context"
//	    "log/slog"
//	    "os"
//	    "os/signal"
//	    "syscall"
//
//	    croniq "github.com/nuetzliches/croniq/sdks/go"
//	)
//
//	func main() {
//	    ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
//	    defer stop()
//
//	    r := croniq.NewRunner("http://localhost:4000", "demo-runner",
//	        croniq.WithAPIKey(os.Getenv("CRONIQ_API_KEY")),
//	        croniq.WithCapabilities("demo"),
//	        croniq.WithMaxInflight(5),
//	    )
//
//	    r.Register("hello:world", func(ctx context.Context, ec *croniq.ExecutionContext) error {
//	        slog.InfoContext(ctx, "processing", "execution_id", ec.ExecutionID, "attempt", ec.Attempt)
//	        return nil
//	    })
//
//	    if err := r.Run(ctx); err != nil {
//	        slog.Error("runner exited", "error", err)
//	        os.Exit(1)
//	    }
//	}
//
// # Triggering jobs (producer)
//
// Runners are the consumer side of Croniq. To fire a job on demand — the
// producer side — use [TriggerClient], a first-class wrapper over
// POST /v1/trigger that is independent of [Runner] and carries its own
// credentials (the jobs:trigger scope):
//
//	tc := croniq.NewTriggerClient("http://localhost:4000").
//	    WithAPIKey(os.Getenv("CRONIQ_TRIGGER_KEY"))
//
//	resp, err := tc.Trigger(ctx, &croniq.TriggerRequest{
//	    JobKey:         "billing:invoice",
//	    IdempotencyKey: "evt-2026-07-14-001",
//	})
//
// Unset optional fields are omitted from the request body; a repeat trigger
// carrying the same idempotency_key surfaces the existing execution with
// TriggerResponse.Deduplicated set. A non-2xx response (including the 429
// per-job queue-overflow cap) is returned as a [*ServerError].
//
// # Streaming logs
//
// For long-running handlers that emit a lot of output, use
// [ExecutionContext.LogWriter] to buffer events into a bounded channel
// and let a background goroutine batch-POST them to the server. The
// runner drains the writer before sending the ack so no events are lost.
//
// # OpenTelemetry
//
// Tracing is opt-in via the sibling [otel] package:
//
//	import croniqotel "github.com/nuetzliches/croniq/sdks/go/otel"
//
//	r := croniq.NewRunner(...,
//	    croniq.WithMiddleware(croniqotel.Tracing()),
//	)
//
// [Croniq]: https://github.com/nuetzliches/croniq
// [otel]: https://pkg.go.dev/github.com/nuetzliches/croniq/sdks/go/otel
package croniq

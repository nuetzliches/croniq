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

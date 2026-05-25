// Quickstart example for the Croniq Go runner SDK.
//
// Run against a local server (e.g. `docker compose up`):
//
//	CRONIQ_API_KEY=croniq_… go run ./sdks/go/examples/quickstart
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
	slog.SetDefault(slog.New(slog.NewTextHandler(os.Stderr, &slog.HandlerOptions{Level: slog.LevelInfo})))

	ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer stop()

	r := croniq.NewRunner(
		envOr("CRONIQ_SERVER_URL", "http://localhost:4000"),
		croniq.ResolveRunnerID("go-quickstart"),
		croniq.WithAPIKey(os.Getenv("CRONIQ_API_KEY")),
		croniq.WithCapabilities("demo"),
		croniq.WithTags("lang=go", "env=dev"),
		croniq.WithMaxInflight(5),
	)

	// Direct logging — the SDK enriches each event with job_key /
	// runner_id / runner_tags so log queries can filter without the
	// call site threading values through.
	r.Register("hello:world", func(ctx context.Context, ec *croniq.ExecutionContext) error {
		slog.InfoContext(ctx, "processing",
			"execution_id", ec.ExecutionID,
			"attempt", ec.Attempt,
		)
		ec.Log(ctx, "info", "hello from go runner")
		return nil
	})

	// Streaming logs — for handlers that produce a lot of output, use
	// the writer to avoid HTTP backpressure in your inner loop.
	r.Register("noisy:job", func(ctx context.Context, ec *croniq.ExecutionContext) error {
		w := ec.LogWriter()
		for i := 0; i < 200; i++ {
			w.Send(ctx, "info", "stream line")
		}
		return nil
	})

	// Catch-all — handle any job_key the operator points at this runner.
	r.SetDefaultHandler(func(ctx context.Context, ec *croniq.ExecutionContext) error {
		slog.WarnContext(ctx, "default handler hit", "job_key", ec.JobKey)
		return nil
	})

	if err := r.Run(ctx); err != nil {
		slog.Error("runner exited", "error", err)
		os.Exit(1)
	}
}

func envOr(key, fallback string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return fallback
}

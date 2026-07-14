// Producer example for the Croniq Go SDK: fire a job on demand via the
// trigger (producer) client.
//
// Triggering requires the jobs:trigger (or admin) scope — a credential
// distinct from a runner's poll key. Run against a local server:
//
//	CRONIQ_TRIGGER_KEY=croniq_… go run ./sdks/go/examples/trigger billing:invoice
package main

import (
	"context"
	"errors"
	"log/slog"
	"net/http"
	"os"
	"time"

	croniq "github.com/nuetzliches/croniq/sdks/go"
)

func main() {
	slog.SetDefault(slog.New(slog.NewTextHandler(os.Stderr, &slog.HandlerOptions{Level: slog.LevelInfo})))

	jobKey := "hello:world"
	if len(os.Args) > 1 {
		jobKey = os.Args[1]
	}

	tc := croniq.NewTriggerClient(envOr("CRONIQ_SERVER_URL", "http://localhost:4000")).
		WithAPIKey(os.Getenv("CRONIQ_TRIGGER_KEY"))

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	resp, err := tc.Trigger(ctx, &croniq.TriggerRequest{
		JobKey:   jobKey,
		Metadata: map[string]any{"source": "go-trigger-example"},
		// IdempotencyKey dedups at-least-once producers; drop it for a
		// plain fire-and-forget trigger.
		IdempotencyKey: "example-" + time.Now().UTC().Format("20060102"),
	})
	if err != nil {
		// A *croniq.ServerError exposes the HTTP status — 429 means the job
		// is at its per-job queue-overflow cap (max_queue_depth); back off.
		var se *croniq.ServerError
		if errors.As(err, &se) && se.Status == http.StatusTooManyRequests {
			slog.Warn("job at queue-overflow cap — back off and retry later", "job_key", jobKey)
			os.Exit(1)
		}
		slog.Error("trigger failed", "job_key", jobKey, "error", err)
		os.Exit(1)
	}

	slog.Info("triggered",
		"job_key", jobKey,
		"execution_id", resp.ExecutionID,
		"queued", resp.Queued,
		"deduplicated", resp.Deduplicated,
	)
}

func envOr(key, fallback string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return fallback
}

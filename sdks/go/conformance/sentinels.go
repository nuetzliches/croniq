package conformance

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"time"

	croniq "github.com/nuetzliches/croniq/sdks/go"
)

// ApplyHandlers registers each handler in `handlers` on the runner
// using the binding's standard sentinel set: noop / throw / sleep / log
// / stream_logs. The semantics match the .NET binding so the same YAML
// cases drive both.
func ApplyHandlers(r *croniq.Runner, handlers []HandlerSpec) error {
	for _, h := range handlers {
		fn, err := sentinelFor(h)
		if err != nil {
			return err
		}
		switch {
		case h.IsDefault:
			r.SetDefaultHandler(fn)
		case h.Schedule != "":
			r.RegisterWithSchedule(h.JobKey, h.Schedule, fn)
		default:
			r.Register(h.JobKey, fn)
		}
	}
	return nil
}

func sentinelFor(h HandlerSpec) (croniq.HandlerFunc, error) {
	switch h.Behavior {
	case "noop":
		return func(_ context.Context, _ *croniq.ExecutionContext) error {
			return nil
		}, nil

	case "throw":
		msg := h.ErrorMessage
		if msg == "" {
			msg = "thrown by conformance handler"
		}
		return func(_ context.Context, _ *croniq.ExecutionContext) error {
			return errors.New(msg)
		}, nil

	case "sleep":
		d := time.Duration(h.DurationMs) * time.Millisecond
		return func(ctx context.Context, _ *croniq.ExecutionContext) error {
			select {
			case <-time.After(d):
				return nil
			case <-ctx.Done():
				return ctx.Err()
			}
		}, nil

	case "log":
		level := normaliseLevel(h.Level)
		count := h.Count
		if count < 1 {
			count = 1
		}
		msg := h.Message
		return func(ctx context.Context, ec *croniq.ExecutionContext) error {
			for i := 0; i < count; i++ {
				ec.Log(ctx, level, msg)
				_ = slog.Default()
			}
			return nil
		}, nil

	case "stream_logs":
		level := normaliseLevel(h.Level)
		count := h.Count
		if count < 1 {
			count = 1
		}
		interval := time.Duration(h.IntervalMs) * time.Millisecond
		return func(ctx context.Context, ec *croniq.ExecutionContext) error {
			w := ec.LogWriter()
			for i := 0; i < count; i++ {
				w.Send(ctx, level, fmt.Sprintf("line %d", i+1))
				if interval > 0 && i+1 < count {
					select {
					case <-time.After(interval):
					case <-ctx.Done():
						return ctx.Err()
					}
				}
			}
			return nil
		}, nil

	default:
		return nil, fmt.Errorf("unknown handler behavior %q", h.Behavior)
	}
}

func normaliseLevel(s string) string {
	switch s {
	case "trace", "debug", "info", "warn", "error":
		return s
	default:
		return "info"
	}
}

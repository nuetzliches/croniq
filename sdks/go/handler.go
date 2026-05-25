package croniq

import (
	"context"
	"encoding/json"
	"log/slog"
	"sync"
)

// HandlerFunc is the function signature every job handler implements.
//
// The provided ctx is cancelled when the server requests cancellation
// via PollResponse.cancel or when the runner's drain timeout elapses.
// Handlers should propagate ctx into any subprocess, HTTP call, or
// channel read they perform.
type HandlerFunc func(ctx context.Context, ec *ExecutionContext) error

// ExecutionContext is the per-execution value passed to every handler.
// It carries the assignment metadata and an HTTP client scoped to the
// execution's id (so log writes target /v1/work/{this-execution}/events).
type ExecutionContext struct {
	ExecutionID string
	JobKey      string
	Attempt     int
	Metadata    json.RawMessage
	Timeout     string
	RunnerID    string
	RunnerTags  []string

	client *Client

	// Streaming log writer is lazily initialised on the first
	// LogWriter() call so handlers that don't need streaming pay no
	// cost. All clones share one flusher goroutine — the runner awaits
	// its drain before sending the final ack.
	logWriterOnce sync.Once
	logWriter     *LogWriter
}

// Log pushes a single log event for this execution. Errors are logged
// at warn level and swallowed — log delivery is best-effort and never
// the critical path. For high-volume scenarios, prefer [LogWriter].
func (ec *ExecutionContext) Log(ctx context.Context, level, message string) {
	ev := WorkEvent{Level: level, Message: message}
	if err := ec.PushEvents(ctx, []WorkEvent{ev}); err != nil {
		slog.WarnContext(ctx, "failed to push log event",
			"execution_id", ec.ExecutionID,
			"error", err,
		)
	}
}

// PushEvents pushes a batch of structured log events for this execution.
// Three fields are auto-injected into every event's `fields` so log
// queries can filter without the call site threading values through:
//
//   - job_key — the job that produced the event
//   - runner_id — which runner instance handled it
//   - runner_tags — JSON-encoded array of the runner's self-declared tags
//     (omitted when the runner has no tags)
//
// Existing keys in the caller's event are preserved — explicit values win.
//
// This call awaits the HTTP POST inline. For high-volume long-running
// jobs that stream stdout/stderr line by line, prefer [LogWriter], which
// buffers events and posts them asynchronously.
func (ec *ExecutionContext) PushEvents(ctx context.Context, events []WorkEvent) error {
	if len(events) == 0 {
		return nil
	}
	tags := serializeTags(ec.RunnerTags)
	enriched := make([]WorkEvent, len(events))
	for i, ev := range events {
		enriched[i] = enrichEvent(ev, ec.JobKey, ec.RunnerID, tags)
	}
	return ec.client.PushEvents(ctx, ec.ExecutionID, enriched)
}

// LogWriter returns a cloneable streaming log writer for this execution.
//
// The writer enqueues events into a bounded channel; a background
// goroutine batches and POSTs them to the server. Calls only suspend
// on channel capacity, never on HTTP, so a long-running subprocess's
// stdout reader will not deadlock when the server is slow.
//
// The first call spawns the flusher goroutine. Subsequent calls — and
// clones of the returned [LogWriter] — share that single goroutine.
// The runner awaits the writer's drain (bounded by drainTimeout) before
// sending the final ack for this execution, so all queued events are
// server-side by the time the execution is marked complete.
//
// Mixing with [ExecutionContext.Log] / [ExecutionContext.PushEvents]
// works, but the server may receive events out-of-order relative to
// client-side issue order because timestamps are assigned on receipt.
// For strict ordering, pick one path per handler.
func (ec *ExecutionContext) LogWriter() *LogWriter {
	ec.logWriterOnce.Do(func() {
		ec.logWriter = newLogWriter(ec.client, ec.ExecutionID, ec.JobKey, ec.RunnerID, ec.RunnerTags)
	})
	return ec.logWriter
}

// handlerEntry is a registered handler with metadata used by the
// runner's lookup logic.
type handlerEntry struct {
	fn HandlerFunc
}

type handlerRegistry struct {
	mu       sync.RWMutex
	handlers map[string]handlerEntry
	def      *handlerEntry
}

func newHandlerRegistry() *handlerRegistry {
	return &handlerRegistry{handlers: make(map[string]handlerEntry)}
}

func (r *handlerRegistry) register(jobKey string, fn HandlerFunc) {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.handlers[jobKey] = handlerEntry{fn: fn}
}

func (r *handlerRegistry) setDefault(fn HandlerFunc) {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.def = &handlerEntry{fn: fn}
}

func (r *handlerRegistry) get(jobKey string) (HandlerFunc, bool) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	if h, ok := r.handlers[jobKey]; ok {
		return h.fn, true
	}
	if r.def != nil {
		return r.def.fn, true
	}
	return nil, false
}

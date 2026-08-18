package croniq

import (
	"context"
	"log/slog"
	"sync"
	"time"
)

// Tunables — chosen to mirror the Rust/.NET SDKs.
const (
	// logChannelCapacity admits roughly one second of typical bursty
	// output (~250 lines/sec) before producers feel backpressure —
	// big enough that a chatty test suite doesn't stall on every event,
	// small enough that genuine server slowness produces backpressure
	// instead of unbounded memory growth.
	logChannelCapacity = 256

	// logBatchSizeThreshold is the buffered-event count that triggers
	// an immediate flush.
	logBatchSizeThreshold = 32

	// logBatchTimeThreshold is the maximum time the flusher will hold
	// events before posting. Sub-second cadence keeps the live-progress
	// feel.
	logBatchTimeThreshold = 200 * time.Millisecond

	// logMaxBatchPerPost caps events per HTTP POST. A single chatty
	// wake-up that fills the channel gets posted in chunks rather than
	// one huge body.
	logMaxBatchPerPost = 100

	// logShutdownTimeout is the wall-clock budget for ShutdownAndDrain.
	// If the server is unreachable at job-end time, the runner moves on
	// to ack after this budget regardless — losing late events but not
	// blocking the entire dispatch loop.
	logShutdownTimeout = 5 * time.Second
)

// pusher is the minimal interface the flusher uses to post batches.
// Used by tests to substitute a recording mock.
type pusher interface {
	PushEvents(ctx context.Context, executionID string, events []WorkEvent) error
}

// LogWriter is a cloneable, fire-and-forget streaming log handle.
//
// Send enqueues an event into a bounded channel and returns as soon as
// the slot is allocated — never blocking on the server. Use Flush to
// wait for currently-queued events to be POSTed before continuing.
type LogWriter struct {
	tx chan logCmd

	mu       sync.Mutex
	shutdown chan struct{}
	done     chan struct{}
}

type logCmdKind int

const (
	cmdEvent logCmdKind = iota
	cmdFlush
)

type logCmd struct {
	kind  logCmdKind
	event WorkEvent
	ack   chan struct{}
}

// NullLogWriter constructs a writer that silently discards every event.
// Useful for unit tests or CLI tools where streaming is irrelevant.
func NullLogWriter() *LogWriter {
	w := &LogWriter{
		tx:       make(chan logCmd, 64),
		shutdown: make(chan struct{}),
		done:     make(chan struct{}),
	}
	go func() {
		defer close(w.done)
		for range w.tx {
			// drop on the floor
		}
	}()
	return w
}

// newLogWriter spawns the background flusher and returns the public
// handle.
func newLogWriter(client pusher, executionID, jobKey, runnerID string, runnerTags []string) *LogWriter {
	w := &LogWriter{
		tx:       make(chan logCmd, logChannelCapacity),
		shutdown: make(chan struct{}),
		done:     make(chan struct{}),
	}
	go w.flusherLoop(client, executionID, jobKey, runnerID, serializeTags(runnerTags))
	return w
}

// Send pushes a single log event with the given level and message.
// Same fire-and-forget semantics as SendEvent.
func (w *LogWriter) Send(ctx context.Context, level, message string) {
	w.SendEvent(ctx, WorkEvent{Level: level, Message: message})
}

// SendEvent pushes a fully-populated WorkEvent. The call may suspend on
// channel capacity (genuine server slowness) but never on HTTP. Returns
// when the event is enqueued or ctx is cancelled.
func (w *LogWriter) SendEvent(ctx context.Context, event WorkEvent) {
	select {
	case w.tx <- logCmd{kind: cmdEvent, event: event}:
	case <-ctx.Done():
	case <-w.shutdown:
		// Writer is shutting down — drop silently to match the
		// fire-and-forget contract.
	}
}

// Flush blocks until every event currently queued has been POSTed.
// Returns immediately if the flusher has already exited.
func (w *LogWriter) Flush(ctx context.Context) {
	ack := make(chan struct{})
	select {
	case w.tx <- logCmd{kind: cmdFlush, ack: ack}:
	case <-ctx.Done():
		return
	case <-w.done:
		return
	}
	select {
	case <-ack:
	case <-ctx.Done():
	case <-w.done:
	}
}

// shutdownAndDrain signals the flusher to drain queued events and exit,
// then waits up to logShutdownTimeout. Idempotent — repeat calls are
// no-ops.
func (w *LogWriter) shutdownAndDrain() {
	w.mu.Lock()
	select {
	case <-w.shutdown:
		w.mu.Unlock()
		return
	default:
		close(w.shutdown)
	}
	w.mu.Unlock()

	select {
	case <-w.done:
	case <-time.After(logShutdownTimeout):
		slog.Warn("log writer drain timed out — late events may be lost",
			"timeout", logShutdownTimeout,
		)
	}
}

func (w *LogWriter) flusherLoop(client pusher, executionID, jobKey, runnerID, serializedTags string) {
	defer close(w.done)

	buffer := make([]WorkEvent, 0, logBatchSizeThreshold)
	ticker := time.NewTicker(logBatchTimeThreshold)
	defer ticker.Stop()

	flush := func() {
		if len(buffer) == 0 {
			return
		}
		for len(buffer) > 0 {
			n := logMaxBatchPerPost
			if n > len(buffer) {
				n = len(buffer)
			}
			chunk := make([]WorkEvent, n)
			for i := 0; i < n; i++ {
				chunk[i] = enrichEvent(buffer[i], jobKey, runnerID, serializedTags)
			}
			buffer = buffer[n:]
			// Use a fresh context so individual POSTs aren't bound to
			// any in-flight caller's ctx; the surrounding runner enforces
			// the overall drain timeout.
			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			err := client.PushEvents(ctx, executionID, chunk)
			cancel()
			if err != nil {
				if isOwnershipDenied(err) {
					// Permanent (#436/#437) — every later batch is lost too,
					// so the operator must see this rather than wonder why
					// the execution produced no output.
					slog.Error("log writer batch POST refused with 403 Forbidden — this runner's credential does not own its runner_id, so no log event will reach the server; give the runner its own runner_id, or release the existing binding with DELETE /v1/runners/{id}",
						"execution_id", executionID,
						"dropped", len(chunk),
						"error", err,
					)
				} else {
					slog.Warn("log writer batch POST failed — events lost",
						"execution_id", executionID,
						"dropped", len(chunk),
						"error", err,
					)
				}
			}
		}
		// reset slice capacity so we don't accumulate after a large burst
		if cap(buffer) > logChannelCapacity {
			buffer = make([]WorkEvent, 0, logBatchSizeThreshold)
		}
	}

	for {
		select {
		case <-w.shutdown:
			// Drain anything left in the channel non-blockingly, then
			// flush and exit.
		drain:
			for {
				select {
				case cmd := <-w.tx:
					switch cmd.kind {
					case cmdEvent:
						buffer = append(buffer, cmd.event)
					case cmdFlush:
						close(cmd.ack)
					}
				default:
					break drain
				}
			}
			flush()
			return

		case cmd := <-w.tx:
			switch cmd.kind {
			case cmdEvent:
				buffer = append(buffer, cmd.event)
				if len(buffer) >= logBatchSizeThreshold {
					flush()
				}
			case cmdFlush:
				flush()
				close(cmd.ack)
			}

		case <-ticker.C:
			flush()
		}
	}
}

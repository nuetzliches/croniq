package croniq

import (
	"context"
	"errors"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

// mockPusher records every batch and lets tests inject failures / latency.
type mockPusher struct {
	mu        sync.Mutex
	posts     [][]WorkEvent
	failFirst atomic.Int32
	latency   time.Duration
}

func (m *mockPusher) PushEvents(_ context.Context, _ string, events []WorkEvent) error {
	if m.latency > 0 {
		time.Sleep(m.latency)
	}
	if m.failFirst.Load() > 0 {
		m.failFirst.Add(-1)
		return errors.New("mock failure")
	}
	m.mu.Lock()
	defer m.mu.Unlock()
	cp := make([]WorkEvent, len(events))
	copy(cp, events)
	m.posts = append(m.posts, cp)
	return nil
}

func (m *mockPusher) captured() [][]WorkEvent {
	m.mu.Lock()
	defer m.mu.Unlock()
	out := make([][]WorkEvent, len(m.posts))
	for i, p := range m.posts {
		out[i] = append([]WorkEvent(nil), p...)
	}
	return out
}

func (m *mockPusher) totalEvents() int {
	m.mu.Lock()
	defer m.mu.Unlock()
	n := 0
	for _, p := range m.posts {
		n += len(p)
	}
	return n
}

func spawnWriter(p pusher) *LogWriter {
	return newLogWriter(p, "exec-1", "job:test", "runner-1", []string{"env=test"})
}

func ev(msg string) WorkEvent {
	return WorkEvent{Level: "info", Message: msg}
}

func TestFlushesOnSizeThreshold(t *testing.T) {
	p := &mockPusher{}
	w := spawnWriter(p)

	for i := 0; i < logBatchSizeThreshold; i++ {
		w.SendEvent(context.Background(), ev("line"))
	}
	// Give the flusher a moment to react to the size trigger.
	time.Sleep(50 * time.Millisecond)
	w.shutdownAndDrain()

	posts := p.captured()
	if len(posts) != 1 {
		t.Fatalf("expected exactly one batch, got %d", len(posts))
	}
	if len(posts[0]) != logBatchSizeThreshold {
		t.Errorf("batch size = %d, want %d", len(posts[0]), logBatchSizeThreshold)
	}
	// Enrichment must have happened in-flight.
	if got := posts[0][0].Fields["job_key"]; got != "job:test" {
		t.Errorf("job_key not enriched: %q", got)
	}
	if got := posts[0][0].Fields["runner_id"]; got != "runner-1" {
		t.Errorf("runner_id not enriched: %q", got)
	}
	if got := posts[0][0].Fields["runner_tags"]; got != `["env=test"]` {
		t.Errorf("runner_tags not enriched: %q", got)
	}
}

func TestFlushesOnTimeThreshold(t *testing.T) {
	p := &mockPusher{}
	w := spawnWriter(p)

	// Push fewer events than the size threshold so only the time
	// trigger can flush them.
	for i := 0; i < 5; i++ {
		w.SendEvent(context.Background(), ev("line"))
	}
	time.Sleep(logBatchTimeThreshold + 100*time.Millisecond)
	w.shutdownAndDrain()

	posts := p.captured()
	if len(posts) < 1 {
		t.Fatalf("expected at least one batch, got 0")
	}
	if p.totalEvents() != 5 {
		t.Errorf("total events = %d, want 5", p.totalEvents())
	}
}

func TestFlushWaitsForPendingPost(t *testing.T) {
	p := &mockPusher{latency: 80 * time.Millisecond}
	w := spawnWriter(p)

	for i := 0; i < 5; i++ {
		w.SendEvent(context.Background(), ev("line"))
	}
	w.Flush(context.Background())

	if got := len(p.captured()); got != 1 {
		t.Errorf("after Flush expected 1 batch, got %d", got)
	}
	if p.totalEvents() != 5 {
		t.Errorf("total events after Flush = %d, want 5", p.totalEvents())
	}
	w.shutdownAndDrain()
}

func TestShutdownDrainsRemainingEvents(t *testing.T) {
	p := &mockPusher{}
	w := spawnWriter(p)

	for i := 0; i < 7; i++ {
		w.SendEvent(context.Background(), ev("line"))
	}
	// Don't wait for the time threshold — shutdown should drain.
	w.shutdownAndDrain()

	if p.totalEvents() != 7 {
		t.Errorf("total events = %d, want 7", p.totalEvents())
	}
}

func TestRespectsMaxBatchPerPost(t *testing.T) {
	p := &mockPusher{}
	w := spawnWriter(p)

	for i := 0; i < 250; i++ {
		w.SendEvent(context.Background(), ev("line"))
	}
	w.shutdownAndDrain()

	if p.totalEvents() != 250 {
		t.Errorf("total events = %d, want 250", p.totalEvents())
	}
	for _, batch := range p.captured() {
		if len(batch) > logMaxBatchPerPost {
			t.Errorf("batch of %d exceeded MAX_BATCH_PER_POST (%d)", len(batch), logMaxBatchPerPost)
		}
	}
}

func TestHTTPFailureDropsBatchKeepsFlusherAlive(t *testing.T) {
	p := &mockPusher{}
	p.failFirst.Store(1)
	w := spawnWriter(p)

	for i := 0; i < logBatchSizeThreshold; i++ {
		w.SendEvent(context.Background(), ev("first"))
	}
	time.Sleep(50 * time.Millisecond)
	for i := 0; i < 5; i++ {
		w.SendEvent(context.Background(), ev("second"))
	}
	w.shutdownAndDrain()

	if p.totalEvents() != 5 {
		t.Errorf("total events = %d, want 5 (first batch should be dropped)", p.totalEvents())
	}
}

func TestSendAfterShutdownIsSwallowed(t *testing.T) {
	p := &mockPusher{}
	w := spawnWriter(p)
	w.shutdownAndDrain()

	// Should not panic or hang.
	done := make(chan struct{})
	go func() {
		w.SendEvent(context.Background(), ev("late"))
		close(done)
	}()
	select {
	case <-done:
	case <-time.After(1 * time.Second):
		t.Fatal("SendEvent hung after shutdown")
	}
}

func TestNullLogWriter(t *testing.T) {
	w := NullLogWriter()
	// Should never block, never panic.
	for i := 0; i < 100; i++ {
		w.Send(context.Background(), "info", "line")
	}
}

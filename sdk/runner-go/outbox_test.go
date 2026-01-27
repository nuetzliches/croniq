package croniqrunner

import (
	"context"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"sync/atomic"
	"testing"
	"time"
)

func TestOutboxReplay_DoesNotDuplicate(t *testing.T) {
	var ackCalls int32
	var eventsCalls int32

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case r.Method == http.MethodPost && r.URL.Path == "/tenants/tenant/work/ack":
			atomic.AddInt32(&ackCalls, 1)
			w.WriteHeader(http.StatusNoContent)
		case r.Method == http.MethodPost && r.URL.Path == "/tenants/tenant/work/exec-1:events":
			atomic.AddInt32(&eventsCalls, 1)
			w.WriteHeader(http.StatusNoContent)
		default:
			w.WriteHeader(http.StatusNotFound)
		}
	}))
	t.Cleanup(server.Close)

	tempDir := t.TempDir()
	outboxPath := filepath.Join(tempDir, "runner-outbox.jsonl")

	config := RunnerConfig{
		Config: Config{
			BaseURL:        server.URL,
			TenantID:       "tenant",
			EnvironmentTag: "dev",
			ApiKey:         "ak_test",
		},
		RunnerId:         "runner-1",
		TransportMode:    TransportPolling,
		OutboxPath:       outboxPath,
		OutboxMaxEntries: 100,
		OutboxMaxBytes:   1_000_000,
	}

	runner, err := NewRunner(config)
	if err != nil {
		t.Fatalf("failed to create runner: %v", err)
	}

	lease := Lease{
		ExecutionId:       "exec-1",
		LeaseId:           "lease-1",
		TriggerId:         "trigger-1",
		JobKey:            "job-1",
		FireAtUtc:         time.Now().Add(-time.Minute),
		LeaseExpiresAtUtc: time.Now().Add(time.Minute),
	}

	runner.enqueueOutboxAckSuccess(lease)
	runner.enqueueOutboxEvents(lease, []WorkEvent{{Message: "hello"}})

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	go runner.replayOutboxLoop(ctx)

	waitForCount(t, &ackCalls, 1, time.Second*2)
	waitForCount(t, &eventsCalls, 1, time.Second*2)

	cancel()

	runner.outbox.Load()
	if len(runner.outbox.Items()) != 0 {
		t.Fatalf("expected outbox to be empty after replay")
	}
}

func waitForCount(t *testing.T, counter *int32, expected int32, timeout time.Duration) {
	deadline := time.Now().Add(timeout)
	for time.Now().Before(deadline) {
		if atomic.LoadInt32(counter) >= expected {
			return
		}
		time.Sleep(10 * time.Millisecond)
	}
	t.Fatalf("expected count %d but got %d", expected, atomic.LoadInt32(counter))
}

func TestOutboxReplay_PersistsAcrossRestart(t *testing.T) {
	tempDir := t.TempDir()
	outboxPath := filepath.Join(tempDir, "runner-outbox.jsonl")

	store := newOutboxStore(outboxPath, 100, 1_000_000)
	store.Enqueue(outboxEntry{ID: "1", Type: "ack_success", Payload: []byte("{}")})

	if _, err := os.Stat(outboxPath); err != nil {
		t.Fatalf("expected outbox file to exist: %v", err)
	}

	second := newOutboxStore(outboxPath, 100, 1_000_000)
	second.Load()
	items := second.Items()
	if len(items) != 1 {
		t.Fatalf("expected 1 outbox entry after reload, got %d", len(items))
	}
}

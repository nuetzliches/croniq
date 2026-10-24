package croniq

import (
	"context"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

// shrinkAckRetryDelays makes the K-1 ack retry fast for the duration of a
// test. Tests that use it must not run in parallel with each other.
func shrinkAckRetryDelays(t *testing.T) {
	t.Helper()
	saved := ackRetryDelays
	ackRetryDelays = []time.Duration{20 * time.Millisecond, 40 * time.Millisecond}
	t.Cleanup(func() { ackRetryDelays = saved })
}

// ackServer answers /v1/work/ack with the scripted statuses in order (the
// last one repeats). A status of 0 stands for a transport error: the
// connection is closed without a response.
func ackServer(t *testing.T, statuses ...int) (*httptest.Server, *atomic.Int32) {
	t.Helper()
	var acks atomic.Int32
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/v1/work/ack" {
			w.WriteHeader(http.StatusOK)
			_, _ = w.Write([]byte("{}"))
			return
		}
		n := int(acks.Add(1))
		status := statuses[len(statuses)-1]
		if n <= len(statuses) {
			status = statuses[n-1]
		}
		if status == 0 {
			conn, _, err := w.(http.Hijacker).Hijack()
			if err == nil {
				_ = conn.Close()
			}
			return
		}
		w.WriteHeader(status)
		_, _ = w.Write([]byte("{}"))
	}))
	t.Cleanup(srv.Close)
	return srv, &acks
}

func TestAckRetry(t *testing.T) {
	shrinkAckRetryDelays(t)

	cases := []struct {
		name     string
		statuses []int
		want     int32
	}{
		{"5xx then 200 is retried once", []int{503, 200}, 2},
		{"transport error then 200 is retried once", []int{0, 200}, 2},
		{"persistent 5xx stops after three attempts", []int{500}, 3},
		{"4xx is not retried", []int{409}, 1},
		{"403 is not retried", []int{403}, 1},
		{"200 is sent once", []int{200}, 1},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			srv, acks := ackServer(t, tc.statuses...)
			r := NewRunner(srv.URL, "test-runner", WithRequestTimeout(time.Second))
			r.ackResult(WorkAssignment{ExecutionID: "exec-1", JobKey: "job", Attempt: 1}, "success", "", 1)
			if got := acks.Load(); got != tc.want {
				t.Errorf("acks = %d, want %d", got, tc.want)
			}
		})
	}
}

// A request timeout is a transport error too: the held first attempt is
// given up after RequestTimeout and the second one is sent.
func TestAckRetryAfterRequestTimeout(t *testing.T) {
	shrinkAckRetryDelays(t)

	var acks atomic.Int32
	release := make(chan struct{})
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if acks.Add(1) == 1 {
			<-release
		}
		w.WriteHeader(http.StatusOK)
	}))
	defer srv.Close()
	defer close(release)

	r := NewRunner(srv.URL, "test-runner", WithRequestTimeout(100*time.Millisecond))
	r.ackResult(WorkAssignment{ExecutionID: "exec-1", JobKey: "job", Attempt: 1}, "success", "", 1)
	if got := acks.Load(); got != 2 {
		t.Errorf("acks = %d, want 2", got)
	}
}

// Review 2026-10-24, K-4: a handler still running at the drain timeout is
// hard-cancelled, and its failure ack — retries included — is still sent
// before Run returns.
func TestDrainTimeoutStillAcksWithRetry(t *testing.T) {
	shrinkAckRetryDelays(t)

	var mu sync.Mutex
	var ackBodies []string
	var polls atomic.Int32
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		buf := make([]byte, 4096)
		n, _ := r.Body.Read(buf)
		switch r.URL.Path {
		case "/v1/work/poll":
			if polls.Add(1) == 1 {
				_, _ = w.Write([]byte(`{"work":[{"execution_id":"exec-drain","job_key":"stuck","fire_at":"2026-10-24T10:00:00Z","attempt":1,"metadata":{},"timeout":"5m"}],"cancel":[]}`))
				return
			}
			_, _ = w.Write([]byte(`{"work":[],"cancel":[]}`))
		case "/v1/work/ack":
			mu.Lock()
			ackBodies = append(ackBodies, string(buf[:n]))
			first := len(ackBodies) == 1
			mu.Unlock()
			if first {
				w.WriteHeader(http.StatusServiceUnavailable)
				return
			}
			w.WriteHeader(http.StatusOK)
		default:
			w.WriteHeader(http.StatusOK)
		}
	}))
	defer srv.Close()

	r := NewRunner(srv.URL, "test-runner",
		WithMaxInflight(1),
		WithPollTimeout(500*time.Millisecond),
		WithCapacityBackoff(20*time.Millisecond),
		WithDrainTimeout(100*time.Millisecond),
	)
	started := make(chan struct{})
	r.Register("stuck", func(ctx context.Context, _ *ExecutionContext) error {
		close(started)
		<-ctx.Done()
		return ctx.Err()
	})

	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan struct{})
	go func() {
		_ = r.Run(ctx)
		close(done)
	}()
	select {
	case <-started:
	case <-time.After(2 * time.Second):
		t.Fatal("handler never started")
	}
	cancel()
	select {
	case <-done:
	case <-time.After(3 * time.Second):
		t.Fatal("Run did not return after the drain timeout")
	}

	// Run has returned, so every ack it was going to send has been sent.
	mu.Lock()
	defer mu.Unlock()
	if len(ackBodies) != 2 {
		t.Fatalf("acks = %d, want 2 (503, then the retry)", len(ackBodies))
	}
	for _, b := range ackBodies {
		if !strings.Contains(b, `"execution_id":"exec-drain"`) || !strings.Contains(b, `"status":"failure"`) {
			t.Errorf("ack body = %s, want a failure ack for exec-drain", b)
		}
	}
}

// Review 2026-10-24, K-11: a re-delivered execution_id that is still in
// flight is not dispatched a second time, and its cancel handle survives.
func TestDispatchIgnoresInflightExecutionID(t *testing.T) {
	srv, acks := ackServer(t, 200)
	r := NewRunner(srv.URL, "test-runner")
	release := make(chan struct{})
	var runs atomic.Int32
	r.Register("job", func(ctx context.Context, _ *ExecutionContext) error {
		runs.Add(1)
		select {
		case <-release:
			return nil
		case <-ctx.Done():
			return ctx.Err()
		}
	})

	var wg sync.WaitGroup
	a := WorkAssignment{ExecutionID: "exec-dup", JobKey: "job", Attempt: 1}
	if !r.dispatch(context.Background(), a, &wg) {
		t.Fatal("first dispatch refused")
	}
	if r.dispatch(context.Background(), a, &wg) {
		t.Fatal("second dispatch of an in-flight execution_id was accepted")
	}

	// The surviving cancel handle is the first run's: cancelling through it
	// ends that run.
	r.cancelInflight("exec-dup")
	wg.Wait()
	close(release)

	if got := runs.Load(); got != 1 {
		t.Errorf("handler runs = %d, want 1", got)
	}
	if got := acks.Load(); got != 1 {
		t.Errorf("acks = %d, want 1", got)
	}
	if n := r.inflightCount(); n != 0 {
		t.Errorf("inflight = %d after the run, want 0", n)
	}
}

func TestJitterBounds(t *testing.T) {
	const d = time.Second
	lo, hi := d, time.Duration(0)
	for range 2000 {
		j := jitter(d)
		if j < 800*time.Millisecond || j > 1200*time.Millisecond {
			t.Fatalf("jitter(%v) = %v, outside [0.8, 1.2]×", d, j)
		}
		lo, hi = min(lo, j), max(hi, j)
	}
	// Not a constant: the spread must actually be used.
	if hi-lo < 200*time.Millisecond {
		t.Errorf("jitter spread %v..%v is too narrow", lo, hi)
	}
	if got := jitter(0); got != 0 {
		t.Errorf("jitter(0) = %v, want 0", got)
	}
}

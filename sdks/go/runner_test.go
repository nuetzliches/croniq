package croniq

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

// recordingServer captures every request and serves canned responses
// keyed by path. The Nth call to a given (method, path) pair selects
// the response from the configured queue; further calls fall back to
// the queue's last entry.
type recordingServer struct {
	mu       sync.Mutex
	requests []recordedReq
	srv      *httptest.Server
	respond  map[string][]cannedResp
	hits     map[string]int
}

type recordedReq struct {
	method  string
	path    string
	body    string
	headers http.Header
}

type cannedResp struct {
	status int
	body   string
}

func newRecordingServer() *recordingServer {
	rs := &recordingServer{
		respond: make(map[string][]cannedResp),
		hits:    make(map[string]int),
	}
	rs.srv = httptest.NewServer(http.HandlerFunc(rs.handle))
	return rs
}

func (rs *recordingServer) handle(w http.ResponseWriter, r *http.Request) {
	body, _ := io.ReadAll(r.Body)
	rs.mu.Lock()
	rs.requests = append(rs.requests, recordedReq{
		method:  r.Method,
		path:    r.URL.Path,
		body:    string(body),
		headers: r.Header.Clone(),
	})

	key := r.Method + " " + r.URL.Path
	rs.hits[key]++
	hit := rs.hits[key]
	queue := rs.respond[key]
	rs.mu.Unlock()

	// Use the hit count captured under the lock above — reading
	// rs.hits[key] here (unlocked) races with the rs.hits[key]++ write
	// on a concurrent request, e.g. when a 409 conflict makes the runner
	// retry while an earlier poll is still in flight.
	var resp cannedResp
	switch {
	case len(queue) == 0:
		resp = cannedResp{status: 200, body: "{}"}
	case hit-1 < len(queue):
		resp = queue[hit-1]
	default:
		resp = queue[len(queue)-1]
	}

	w.WriteHeader(resp.status)
	if resp.body != "" {
		_, _ = io.WriteString(w, resp.body)
	}
}

func (rs *recordingServer) reply(method, path string, responses ...cannedResp) {
	rs.respond[method+" "+path] = responses
}

func (rs *recordingServer) count(method, path string) int {
	rs.mu.Lock()
	defer rs.mu.Unlock()
	n := 0
	for _, r := range rs.requests {
		if r.method == method && r.path == path {
			n++
		}
	}
	return n
}

func (rs *recordingServer) firstBody(method, path string) string {
	rs.mu.Lock()
	defer rs.mu.Unlock()
	for _, r := range rs.requests {
		if r.method == method && r.path == path {
			return r.body
		}
	}
	return ""
}

func (rs *recordingServer) close() { rs.srv.Close() }

func TestRunnerDispatchesSuccessfulHandler(t *testing.T) {
	rs := newRecordingServer()
	defer rs.close()

	rs.reply("POST", "/v1/work/poll",
		cannedResp{
			status: 200,
			body: `{"work":[{"execution_id":"exec-1","job_key":"billing:invoice","fire_at":"2026-05-23T10:00:00Z","attempt":1,"metadata":{},"timeout":"1m"}],"cancel":[]}`,
		},
		cannedResp{status: 200, body: `{"work":[],"cancel":[]}`},
	)
	rs.reply("POST", "/v1/work/ack", cannedResp{status: 200, body: "{}"})

	r := NewRunner(rs.srv.URL, "test-runner",
		WithAPIKey("croniq_testkey"),
		WithMaxInflight(1),
		WithPollTimeout(500*time.Millisecond),
		WithPollRetryDelay(100*time.Millisecond),
		WithCapacityBackoff(50*time.Millisecond),
		WithDrainTimeout(2*time.Second),
	)
	var handlerCalled atomic.Bool
	r.Register("billing:invoice", func(ctx context.Context, ec *ExecutionContext) error {
		handlerCalled.Store(true)
		if ec.ExecutionID != "exec-1" || ec.Attempt != 1 {
			t.Errorf("unexpected execution context: %+v", ec)
		}
		return nil
	})

	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()

	done := make(chan struct{})
	go func() {
		_ = r.Run(ctx)
		close(done)
	}()

	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		if rs.count("POST", "/v1/work/ack") >= 1 {
			break
		}
		time.Sleep(20 * time.Millisecond)
	}
	cancel()
	<-done

	if !handlerCalled.Load() {
		t.Error("handler was not invoked")
	}

	ackBody := rs.firstBody("POST", "/v1/work/ack")
	if !strings.Contains(ackBody, `"execution_id":"exec-1"`) {
		t.Errorf("ack body missing execution_id: %s", ackBody)
	}
	if !strings.Contains(ackBody, `"status":"success"`) {
		t.Errorf("ack body status not success: %s", ackBody)
	}
}

func TestRunnerAcksFailureOnHandlerError(t *testing.T) {
	rs := newRecordingServer()
	defer rs.close()

	rs.reply("POST", "/v1/work/poll",
		cannedResp{
			status: 200,
			body: `{"work":[{"execution_id":"exec-2","job_key":"job:fail","fire_at":"2026-05-23T10:00:00Z","attempt":2,"metadata":{},"timeout":"1m"}],"cancel":[]}`,
		},
		cannedResp{status: 200, body: `{"work":[],"cancel":[]}`},
	)
	rs.reply("POST", "/v1/work/ack", cannedResp{status: 200, body: "{}"})

	r := NewRunner(rs.srv.URL, "test-runner",
		WithMaxInflight(1),
		WithPollTimeout(500*time.Millisecond),
		WithPollRetryDelay(100*time.Millisecond),
		WithCapacityBackoff(50*time.Millisecond),
		WithDrainTimeout(2*time.Second),
	)
	r.Register("job:fail", func(ctx context.Context, ec *ExecutionContext) error {
		return &handlerErr{"billing service unreachable"}
	})

	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()

	done := make(chan struct{})
	go func() {
		_ = r.Run(ctx)
		close(done)
	}()

	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		if rs.count("POST", "/v1/work/ack") >= 1 {
			break
		}
		time.Sleep(20 * time.Millisecond)
	}
	cancel()
	<-done

	ackBody := rs.firstBody("POST", "/v1/work/ack")
	var ack AckRequest
	if err := json.Unmarshal([]byte(ackBody), &ack); err != nil {
		t.Fatalf("unmarshal ack: %v", err)
	}
	if ack.Status != "failure" {
		t.Errorf("status = %q, want failure", ack.Status)
	}
	if !strings.Contains(ack.Error, "billing service unreachable") {
		t.Errorf("error not forwarded: %q", ack.Error)
	}
	if ack.Attempt != 2 {
		t.Errorf("attempt = %d, want 2", ack.Attempt)
	}
}

func TestRunnerSurvives409PollAndKeepsPolling(t *testing.T) {
	rs := newRecordingServer()
	defer rs.close()

	rs.reply("POST", "/v1/work/poll",
		cannedResp{status: 409, body: `{"error":"runner instance conflict"}`},
		cannedResp{status: 200, body: `{"work":[],"cancel":[]}`},
	)

	r := NewRunner(rs.srv.URL, "test-runner",
		WithMaxInflight(1),
		WithPollTimeout(300*time.Millisecond),
		WithPollRetryDelay(100*time.Millisecond),
		WithDrainTimeout(500*time.Millisecond),
	)

	ctx, cancel := context.WithTimeout(context.Background(), 1500*time.Millisecond)
	done := make(chan struct{})
	go func() {
		_ = r.Run(ctx)
		close(done)
	}()

	deadline := time.Now().Add(1500 * time.Millisecond)
	for time.Now().Before(deadline) {
		if rs.count("POST", "/v1/work/poll") >= 2 {
			break
		}
		time.Sleep(20 * time.Millisecond)
	}
	cancel()
	<-done

	if got := rs.count("POST", "/v1/work/poll"); got < 2 {
		t.Errorf("expected at least 2 polls (survives 409), got %d", got)
	}
}

func TestRunnerStopsAfterConsecutive409Polls(t *testing.T) {
	// The other half of TestRunnerSurvives409PollAndKeepsPolling: a single
	// 409 is transient and retried, but a streak of them is a duplicate
	// deployment — two processes started with the same fixed runner_id —
	// and retrying that forever hides the misconfiguration behind a
	// warning that scrolls past (issue #134 sub-item 1).
	rs := newRecordingServer()
	defer rs.close()

	// One canned response, no follow-up: the recording server repeats the
	// last reply, so every poll conflicts.
	rs.reply("POST", "/v1/work/poll",
		cannedResp{status: 409, body: `{"error":"runner instance conflict"}`},
	)

	r := NewRunner(rs.srv.URL, "test-runner",
		WithMaxInflight(1),
		WithPollTimeout(300*time.Millisecond),
		WithPollRetryDelay(50*time.Millisecond),
		WithDrainTimeout(500*time.Millisecond),
		WithMaxConsecutivePollConflicts(3),
	)

	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()

	errCh := make(chan error, 1)
	go func() { errCh <- r.Run(ctx) }()

	var err error
	select {
	case err = <-errCh:
	case <-time.After(2 * time.Second):
		t.Fatal("Run did not return — the runner kept polling past the conflict ceiling")
	}

	var conflict *PollInstanceConflictError
	if !errors.As(err, &conflict) {
		t.Fatalf("expected *PollInstanceConflictError, got %v", err)
	}
	if conflict.RunnerID != "test-runner" {
		t.Errorf("expected the error to name the runner_id, got %q", conflict.RunnerID)
	}
	if conflict.ConsecutiveCount != 3 {
		t.Errorf("ConsecutiveCount = %d, want 3", conflict.ConsecutiveCount)
	}
	if !strings.Contains(conflict.Error(), "rotate the runner_id") {
		t.Errorf("expected the error to name the remedy, got %q", conflict.Error())
	}
	if got := rs.count("POST", "/v1/work/poll"); got != 3 {
		t.Errorf("expected exactly 3 polls (the configured ceiling), got %d", got)
	}
}

func TestRunnerSurvives401PollAndKeepsPolling(t *testing.T) {
	// A single 401 must not be fatal. Key rotation hands over by installing
	// the new key and giving the old one an expiry (server issue #471); a
	// runner that died on one 401 would turn a race around that handover
	// into an outage.
	rs := newRecordingServer()
	defer rs.close()

	rs.reply("POST", "/v1/work/poll",
		cannedResp{status: 401, body: `{"error":"unauthorized"}`},
		cannedResp{status: 200, body: `{"work":[],"cancel":[]}`},
	)

	r := NewRunner(rs.srv.URL, "test-runner",
		WithMaxInflight(1),
		WithPollTimeout(300*time.Millisecond),
		WithPollRetryDelay(50*time.Millisecond),
		WithDrainTimeout(500*time.Millisecond),
		WithMaxConsecutiveAuthFailures(3),
	)

	ctx, cancel := context.WithTimeout(context.Background(), 700*time.Millisecond)
	defer cancel()
	if err := r.Run(ctx); err != nil {
		t.Fatalf("a single 401 must be survivable, got %v", err)
	}
	if got := rs.count("POST", "/v1/work/poll"); got < 2 {
		t.Errorf("expected the runner to poll again after the 401, got %d polls", got)
	}
}

func TestRunnerStopsAfterConsecutive401Polls(t *testing.T) {
	// The credential is read once and never re-read, so a rejected key keeps
	// being rejected. Retrying forever left the process up, healthy-looking
	// and idle, and — because it never exited non-zero — never restarted,
	// which is the one thing that would have fixed it (issue #473).
	rs := newRecordingServer()
	defer rs.close()

	rs.reply("POST", "/v1/work/poll",
		cannedResp{status: 401, body: `{"error":"unauthorized"}`},
	)

	r := NewRunner(rs.srv.URL, "test-runner",
		WithMaxInflight(1),
		WithPollTimeout(300*time.Millisecond),
		WithPollRetryDelay(50*time.Millisecond),
		WithDrainTimeout(500*time.Millisecond),
		WithMaxConsecutiveAuthFailures(3),
	)

	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()

	errCh := make(chan error, 1)
	go func() { errCh <- r.Run(ctx) }()

	var err error
	select {
	case err = <-errCh:
	case <-time.After(2 * time.Second):
		t.Fatal("Run did not return — the runner kept polling past the auth ceiling")
	}

	var authErr *AuthFailedError
	if !errors.As(err, &authErr) {
		t.Fatalf("expected *AuthFailedError, got %v", err)
	}
	if authErr.ConsecutiveCount != 3 {
		t.Errorf("ConsecutiveCount = %d, want 3", authErr.ConsecutiveCount)
	}
	if !strings.Contains(authErr.Error(), "Restart the runner") {
		t.Errorf("expected the error to name the remedy, got %q", authErr.Error())
	}
	if got := rs.count("POST", "/v1/work/poll"); got != 3 {
		t.Errorf("expected exactly 3 polls (the configured ceiling), got %d", got)
	}
}

func TestRunnerAuthStreakResetsOnNon401(t *testing.T) {
	// A 500 says nothing about whether the credential is valid. Counting it
	// would make an unwell server look like a revoked key.
	rs := newRecordingServer()
	defer rs.close()

	rs.reply("POST", "/v1/work/poll",
		cannedResp{status: 401, body: `{"error":"unauthorized"}`},
		cannedResp{status: 500, body: `{"error":"boom"}`},
		cannedResp{status: 401, body: `{"error":"unauthorized"}`},
		cannedResp{status: 200, body: `{"work":[],"cancel":[]}`},
	)

	r := NewRunner(rs.srv.URL, "test-runner",
		WithMaxInflight(1),
		WithPollTimeout(300*time.Millisecond),
		WithPollRetryDelay(50*time.Millisecond),
		WithDrainTimeout(500*time.Millisecond),
		WithMaxConsecutiveAuthFailures(2),
	)

	ctx, cancel := context.WithTimeout(context.Background(), 1*time.Second)
	defer cancel()
	// Two 401s with a 500 between them: never two in a row, so the ceiling
	// of 2 must not trip.
	if err := r.Run(ctx); err != nil {
		t.Fatalf("the 500 must reset the auth streak, got %v", err)
	}
}

func TestRunnerConflictStreakResetsOnNon409(t *testing.T) {
	// The streak counts *consecutive* conflicts. A 500 in between is
	// unrelated to instance ownership — a server restart, a proxy hiccup —
	// so it must clear the counter rather than letting an unlucky mix of
	// failures add up to a fatal error.
	rs := newRecordingServer()
	defer rs.close()

	rs.reply("POST", "/v1/work/poll",
		cannedResp{status: 409, body: `{"error":"runner instance conflict"}`},
		cannedResp{status: 500, body: `{"error":"boom"}`},
		cannedResp{status: 409, body: `{"error":"runner instance conflict"}`},
		cannedResp{status: 200, body: `{"work":[],"cancel":[]}`},
	)

	r := NewRunner(rs.srv.URL, "test-runner",
		WithMaxInflight(1),
		WithPollTimeout(300*time.Millisecond),
		WithPollRetryDelay(50*time.Millisecond),
		WithDrainTimeout(500*time.Millisecond),
		WithMaxConsecutivePollConflicts(2),
	)

	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	done := make(chan error, 1)
	go func() { done <- r.Run(ctx) }()

	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		if rs.count("POST", "/v1/work/poll") >= 4 {
			break
		}
		time.Sleep(20 * time.Millisecond)
	}
	cancel()

	if err := <-done; err != nil {
		t.Fatalf("expected the runner to survive 409/500/409 with a ceiling of 2, got %v", err)
	}
	if got := rs.count("POST", "/v1/work/poll"); got < 4 {
		t.Errorf("expected at least 4 polls, got %d", got)
	}
}

func TestRunnerStopsOnPoll403(t *testing.T) {
	// Counterpart to TestRunnerSurvives409PollAndKeepsPolling: a 409 is
	// transient and retried until the conflict ceiling, a 403 is permanent
	// and must stop the runner on the first occurrence (issue #437).
	rs := newRecordingServer()
	defer rs.close()

	rs.reply("POST", "/v1/work/poll",
		cannedResp{status: 403, body: `{"error":"runner_id is bound to a different credential"}`},
	)

	r := NewRunner(rs.srv.URL, "test-runner",
		WithMaxInflight(1),
		WithPollTimeout(300*time.Millisecond),
		WithPollRetryDelay(100*time.Millisecond),
		WithDrainTimeout(500*time.Millisecond),
	)

	ctx, cancel := context.WithTimeout(context.Background(), 1500*time.Millisecond)
	defer cancel()

	errCh := make(chan error, 1)
	go func() { errCh <- r.Run(ctx) }()

	var err error
	select {
	case err = <-errCh:
	case <-time.After(1500 * time.Millisecond):
		t.Fatal("Run did not return — the runner kept polling after a 403")
	}

	var denied *OwnershipDeniedError
	if !errors.As(err, &denied) {
		t.Fatalf("expected *OwnershipDeniedError, got %v", err)
	}
	if denied.RunnerID != "test-runner" {
		t.Errorf("expected the error to name the runner_id, got %q", denied.RunnerID)
	}
	if !strings.Contains(denied.Error(), "DELETE /v1/runners/{id}") {
		t.Errorf("expected the error to name the remedy, got %q", denied.Error())
	}
	if got := rs.count("POST", "/v1/work/poll"); got != 1 {
		t.Errorf("expected exactly 1 poll (403 is fatal), got %d", got)
	}
}

type handlerErr struct{ msg string }

func (e *handlerErr) Error() string { return e.msg }

package croniq

import (
	"context"
	"encoding/json"
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

type handlerErr struct{ msg string }

func (e *handlerErr) Error() string { return e.msg }

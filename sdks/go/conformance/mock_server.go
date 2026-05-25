package conformance

import (
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"time"
)

// RecordedRequest is a snapshot of a single inbound request.
type RecordedRequest struct {
	Method  string
	Path    string
	Headers http.Header
	Body    string
}

// MockServer replays a case's server_script. Each (method, path)
// receives an ordered list of rules; rules with a specific match_count
// match only the Nth hit, others fall through. This mirrors the .NET
// binding's SequentialResponseProvider so cases drive both bindings the
// same way.
type MockServer struct {
	mu       sync.Mutex
	requests []RecordedRequest
	hits     map[string]int
	groups   map[string][]ScriptEntrySpec
	srv      *httptest.Server
}

// StartMockServer builds and starts a server scripted from the given
// case. Caller is responsible for Close.
func StartMockServer(script []ScriptEntrySpec) *MockServer {
	m := &MockServer{
		hits:   make(map[string]int),
		groups: make(map[string][]ScriptEntrySpec),
	}
	for _, e := range script {
		key := e.Method() + " " + e.Path()
		m.groups[key] = append(m.groups[key], e)
	}
	m.srv = httptest.NewServer(http.HandlerFunc(m.handle))
	return m
}

// BaseURL returns the URL the SDK should target.
func (m *MockServer) BaseURL() string { return m.srv.URL }

// Close stops the mock and releases its port.
func (m *MockServer) Close() { m.srv.Close() }

// Recorded returns a snapshot of every request the mock has received,
// in receipt order.
func (m *MockServer) Recorded() []RecordedRequest {
	m.mu.Lock()
	defer m.mu.Unlock()
	out := make([]RecordedRequest, len(m.requests))
	copy(out, m.requests)
	return out
}

func (m *MockServer) handle(w http.ResponseWriter, r *http.Request) {
	body, _ := io.ReadAll(r.Body)
	m.mu.Lock()
	m.requests = append(m.requests, RecordedRequest{
		Method:  r.Method,
		Path:    r.URL.Path,
		Headers: r.Header.Clone(),
		Body:    string(body),
	})
	key := r.Method + " " + r.URL.Path
	m.hits[key]++
	hit := m.hits[key]
	group := m.groups[key]
	m.mu.Unlock()

	entry := pickEntry(group, hit)
	if entry == nil {
		http.Error(w, `{"error":"no rule"}`, http.StatusNotFound)
		return
	}

	if entry.Respond.DelayMs != nil && *entry.Respond.DelayMs > 0 {
		time.Sleep(time.Duration(*entry.Respond.DelayMs) * time.Millisecond)
	}

	for k, v := range entry.Respond.Headers {
		w.Header().Set(k, v)
	}
	if entry.Respond.Body != nil {
		w.Header().Set("Content-Type", "application/json")
	}
	status := entry.Respond.Status
	if status == 0 {
		status = http.StatusOK
	}
	w.WriteHeader(status)

	if entry.Respond.Body != nil {
		buf, err := json.Marshal(entry.Respond.Body)
		if err != nil {
			// A malformed canned body is a case-author bug — surface it
			// loudly rather than silently sending an empty response.
			io.WriteString(w, `{"error":"`+strings.ReplaceAll(err.Error(), `"`, `\"`)+`"}`)
			return
		}
		_, _ = w.Write(buf)
	}
}

// pickEntry resolves "the Nth matching rule" for a given hit count.
// Strategy: prefer the rule whose match_count equals N; otherwise the
// first rule with no match_count (the fallthrough). Mirrors the .NET
// binding's selection order.
func pickEntry(group []ScriptEntrySpec, hit int) *ScriptEntrySpec {
	for i := range group {
		if group[i].MatchCount != nil && *group[i].MatchCount == hit {
			return &group[i]
		}
	}
	for i := range group {
		if group[i].MatchCount == nil {
			return &group[i]
		}
	}
	return nil
}

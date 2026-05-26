package croniq

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestClientPollSendsExpectedRequest(t *testing.T) {
	var receivedAuth, receivedBody string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodPost || r.URL.Path != "/v1/work/poll" {
			t.Errorf("got %s %s", r.Method, r.URL.Path)
		}
		receivedAuth = r.Header.Get("Authorization")
		body, _ := io.ReadAll(r.Body)
		receivedBody = string(body)
		w.Header().Set("Content-Type", "application/json")
		_, _ = io.WriteString(w, `{"work":[],"cancel":[]}`)
	}))
	defer srv.Close()

	c := NewClient(srv.URL).WithAPIKey("croniq_test")
	resp, err := c.Poll(context.Background(), &PollRequest{
		RunnerID:    "r-1",
		MaxInflight: 2,
	})
	if err != nil {
		t.Fatalf("poll failed: %v", err)
	}
	if len(resp.Work) != 0 || len(resp.Cancel) != 0 {
		t.Errorf("expected empty response, got %+v", resp)
	}
	if receivedAuth != "ApiKey croniq_test" {
		t.Errorf("authorization = %q", receivedAuth)
	}
	var parsed PollRequest
	if err := json.Unmarshal([]byte(receivedBody), &parsed); err != nil {
		t.Fatalf("unmarshal body: %v", err)
	}
	if parsed.RunnerID != "r-1" || parsed.MaxInflight != 2 {
		t.Errorf("body decoded incorrectly: %+v", parsed)
	}
}

func TestClientReturnsServerErrorOnNon2xx(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusConflict)
		_, _ = io.WriteString(w, `{"error":"conflict"}`)
	}))
	defer srv.Close()

	c := NewClient(srv.URL)
	_, err := c.Poll(context.Background(), &PollRequest{RunnerID: "r-1"})
	if err == nil {
		t.Fatal("expected error, got nil")
	}
	var se *ServerError
	if !errorAs(err, &se) {
		t.Fatalf("expected *ServerError, got %T", err)
	}
	if se.Status != 409 {
		t.Errorf("status = %d, want 409", se.Status)
	}
}

func TestClientBearerAuth(t *testing.T) {
	var seen string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		seen = r.Header.Get("Authorization")
		_, _ = io.WriteString(w, `{"work":[],"cancel":[]}`)
	}))
	defer srv.Close()

	c := NewClient(srv.URL).WithBearer("eyJ.token.value")
	_, _ = c.Poll(context.Background(), &PollRequest{RunnerID: "r-1"})
	if seen != "Bearer eyJ.token.value" {
		t.Errorf("got %q", seen)
	}
}

func TestClientPushEventsTargetsExecutionPath(t *testing.T) {
	var seenPath string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		seenPath = r.URL.Path
		_, _ = io.WriteString(w, `{}`)
	}))
	defer srv.Close()

	c := NewClient(srv.URL)
	err := c.PushEvents(context.Background(), "exec-abc", []WorkEvent{{Message: "hi"}})
	if err != nil {
		t.Fatalf("push events: %v", err)
	}
	if seenPath != "/v1/work/exec-abc/events" {
		t.Errorf("path = %q", seenPath)
	}
}

func TestClientPushEventsEmptyIsNoop(t *testing.T) {
	called := false
	srv := httptest.NewServer(http.HandlerFunc(func(_ http.ResponseWriter, _ *http.Request) {
		called = true
	}))
	defer srv.Close()

	if err := NewClient(srv.URL).PushEvents(context.Background(), "exec-1", nil); err != nil {
		t.Fatalf("err = %v", err)
	}
	if called {
		t.Error("empty events list should not produce an HTTP call")
	}
}

// errorAs is a tiny wrapper around errors.As to keep test files import-light.
func errorAs(err error, target any) bool {
	type errAs interface{ As(any) bool }
	if e, ok := err.(errAs); ok {
		return e.As(target)
	}
	// Fall back to standard library's reflection-based As, but avoid
	// importing errors in the helper signature.
	return errorsAs(err, target)
}

// errorsAs is a thin shim so we don't have to import "errors" everywhere.
func errorsAs(err error, target any) bool {
	if err == nil {
		return false
	}
	switch t := target.(type) {
	case **ServerError:
		if se, ok := err.(*ServerError); ok {
			*t = se
			return true
		}
	}
	return false
}

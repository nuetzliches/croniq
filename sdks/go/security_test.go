package croniq

import (
	"bytes"
	"context"
	"log/slog"
	"strings"
	"testing"
)

// Base-URL transport-security checks (issue #440). https:// is always
// accepted; http:// only for a loopback host (the http://localhost:4000
// quickstart path) or behind an explicit WithInsecureHTTP opt-in, which
// additionally logs one loud warning.

var acceptedBaseURLs = []string{
	"https://croniq.example.com",
	"https://croniq.example.com:4000",
	"http://localhost:4000",
	"http://LOCALHOST:4000",
	"http://127.0.0.1:4000",
	"http://127.10.20.30:4000",
	"http://[::1]:4000",
}

var rejectedBaseURLs = []string{
	"http://croniq.example.com",
	"http://croniq.example.com:4000",
	"http://10.0.0.5:4000",
	"http://[2001:db8::1]:4000",
}

func TestIsLoopbackHost(t *testing.T) {
	cases := map[string]bool{
		"localhost":       true,
		"LocalHost":       true,
		"127.0.0.1":       true,
		"127.255.255.254": true,
		"::1":             true,
		"[::1]":           true,
		"example.com":     false,
		"10.0.0.5":        false,
		"2001:db8::1":     false,
		"":                false,
	}
	for host, want := range cases {
		if got := IsLoopbackHost(host); got != want {
			t.Errorf("IsLoopbackHost(%q) = %v, want %v", host, got, want)
		}
	}
}

func TestNewClientAcceptsSecureOrLoopbackURL(t *testing.T) {
	for _, u := range acceptedBaseURLs {
		if err := NewClient(u).Err(); err != nil {
			t.Errorf("NewClient(%q).Err() = %v, want nil", u, err)
		}
	}
}

func TestNewClientRejectsNonLoopbackCleartextURL(t *testing.T) {
	for _, u := range rejectedBaseURLs {
		err := NewClient(u).Err()
		if err == nil {
			t.Errorf("NewClient(%q).Err() = nil, want an error", u)
			continue
		}
		// Actionable: names the URL and the opt-in.
		if !strings.Contains(err.Error(), u) {
			t.Errorf("error for %q does not name the URL: %v", u, err)
		}
		if !strings.Contains(err.Error(), "WithInsecureHTTP") {
			t.Errorf("error for %q does not name the opt-in: %v", u, err)
		}
	}
}

func TestNewClientRejectsUnsupportedScheme(t *testing.T) {
	err := NewClient("ftp://croniq.example.com").Err()
	if err == nil || !strings.Contains(err.Error(), "unsupported scheme") {
		t.Errorf("err = %v, want an unsupported-scheme error", err)
	}
}

func TestRefusedClientNeverSendsARequest(t *testing.T) {
	c := NewClient("http://croniq.example.com:4000").WithAPIKey("croniq_secret")
	if err := c.Ack(context.Background(), &AckRequest{RunnerID: "r-1"}); err == nil {
		t.Fatal("expected the configuration error, got nil")
	}
}

func TestWithInsecureHTTPAcceptsCleartextURLAndWarns(t *testing.T) {
	logs := captureSlog(t)

	c := NewClient("http://croniq.example.com:4000").WithInsecureHTTP()
	if err := c.Err(); err != nil {
		t.Fatalf("Err() = %v, want nil after the opt-in", err)
	}

	out := logs.String()
	if !strings.Contains(out, "SECURITY") || !strings.Contains(out, "cleartext") {
		t.Errorf("expected a loud security warning, got: %q", out)
	}
	if !strings.Contains(out, "http://croniq.example.com:4000") {
		t.Errorf("warning does not name the URL: %q", out)
	}
	if n := strings.Count(out, "SECURITY"); n != 1 {
		t.Errorf("expected exactly one warning, got %d: %q", n, out)
	}
}

func TestLoopbackURLDoesNotWarn(t *testing.T) {
	logs := captureSlog(t)
	if err := NewClient("http://localhost:4000").Err(); err != nil {
		t.Fatalf("the quickstart default must keep working, got %v", err)
	}
	if out := logs.String(); out != "" {
		t.Errorf("expected no log output for a loopback URL, got: %q", out)
	}
}

func TestNewRunnerRefusesCleartextURLUntilOptedIn(t *testing.T) {
	r := NewRunner("http://croniq.example.com:4000", "runner-1", WithAPIKey("croniq_secret"))
	err := r.Run(context.Background())
	if err == nil {
		t.Fatal("Run() = nil, want the base-URL configuration error")
	}
	if !strings.Contains(err.Error(), "WithInsecureHTTP") {
		t.Errorf("error does not name the opt-in: %v", err)
	}

	logs := captureSlog(t)
	opted := NewRunner(
		"http://croniq.example.com:4000",
		"runner-1",
		WithAPIKey("croniq_secret"),
		WithInsecureHTTP(),
	)
	if err := opted.Client().Err(); err != nil {
		t.Fatalf("Err() = %v, want nil after WithInsecureHTTP()", err)
	}
	if !strings.Contains(logs.String(), "SECURITY") {
		t.Errorf("expected a loud security warning, got: %q", logs.String())
	}
}

func TestNewTriggerClientRefusesCleartextURLUntilOptedIn(t *testing.T) {
	tc := NewTriggerClient("http://croniq.example.com:4000").WithAPIKey("croniq_secret")
	if _, err := tc.Trigger(context.Background(), &TriggerRequest{JobKey: "billing:invoice"}); err == nil {
		t.Fatal("expected the configuration error, got nil")
	}

	logs := captureSlog(t)
	opted := NewTriggerClient("http://croniq.example.com:4000").WithInsecureHTTP()
	if err := opted.Err(); err != nil {
		t.Fatalf("Err() = %v, want nil after WithInsecureHTTP()", err)
	}
	if !strings.Contains(logs.String(), "SECURITY") {
		t.Errorf("expected a loud security warning, got: %q", logs.String())
	}
}

// captureSlog redirects the default slog logger into a buffer for the
// duration of the test and restores it afterwards.
func captureSlog(t *testing.T) *bytes.Buffer {
	t.Helper()
	buf := &bytes.Buffer{}
	previous := slog.Default()
	slog.SetDefault(slog.New(slog.NewTextHandler(buf, &slog.HandlerOptions{Level: slog.LevelWarn})))
	t.Cleanup(func() { slog.SetDefault(previous) })
	return buf
}

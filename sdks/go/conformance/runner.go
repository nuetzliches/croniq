package conformance

import (
	"context"
	"strings"
	"testing"
	"time"

	croniq "github.com/nuetzliches/croniq/sdks/go"
)

// Run executes a single conformance Spec against a fresh mock server.
// It uses t.Fatalf for setup failures and t.Errorf for assertion
// failures, so each YAML case shows up as a self-contained test.
func Run(t *testing.T, spec *Spec) {
	t.Helper()

	mock := StartMockServer(spec.ServerScript)
	defer mock.Close()

	r := buildRunner(t, spec, mock.BaseURL())
	if err := ApplyHandlers(r, spec.Handlers); err != nil {
		t.Fatalf("apply handlers: %v", err)
	}

	deadline := time.Duration(deref(spec.Expectations.DurationMaxMs, 5000)) * time.Millisecond
	parentCtx, parentCancel := context.WithTimeout(context.Background(), deadline)
	defer parentCancel()

	// Per-runner ctx: lets us stop polling separately from the case-
	// level deadline. The drain still waits up to drain_timeout_ms.
	runnerCtx, runnerCancel := context.WithCancel(parentCtx)

	// Optional binding hook — cancel partway through to exercise drain
	// / shutdown cases.
	if spec.ShutdownAfterMs != nil {
		after := time.Duration(*spec.ShutdownAfterMs) * time.Millisecond
		time.AfterFunc(after, runnerCancel)
	}

	runDone := make(chan struct{})
	go func() {
		_ = r.Run(runnerCtx)
		close(runDone)
	}()

	// Poll the mock's request log until the case's expectations are
	// satisfied. Lets short cases finish early instead of always waiting
	// duration_max_ms.
	start := time.Now()
	for {
		if parentCtx.Err() != nil {
			break
		}
		if expectationsMet(spec.Expectations, mock.Recorded()) {
			break
		}
		select {
		case <-parentCtx.Done():
		case <-time.After(50 * time.Millisecond):
		}
	}

	runnerCancel()
	<-runDone

	assertExpectations(t, spec, mock.Recorded(), time.Since(start))
}

func buildRunner(t *testing.T, spec *Spec, baseURL string) *croniq.Runner {
	t.Helper()
	cfg := spec.RunnerConfig

	runnerID := cfg.RunnerID
	switch {
	case runnerID != "":
		// explicit — use as-is
	case cfg.RunnerIDPrefix != "":
		runnerID = croniq.ResolveRunnerID(cfg.RunnerIDPrefix)
	default:
		runnerID = "test-runner"
	}

	opts := []croniq.Option{
		croniq.WithCapabilities(cfg.Capabilities...),
		croniq.WithTags(cfg.Tags...),
	}
	if cfg.MaxInflight != nil {
		opts = append(opts, croniq.WithMaxInflight(*cfg.MaxInflight))
	}
	if cfg.APIKey != "" {
		opts = append(opts, croniq.WithAPIKey(cfg.APIKey))
	} else if cfg.BearerToken != "" {
		opts = append(opts, croniq.WithBearer(cfg.BearerToken))
	}
	if cfg.PollTimeoutMs != nil {
		opts = append(opts, croniq.WithPollTimeout(time.Duration(*cfg.PollTimeoutMs)*time.Millisecond))
	}
	if cfg.RenewIntervalMs != nil {
		opts = append(opts, croniq.WithRenewInterval(time.Duration(*cfg.RenewIntervalMs)*time.Millisecond))
	}
	if cfg.DrainTimeoutMs != nil {
		opts = append(opts, croniq.WithDrainTimeout(time.Duration(*cfg.DrainTimeoutMs)*time.Millisecond))
	}
	if cfg.PollRetryDelayMs != nil {
		opts = append(opts, croniq.WithPollRetryDelay(time.Duration(*cfg.PollRetryDelayMs)*time.Millisecond))
	}
	if cfg.CapacityBackoffMs != nil {
		opts = append(opts, croniq.WithCapacityBackoff(time.Duration(*cfg.CapacityBackoffMs)*time.Millisecond))
	}

	return croniq.NewRunner(baseURL, runnerID, opts...)
}

// expectationsMet returns true when every expectation can already be
// declared satisfied; max_count caps require waiting the full deadline
// to be observable so we deliberately keep the loop running in that case.
func expectationsMet(exp Expectations, recorded []RecordedRequest) bool {
	for _, e := range exp.HTTP {
		if e.MaxCount != nil {
			return false
		}
		matching := countMatching(e, recorded)
		if e.ExactCount != nil && matching < *e.ExactCount {
			return false
		}
		if e.MinCount != nil && matching < *e.MinCount {
			return false
		}
	}
	return true
}

func assertExpectations(t *testing.T, spec *Spec, recorded []RecordedRequest, _ time.Duration) {
	t.Helper()
	for _, e := range spec.Expectations.HTTP {
		matches := filterMatching(e, recorded)
		count := len(matches)
		if e.ExactCount != nil && count != *e.ExactCount {
			t.Errorf("%s %s: exact_count=%d, got %d", e.Method, e.Path, *e.ExactCount, count)
		}
		if e.MinCount != nil && count < *e.MinCount {
			t.Errorf("%s %s: min_count=%d, got %d", e.Method, e.Path, *e.MinCount, count)
		}
		if e.MaxCount != nil && count > *e.MaxCount {
			t.Errorf("%s %s: max_count=%d, got %d", e.Method, e.Path, *e.MaxCount, count)
		}
		if len(matches) == 0 {
			continue
		}

		// Header assertions apply to the first matching request.
		first := matches[0]
		for name, want := range e.Headers {
			got := first.Headers.Get(name)
			if got == "" {
				t.Errorf("%s %s: missing header %q", e.Method, e.Path, name)
				continue
			}
			if want == "*" {
				continue // any non-empty
			}
			if got != want {
				t.Errorf("%s %s: header %q = %q, want %q", e.Method, e.Path, name, got, want)
			}
		}

		// Body match also applies to the first matching request.
		if e.BodyMatch != nil {
			if msg := MatchBody(e.BodyMatch, first.Body); msg != "" {
				t.Errorf("%s %s: body mismatch — %s\n  actual: %s",
					e.Method, e.Path, msg, first.Body)
			}
		}
	}
}

func countMatching(e HTTPExpectation, recorded []RecordedRequest) int {
	return len(filterMatching(e, recorded))
}

func filterMatching(e HTTPExpectation, recorded []RecordedRequest) []RecordedRequest {
	out := make([]RecordedRequest, 0, len(recorded))
	for _, r := range recorded {
		if !strings.EqualFold(r.Method, e.Method) || r.Path != e.Path {
			continue
		}
		out = append(out, r)
	}
	return out
}

func deref(p *int, fallback int) int {
	if p == nil {
		return fallback
	}
	return *p
}

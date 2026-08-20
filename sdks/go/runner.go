package croniq

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"log/slog"
	"net/http"
	"sync"
	"time"
)

// Default tunables. Match the .NET SDK so the same conformance YAML
// can drive both bindings.
const (
	DefaultMaxInflight     = 5
	DefaultPollTimeout     = 35 * time.Second
	DefaultRenewInterval   = 15 * time.Second
	DefaultDrainTimeout    = 30 * time.Second
	DefaultPollRetryDelay  = 5 * time.Second
	DefaultCapacityBackoff = 500 * time.Millisecond

	// DefaultMaxConsecutivePollConflicts caps how many consecutive
	// 409 Conflict responses the poll loop tolerates before giving up.
	// See [Options.MaxConsecutivePollConflicts].
	DefaultMaxConsecutivePollConflicts = 3

	// DefaultMaxConsecutiveAuthFailures caps how many consecutive 401
	// Unauthorized responses the runner tolerates before giving up.
	// See [Options.MaxConsecutiveAuthFailures].
	DefaultMaxConsecutiveAuthFailures = 3
)

// Options carries all runner-level configuration. Construct via
// [NewRunner] with [Option] functions — direct field access is allowed
// for callers who prefer struct literals.
type Options struct {
	ServerURL    string
	RunnerID     string
	APIKey       string
	BearerToken  string
	Capabilities []string
	Tags         []string
	MaxInflight  int
	InstanceID   string

	PollTimeout     time.Duration
	RenewInterval   time.Duration
	DrainTimeout    time.Duration
	PollRetryDelay  time.Duration
	CapacityBackoff time.Duration

	// MaxConsecutivePollConflicts caps the streak of consecutive
	// 409 Conflict responses from POST /v1/work/poll that the runner
	// tolerates. On exhaustion [Runner.Run] returns a
	// [PollInstanceConflictError] instead of retrying forever, because a
	// sustained 409 means a second process is registered under the same
	// runner_id and no amount of retrying fixes that. The counter resets
	// on a successful poll or on any non-409 failure (5xx, network,
	// timeout), which say nothing about instance ownership. Defaults to
	// [DefaultMaxConsecutivePollConflicts]; set via
	// [WithMaxConsecutivePollConflicts].
	MaxConsecutivePollConflicts int

	// MaxConsecutiveAuthFailures caps the streak of consecutive 401
	// Unauthorized responses tolerated before [Runner.Run] returns an
	// [AuthFailedError]. The API key is read once and never re-read, so a
	// rejected credential cannot fix itself; retrying only produces an
	// idle-looking process that never exits. Reset by any successful poll
	// and by any other error — a 5xx says nothing about whether the key is
	// valid. Defaults to [DefaultMaxConsecutiveAuthFailures]; set via
	// [WithMaxConsecutiveAuthFailures].
	MaxConsecutiveAuthFailures int

	// AllowInsecureHTTP opts in to a cleartext http:// ServerURL on a
	// non-loopback host. Off by default: such a URL is otherwise refused
	// by NewRunner, because the API key would be sent in the clear on
	// every poll. Set via [WithInsecureHTTP].
	AllowInsecureHTTP bool

	// HTTPClient lets callers inject a custom http.Client. If nil, a
	// default is created.
	Client *Client

	// Middleware wraps every handler invocation. Use this for opt-in
	// cross-cutting concerns like tracing. Composes outside-in: the
	// first registered middleware is the outermost wrapper.
	Middleware []Middleware
}

// Option mutates an [Options]. Composed in [NewRunner].
type Option func(*Options)

// Middleware wraps a [HandlerFunc] with cross-cutting behaviour (tracing,
// metrics, recovery, …). Returned function must invoke `next` to run
// the actual handler.
type Middleware func(next HandlerFunc) HandlerFunc

// WithAPIKey configures `Authorization: ApiKey {key}` on every request.
func WithAPIKey(key string) Option { return func(o *Options) { o.APIKey = key } }

// WithBearer configures `Authorization: Bearer {token}` on every request.
func WithBearer(token string) Option { return func(o *Options) { o.BearerToken = token } }

// WithCapabilities sets the runner's capabilities for work routing.
// Capabilities drive `require`/`prefer` placement in the Croniqfile;
// see the docs for the capabilities-vs-tags distinction.
func WithCapabilities(caps ...string) Option {
	return func(o *Options) { o.Capabilities = append([]string(nil), caps...) }
}

// WithTags attaches free-form filter-only labels to this runner. Tags
// do NOT influence routing — they exist for ops visibility (env=prod,
// team=ops, lang=go, …).
func WithTags(tags ...string) Option {
	return func(o *Options) { o.Tags = append([]string(nil), tags...) }
}

// WithMaxInflight caps the number of executions a single runner will
// hold simultaneously. Defaults to [DefaultMaxInflight].
func WithMaxInflight(n int) Option { return func(o *Options) { o.MaxInflight = n } }

// WithPollTimeout sets the deadline applied to each /v1/work/poll call.
func WithPollTimeout(d time.Duration) Option { return func(o *Options) { o.PollTimeout = d } }

// WithRenewInterval sets the cadence at which /v1/work/renew heartbeats
// are sent while a handler is in flight.
func WithRenewInterval(d time.Duration) Option { return func(o *Options) { o.RenewInterval = d } }

// WithDrainTimeout caps how long the runner will wait for in-flight
// handlers to finish naturally after its [context.Context] is cancelled.
// Past this budget remaining handlers are cancelled hard.
func WithDrainTimeout(d time.Duration) Option { return func(o *Options) { o.DrainTimeout = d } }

// WithPollRetryDelay sets the backoff applied after a failed poll (or
// any non-2xx server response).
func WithPollRetryDelay(d time.Duration) Option { return func(o *Options) { o.PollRetryDelay = d } }

// WithCapacityBackoff sets the wait applied when the runner is at
// max_inflight and cannot accept more work.
func WithCapacityBackoff(d time.Duration) Option {
	return func(o *Options) { o.CapacityBackoff = d }
}

// WithMaxConsecutivePollConflicts sets how many consecutive 409 Conflict
// poll responses the runner tolerates before returning a
// [PollInstanceConflictError] from [Runner.Run]. Defaults to
// [DefaultMaxConsecutivePollConflicts].
func WithMaxConsecutivePollConflicts(n int) Option {
	return func(o *Options) { o.MaxConsecutivePollConflicts = n }
}

// WithMaxConsecutiveAuthFailures sets how many consecutive 401 Unauthorized
// responses the runner tolerates before stopping. Defaults to
// [DefaultMaxConsecutiveAuthFailures].
func WithMaxConsecutiveAuthFailures(n int) Option {
	return func(o *Options) { o.MaxConsecutiveAuthFailures = n }
}

// WithHTTPClient lets callers inject a fully-configured *Client (for
// custom transports, proxies, mTLS, recording, …).
func WithHTTPClient(c *Client) Option { return func(o *Options) { o.Client = c } }

// WithInsecureHTTP opts in to a cleartext http:// server URL on a
// non-loopback host. Without it [NewRunner] refuses such a URL and
// [Runner.Run] returns the error before the first poll; with it the runner
// starts but logs one loud warning, because the API key then travels in
// cleartext on every request (and through any HTTP proxy the environment
// configures). Intended for lab and staging setups with no TLS terminator —
// never for production.
func WithInsecureHTTP() Option { return func(o *Options) { o.AllowInsecureHTTP = true } }

// WithMiddleware appends a [Middleware] to the chain.
func WithMiddleware(mw ...Middleware) Option {
	return func(o *Options) { o.Middleware = append(o.Middleware, mw...) }
}

// Runner orchestrates the poll-dispatch-ack loop against the Croniq
// server. Construct via [NewRunner] and call [Runner.Run] with a
// cancellation [context.Context].
type Runner struct {
	opts     Options
	client   *Client
	handlers *handlerRegistry

	schedulesMu sync.Mutex
	schedules   []jobSchedule

	inflightMu sync.Mutex
	inflight   map[string]context.CancelFunc
}

type jobSchedule struct {
	jobKey   string
	schedule string
}

// NewRunner constructs a Runner targeting serverURL on behalf of
// runnerID. The runner has no handlers registered yet — call
// [Runner.Register], [Runner.RegisterWithSchedule], or
// [Runner.SetDefaultHandler] before [Runner.Run].
func NewRunner(serverURL, runnerID string, opts ...Option) *Runner {
	o := Options{
		ServerURL:       serverURL,
		RunnerID:        runnerID,
		MaxInflight:     DefaultMaxInflight,
		PollTimeout:     DefaultPollTimeout,
		RenewInterval:   DefaultRenewInterval,
		DrainTimeout:    DefaultDrainTimeout,
		PollRetryDelay:  DefaultPollRetryDelay,
		CapacityBackoff: DefaultCapacityBackoff,

		MaxConsecutivePollConflicts: DefaultMaxConsecutivePollConflicts,
		MaxConsecutiveAuthFailures:  DefaultMaxConsecutiveAuthFailures,
	}
	for _, opt := range opts {
		opt(&o)
	}
	// A zero value here means "unset" — a struct-literal caller who never
	// heard of the option would otherwise get a runner that bails on its
	// first 409. Negative values are meaningless for the same reason.
	if o.MaxConsecutivePollConflicts <= 0 {
		o.MaxConsecutivePollConflicts = DefaultMaxConsecutivePollConflicts
	}
	if o.MaxConsecutiveAuthFailures <= 0 {
		o.MaxConsecutiveAuthFailures = DefaultMaxConsecutiveAuthFailures
	}

	client := o.Client
	if client == nil {
		client = NewClient(o.ServerURL)
	}
	// Applied after the options are composed, so the base-URL check runs
	// at construction time with the caller's opt-in already known.
	if o.AllowInsecureHTTP {
		client.WithInsecureHTTP()
	}
	switch {
	case o.APIKey != "":
		client.WithAPIKey(o.APIKey)
	case o.BearerToken != "":
		client.WithBearer(o.BearerToken)
	}

	if o.InstanceID == "" {
		o.InstanceID = randomInstanceID()
	}

	return &Runner{
		opts:     o,
		client:   client,
		handlers: newHandlerRegistry(),
		inflight: make(map[string]context.CancelFunc),
	}
}

// Register associates a handler with a job_key. The job must already
// exist on the server (via Croniqfile or /v1/jobs/register).
func (r *Runner) Register(jobKey string, fn HandlerFunc) {
	r.handlers.register(jobKey, fn)
}

// RegisterWithSchedule registers a handler AND its schedule. On
// [Runner.Run] the runner calls POST /v1/jobs/register so the server
// creates the job + trigger if they don't exist. Croniqfile-managed
// jobs take precedence and the schedule is ignored.
func (r *Runner) RegisterWithSchedule(jobKey, schedule string, fn HandlerFunc) {
	r.handlers.register(jobKey, fn)
	r.schedulesMu.Lock()
	r.schedules = append(r.schedules, jobSchedule{jobKey: jobKey, schedule: schedule})
	r.schedulesMu.Unlock()
}

// SetDefaultHandler registers a catch-all invoked when no specific
// handler matches the assignment's job_key.
func (r *Runner) SetDefaultHandler(fn HandlerFunc) {
	r.handlers.setDefault(fn)
}

// RunnerID returns the runner_id this Runner was constructed with.
func (r *Runner) RunnerID() string { return r.opts.RunnerID }

// Client returns the underlying HTTP client. Callers may use it to
// issue ad-hoc API calls (e.g. POST /v1/trigger) without bypassing the
// runner's auth configuration.
func (r *Runner) Client() *Client { return r.client }

// Run starts the poll-dispatch loop. Returns when ctx is cancelled and
// all in-flight handlers have drained (or the drain timeout has elapsed,
// in which case remaining handlers are cancelled hard).
func (r *Runner) Run(ctx context.Context) error {
	// Surface a base-URL that NewRunner refused before any credential
	// reaches the wire (see [WithInsecureHTTP]).
	if err := r.client.Err(); err != nil {
		return err
	}

	slog.InfoContext(ctx, "runner starting",
		"runner_id", r.opts.RunnerID,
		"capabilities", r.opts.Capabilities,
		"max_inflight", r.opts.MaxInflight,
	)

	r.registerSchedules(ctx)

	var wg sync.WaitGroup
	// A fatal poll outcome still drains: in-flight handlers get their
	// grace period before the error reaches the caller.
	err := r.pollLoop(ctx, &wg)
	r.drain(&wg)
	return err
}

// pollLoop runs until ctx is cancelled, or until a poll fails in a way no
// retry can fix. Each iteration: enforce capacity, poll, process
// cancellations, dispatch new assignments.
//
// The returned error is non-nil only for the fatal cases: a 403 ownership
// refusal ([OwnershipDeniedError]) or a streak of 409 conflicts that
// exhausts MaxConsecutivePollConflicts ([PollInstanceConflictError]).
// Every other failure is transient and retried after PollRetryDelay.
func (r *Runner) pollLoop(ctx context.Context, wg *sync.WaitGroup) error {
	// Consecutive 409 Conflict responses on poll. Reset by a successful
	// poll or by any non-409 failure — see MaxConsecutivePollConflicts.
	consecutiveConflicts := 0
	// Consecutive 401s, tracked separately: a run of conflicts must not
	// spend the auth budget, or a duplicate deployment would be reported as
	// an authentication failure.
	consecutiveAuthFailures := 0

	for {
		if ctx.Err() != nil {
			return nil
		}

		// Control-slot polling (issue #176): even at capacity we still
		// poll so the server can deliver cancels via PollResponse.cancel.
		// The server's poll handler returns immediately on capacity=0
		// (no long-poll), so CapacityBackoff paces the loop and prevents
		// a stampede after this at-capacity iteration.
		atCapacity := r.inflightCount() >= r.opts.MaxInflight

		req := &PollRequest{
			RunnerID:     r.opts.RunnerID,
			Capabilities: r.opts.Capabilities,
			MaxInflight:  r.opts.MaxInflight,
			Inflight:     r.inflightIDs(),
			InstanceID:   r.opts.InstanceID,
			Tags:         r.opts.Tags,
		}

		pollCtx, cancel := context.WithTimeout(ctx, r.opts.PollTimeout)
		resp, err := r.client.Poll(pollCtx, req)
		cancel()
		if err != nil {
			if ctx.Err() != nil {
				return nil
			}
			// A 403 is permanent (issue #437): the credential is bound to
			// another runner_id, so the next poll fails identically. Stop
			// with an actionable error instead of retrying on the poll
			// interval, which makes a fenced-out runner look merely idle.
			// Distinct from the 409 arm below, which retries until the
			// streak exhausts MaxConsecutivePollConflicts (see
			// TestRunnerSurvives409PollAndKeepsPolling).
			if isOwnershipDenied(err) {
				slog.ErrorContext(ctx, "fatal: poll refused with 403 Forbidden — this runner's credential does not own runner_id; give the runner its own runner_id, or release the existing binding with DELETE /v1/runners/{id}",
					"runner_id", r.opts.RunnerID,
					"error", err,
				)
				var se *ServerError
				body := ""
				if errors.As(err, &se) {
					body = se.Body
				}
				return &OwnershipDeniedError{
					RunnerID: r.opts.RunnerID,
					Endpoint: "/v1/work/poll",
					Body:     body,
				}
			}
			// A 401 says the key was rejected, and the client never
			// re-reads it, so every later poll presents the same dead
			// credential. Budgeted rather than fatal at once: rotation
			// hands over through an expiry window (server issue #471) and a
			// race around it should not kill a healthy runner (issue #473).
			if isUnauthorized(err) {
				consecutiveAuthFailures++
				if consecutiveAuthFailures >= r.opts.MaxConsecutiveAuthFailures {
					slog.ErrorContext(ctx, "fatal: poll refused with 401 Unauthorized on every attempt — the API key was rejected; it may have been revoked or its rotation grace window elapsed. Restart the runner with the current key",
						"runner_id", r.opts.RunnerID,
						"consecutive", consecutiveAuthFailures,
						"error", err,
					)
					var se *ServerError
					body := ""
					if errors.As(err, &se) {
						body = se.Body
					}
					return &AuthFailedError{
						RunnerID:         r.opts.RunnerID,
						Endpoint:         "/v1/work/poll",
						ConsecutiveCount: consecutiveAuthFailures,
						Body:             body,
					}
				}
				slog.WarnContext(ctx, "poll returned 401 Unauthorized — the API key was rejected; retrying",
					"runner_id", r.opts.RunnerID,
					"consecutive", consecutiveAuthFailures,
					"max_consecutive", r.opts.MaxConsecutiveAuthFailures,
					"retry_after", r.opts.PollRetryDelay,
				)
				select {
				case <-ctx.Done():
					return nil
				case <-time.After(r.opts.PollRetryDelay):
					continue
				}
			}
			// Anything that is not a 401 clears the auth budget: a 5xx or a
			// timeout says nothing about whether the credential is valid.
			consecutiveAuthFailures = 0
			// A 409 means a newer instance has taken this runner_id over
			// (fencing, issue #374). One is transient — the deposed
			// instance may win it back — so we back off and retry. A
			// streak of them is a duplicate deployment, and retrying
			// forever hides it behind a warning that scrolls past
			// (issue #134 sub-item 1).
			if isInstanceConflict(err) {
				consecutiveConflicts++
				if consecutiveConflicts >= r.opts.MaxConsecutivePollConflicts {
					slog.ErrorContext(ctx, "fatal: poll refused with 409 Conflict on every attempt — another runner is registered with this runner_id; stop the duplicate process or rotate the runner_id",
						"runner_id", r.opts.RunnerID,
						"consecutive", consecutiveConflicts,
						"error", err,
					)
					var se *ServerError
					body := ""
					if errors.As(err, &se) {
						body = se.Body
					}
					return &PollInstanceConflictError{
						RunnerID:         r.opts.RunnerID,
						ConsecutiveCount: consecutiveConflicts,
						Body:             body,
					}
				}
				slog.WarnContext(ctx, "poll returned 409 Conflict — another runner instance may be active; retrying",
					"runner_id", r.opts.RunnerID,
					"consecutive", consecutiveConflicts,
					"max_consecutive", r.opts.MaxConsecutivePollConflicts,
					"retry_after", r.opts.PollRetryDelay,
				)
			} else {
				// Non-409 transient (5xx, network, timeout) — unrelated to
				// instance ownership, so a recovered outage must not
				// accumulate with later conflicts.
				consecutiveConflicts = 0
				slog.WarnContext(ctx, "poll failed — backing off",
					"error", err,
					"retry_after", r.opts.PollRetryDelay,
				)
			}
			select {
			case <-ctx.Done():
				return nil
			case <-time.After(r.opts.PollRetryDelay):
				continue
			}
		}

		// Poll succeeded — the other instance must have died or released
		// the identity, so the conflict streak starts over.
		consecutiveConflicts = 0

		// Process server-initiated cancellations before dispatching new
		// work: the cancelled ids may still be in our inflight set and
		// we want their goroutines to start tearing down ASAP.
		for _, id := range resp.Cancel {
			r.cancelInflight(id)
		}

		// At capacity: server returned immediately (work always empty);
		// back off so we don't busy-poll. Cancels above are already
		// processed.
		if atCapacity {
			select {
			case <-ctx.Done():
				return nil
			case <-time.After(r.opts.CapacityBackoff):
				continue
			}
		}

		// Dispatch each assignment in its own goroutine.
		for _, assignment := range resp.Work {
			// Ingest guard: an assignment carrying a control character in
			// either identifier never reaches a handler, a log attribute or a
			// trace attribute. See identifiers.go for the rule and why it is a
			// denylist.
			if field := rejectAssignmentReason(assignment.ExecutionID, assignment.JobKey); field != "" {
				r.rejectAssignment(ctx, assignment, field)
				continue
			}
			wg.Add(1)
			r.dispatch(ctx, assignment, wg)
		}
	}
}

// rejectAssignment handles a work assignment refused by the ingest guard.
//
// The two cases differ in what the runner can still tell the server:
//
//   - Unsafe execution_id — nothing. That value is what addresses an ack or
//     renew, so there is no way to report anything about this execution. The
//     assignment is dropped and the server's lease expires.
//   - Unsafe job_key, valid execution_id — a failure ack. The handler never
//     runs, but the execution completes with an error naming the offending
//     field, so the operator sees a dead-lettered execution instead of one that
//     is silently requeued by the stale-claim reaper and refused again on every
//     later poll.
//
// Called inline rather than in a goroutine: this path only triggers on
// malformed input, so pausing the loop for one small POST costs nothing and
// keeps the ordering observable.
func (r *Runner) rejectAssignment(ctx context.Context, a WorkAssignment, field string) {
	offending := a.JobKey
	ackable := true
	if field == "execution_id" {
		offending = a.ExecutionID
		ackable = false
	}
	// The value is escaped and truncated: this is the one place a refused
	// value is rendered, and it is hostile by definition.
	slog.WarnContext(ctx, "rejected work assignment with unsafe identifier",
		"field", field,
		"value", previewForLog(offending),
		"acked", ackable,
	)
	if !ackable {
		return
	}
	r.ackResult(a, "failure", rejectionAckError(field, offending), 0)
}

// dispatch spawns a goroutine that runs the handler for one assignment,
// drives lease renewal, drains any LogWriter, and acks the result.
func (r *Runner) dispatch(_ context.Context, a WorkAssignment, wg *sync.WaitGroup) {
	// Each execution gets its own context, independent of the runner's
	// parent ctx — that way "stop polling on shutdown" does not also
	// "instantly cancel every in-flight handler." The drain step
	// cancels these only after drainTimeout, if at all.
	execCtx, execCancel := context.WithCancel(context.Background())

	r.inflightMu.Lock()
	r.inflight[a.ExecutionID] = execCancel
	r.inflightMu.Unlock()

	ec := &ExecutionContext{
		ExecutionID:  a.ExecutionID,
		JobKey:       a.JobKey,
		ScheduledFor: parseScheduledFor(a.ScheduledFor),
		Attempt:      a.Attempt,
		Metadata:     a.Metadata,
		Timeout:      a.Timeout,
		RunnerID:     r.opts.RunnerID,
		RunnerTags:   append([]string(nil), r.opts.Tags...),
		client:       r.client,
	}

	go func() {
		defer wg.Done()
		defer execCancel()

		handler, ok := r.handlers.get(a.JobKey)
		if !ok {
			r.ackResult(a, "failure", fmt.Sprintf("no handler registered for %s", a.JobKey), 0)
			r.removeInflight(a.ExecutionID)
			return
		}

		// Compose middleware chain outside-in.
		wrapped := handler
		for i := len(r.opts.Middleware) - 1; i >= 0; i-- {
			wrapped = r.opts.Middleware[i](wrapped)
		}

		// Lease renewal: tick at RenewInterval until the handler returns.
		renewStop := make(chan struct{})
		go r.renewLoop(a.ExecutionID, renewStop)

		start := time.Now()
		err := safeRun(execCtx, wrapped, ec)
		close(renewStop)
		duration := time.Since(start).Milliseconds()

		// Drain the streaming log writer (if any) before the ack so
		// queued events land server-side before the execution is marked
		// complete. Tracked separately from the inflight map so we can
		// drain even if the handler returned via cancellation.
		if ec.logWriter != nil {
			ec.logWriter.shutdownAndDrain()
		}

		status := "success"
		var errStr string
		if err != nil {
			status = "failure"
			errStr = err.Error()
		}

		r.ackResult(a, status, errStr, duration)
		r.removeInflight(a.ExecutionID)
	}()
}

// renewLoop POSTs /v1/work/renew on the configured cadence until the
// handler's done signal closes.
func (r *Runner) renewLoop(executionID string, done <-chan struct{}) {
	ticker := time.NewTicker(r.opts.RenewInterval)
	defer ticker.Stop()
	for {
		select {
		case <-done:
			return
		case <-ticker.C:
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			err := r.client.Renew(ctx, &RenewRequest{
				RunnerID:    r.opts.RunnerID,
				ExecutionID: executionID,
			})
			cancel()
			switch {
			case err == nil:
			case isOwnershipDenied(err):
				// Permanent (#436/#437): every later renew fails the same
				// way and the lease will expire mid-handler.
				slog.Error("lease renew refused with 403 Forbidden — this runner's credential does not own runner_id, so the lease will expire and the execution be reclaimed; give the runner its own runner_id, or release the existing binding with DELETE /v1/runners/{id}",
					"runner_id", r.opts.RunnerID,
					"execution_id", executionID,
					"error", err,
				)
			case serverStatus(err) == http.StatusNotFound || serverStatus(err) == http.StatusConflict:
				// Since #447 renew is a real per-execution lease: 404 (no
				// longer leased here) and 409 (already terminal) are the
				// normal outcome of a renew racing our own completion.
				slog.Debug("lease renew raced execution completion",
					"execution_id", executionID,
					"status", serverStatus(err),
				)
			default:
				slog.Warn("renew failed", "execution_id", executionID, "error", err)
			}
		}
	}
}

// ackResult posts the final ack. Uses a short, decoupled context so the
// ack still goes through even if the runner's ctx was cancelled mid-drain.
func (r *Runner) ackResult(a WorkAssignment, status, errStr string, durationMs int64) {
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	err := r.client.Ack(ctx, &AckRequest{
		RunnerID:    r.opts.RunnerID,
		ExecutionID: a.ExecutionID,
		Status:      status,
		Error:       errStr,
		DurationMs:  durationMs,
		Attempt:     a.Attempt,
	})
	if err != nil {
		if isOwnershipDenied(err) {
			slog.Error("ack refused with 403 Forbidden — this runner's credential does not own runner_id, so the execution stays claimed until its lease expires; give the runner its own runner_id, or release the existing binding with DELETE /v1/runners/{id}",
				"runner_id", r.opts.RunnerID,
				"execution_id", a.ExecutionID,
				"error", err,
			)
			return
		}
		slog.Error("failed to ack execution",
			"execution_id", a.ExecutionID,
			"status", status,
			"error", err,
		)
	}
}

// registerSchedules POSTs each registered (job_key, schedule) pair to
// /v1/jobs/register at startup. Failures are logged but not fatal —
// the runner can still poll for work even if registration fails.
func (r *Runner) registerSchedules(ctx context.Context) {
	r.schedulesMu.Lock()
	schedules := append([]jobSchedule(nil), r.schedules...)
	r.schedulesMu.Unlock()

	for _, s := range schedules {
		slog.InfoContext(ctx, "registering job on server",
			"job_key", s.jobKey,
			"schedule", s.schedule,
		)
		reqCtx, cancel := context.WithTimeout(ctx, 10*time.Second)
		err := r.client.RegisterJob(reqCtx, &RegisterJobRequest{
			JobKey:       s.jobKey,
			Schedule:     s.schedule,
			RunnerID:     r.opts.RunnerID,
			Capabilities: r.opts.Capabilities,
		})
		cancel()
		if err != nil {
			slog.WarnContext(ctx, "failed to register job — will still poll",
				"job_key", s.jobKey,
				"error", err,
			)
			continue
		}
		slog.InfoContext(ctx, "job registered", "job_key", s.jobKey)
	}
}

// drain waits for in-flight handlers to finish naturally, up to
// drainTimeout. Past the timeout, remaining handlers are cancelled hard
// via the execCtx and the wait completes anyway.
func (r *Runner) drain(wg *sync.WaitGroup) {
	if r.inflightCount() == 0 {
		// Fast path — no inflight, nothing to drain.
		wg.Wait()
		return
	}

	slog.Info("draining in-flight handlers", "count", r.inflightCount(), "timeout", r.opts.DrainTimeout)

	done := make(chan struct{})
	go func() {
		wg.Wait()
		close(done)
	}()

	select {
	case <-done:
		slog.Info("drain complete — all inflight handlers finished")
	case <-time.After(r.opts.DrainTimeout):
		slog.Warn("drain timeout — cancelling remaining handlers", "remaining", r.inflightCount())
		r.cancelAllInflight()
		<-done
	}
}

func (r *Runner) inflightCount() int {
	r.inflightMu.Lock()
	defer r.inflightMu.Unlock()
	return len(r.inflight)
}

func (r *Runner) inflightIDs() []string {
	r.inflightMu.Lock()
	defer r.inflightMu.Unlock()
	ids := make([]string, 0, len(r.inflight))
	for id := range r.inflight {
		ids = append(ids, id)
	}
	return ids
}

func (r *Runner) cancelInflight(executionID string) {
	r.inflightMu.Lock()
	cancel, ok := r.inflight[executionID]
	r.inflightMu.Unlock()
	if ok && cancel != nil {
		cancel()
	}
}

func (r *Runner) cancelAllInflight() {
	r.inflightMu.Lock()
	cancels := make([]context.CancelFunc, 0, len(r.inflight))
	for _, c := range r.inflight {
		cancels = append(cancels, c)
	}
	r.inflightMu.Unlock()
	for _, c := range cancels {
		c()
	}
}

func (r *Runner) removeInflight(executionID string) {
	r.inflightMu.Lock()
	delete(r.inflight, executionID)
	r.inflightMu.Unlock()
}

// safeRun protects the poll loop from a panicking handler — surfacing
// the panic as a handler error keeps the runner alive and lets the
// server's retry policy decide what to do.
func safeRun(ctx context.Context, fn HandlerFunc, ec *ExecutionContext) (err error) {
	defer func() {
		if rec := recover(); rec != nil {
			err = fmt.Errorf("handler panicked: %v", rec)
		}
	}()
	err = fn(ctx, ec)
	if err == nil && ctx.Err() != nil {
		// Handler returned nil while its context was cancelled — treat
		// the cancellation as the cause so the server sees a failure
		// for server-requested cancels (case 04).
		err = errors.New("cancelled")
	}
	return err
}

func randomInstanceID() string {
	var buf [8]byte
	if _, err := rand.Read(buf[:]); err != nil {
		return fmt.Sprintf("inst-%d", time.Now().UnixNano())
	}
	return "inst-" + hex.EncodeToString(buf[:])
}

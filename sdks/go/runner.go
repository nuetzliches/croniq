package croniq

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"log/slog"
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

// WithHTTPClient lets callers inject a fully-configured *Client (for
// custom transports, proxies, mTLS, recording, …).
func WithHTTPClient(c *Client) Option { return func(o *Options) { o.Client = c } }

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
	}
	for _, opt := range opts {
		opt(&o)
	}

	client := o.Client
	if client == nil {
		client = NewClient(o.ServerURL)
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
	slog.InfoContext(ctx, "runner starting",
		"runner_id", r.opts.RunnerID,
		"capabilities", r.opts.Capabilities,
		"max_inflight", r.opts.MaxInflight,
	)

	r.registerSchedules(ctx)

	var wg sync.WaitGroup
	r.pollLoop(ctx, &wg)
	r.drain(&wg)
	return nil
}

// pollLoop runs until ctx is cancelled. Each iteration: enforce
// capacity, poll, process cancellations, dispatch new assignments.
func (r *Runner) pollLoop(ctx context.Context, wg *sync.WaitGroup) {
	for {
		if ctx.Err() != nil {
			return
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
				return
			}
			slog.WarnContext(ctx, "poll failed — backing off",
				"error", err,
				"retry_after", r.opts.PollRetryDelay,
			)
			select {
			case <-ctx.Done():
				return
			case <-time.After(r.opts.PollRetryDelay):
				continue
			}
		}

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
				return
			case <-time.After(r.opts.CapacityBackoff):
				continue
			}
		}

		// Dispatch each assignment in its own goroutine.
		for _, assignment := range resp.Work {
			wg.Add(1)
			r.dispatch(ctx, assignment, wg)
		}
	}
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
			if err != nil {
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

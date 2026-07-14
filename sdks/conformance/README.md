# Croniq Runner Conformance Suite

A language-agnostic test suite that asserts the **wire-level behaviour** of a
Croniq runner — independent of the SDK language. Each test case is a YAML file
that:

1. Scripts a mock Croniq server (canned responses for `/v1/work/poll`,
   `/v1/work/ack`, `/v1/work/renew`, `/v1/work/{id}/events`,
   `/v1/jobs/register`).
2. Declares the runner configuration (capabilities, tags, timeouts, etc.).
3. Names the handler "sentinels" the binding must implement
   (`noop`, `throw`, `sleep`, `log`, `stream_logs`).
4. Asserts what HTTP requests the runner produced — method, path, body,
   ordering, count.

A **binding** (a thin per-language adapter) loads the YAML, stands up a real
HTTP mock server, configures the SDK against the mock URL, and verifies the
expectations. The same YAML drives every SDK — that's the point.

## Why

- **Single source of truth for the wire protocol.** Cases are checked into the
  repo next to [`openapi.yaml`](../../openapi.yaml). Schema changes that drift
  from the spec break the suite for every SDK at once.
- **Bootstrapping new SDKs is cheap.** A Python or Go author starts with a
  bag of "definition-of-done" cases instead of guessing what to test.
- **Regression-safe server changes.** The server team can run the suite
  against every SDK before changing `/v1/*` semantics.
- **Lives where the protocol does.** Cases sit beside the OpenAPI spec, not
  buried in a single SDK's test folder.

## Directory layout

```
sdks/conformance/
├── README.md                       this file
├── schema/
│   ├── case-schema.json            JSON-Schema for runner (consumer) cases
│   └── trigger-case-schema.json    JSON-Schema for trigger (producer) cases
├── cases/                          runner (consumer) loop — poll/ack/renew/logs
│   ├── 01-poll-empty.yaml
│   ├── 02-poll-single-success.yaml
│   └── …
└── cases-trigger/                  trigger (producer) — POST /v1/trigger
    ├── 01-trigger-minimal.yaml
    ├── 02-trigger-full-request.yaml
    └── …
```

Runner cases and trigger cases live in **separate directories with separate
schemas** on purpose. A runner binding enumerates `cases/` and drives a poll
loop from each file; a producer-shaped case dropped into `cases/` would break
that loop. Keeping producer cases in `cases-trigger/` lets each SDK add a
trigger runner independently (issues #282–#286) without disturbing bindings
that only implement the consumer side.

Bindings live with their SDK (each SDK author knows its own ecosystem
best — central bindings would force a polyglot test runner). Current
bindings:

- .NET: [`sdks/dotnet/tests/Croniq.Runner.Sdk.Conformance.Tests`](../dotnet/tests/Croniq.Runner.Sdk.Conformance.Tests)
- Python: [`sdks/python/tests/conformance`](../python/tests/conformance)
- Go: [`sdks/go/conformance`](../go/conformance)
- Java: [`sdks/java/conformance-tests`](../java/conformance-tests) (skeleton — full harness lands in PR-2 of [#133](https://github.com/nuetzliches/croniq/issues/133))

## Case anatomy

```yaml
name: "Short imperative title"
description: |
  Plain prose. Explains why the case exists and what the runner must do.

runner_config:                         # → maps to CroniqRunnerOptions (or equivalent)
  capabilities: ["billing"]
  tags: ["env=test"]
  max_inflight: 1
  api_key: "croniq_testkey"
  poll_timeout_ms: 5000
  renew_interval_ms: 1000
  drain_timeout_ms: 5000

handlers:                              # bindings translate behaviours to handlers
  - job_key: "billing:invoice"
    behavior: noop                     # one of: noop | throw | sleep | log | stream_logs
    # behavior-specific extra fields (see schema)

server_script:                         # sequential HTTP rules; first match wins
  - on: "POST /v1/work/poll"
    match_count: 1                     # only the Nth time this rule matches
    respond:
      status: 200
      body:                            # snake_case, matches openapi.yaml
        work:
          - execution_id: "exec-001"
            job_key: "billing:invoice"
            fire_at: "2026-05-23T10:00:00Z"
            attempt: 1
            metadata: {}
            timeout: "1m"
        cancel: []
  - on: "POST /v1/work/poll"           # fallthrough rule for all later polls
    respond:
      status: 200
      body: { work: [], cancel: [] }
  - on: "POST /v1/work/ack"
    respond: { status: 200, body: {} }

expectations:
  duration_max_ms: 3000                # cap on total wall-clock for the case
  http:
    - method: POST
      path: /v1/work/poll
      min_count: 1
      headers:
        authorization: "ApiKey croniq_testkey"
    - method: POST
      path: /v1/work/ack
      exact_count: 1
      body_match:                      # subset match — only listed keys are checked
        runner_id: "*"                 # "*" → any non-empty
        execution_id: "exec-001"
        status: "success"
        attempt: 1
```

### Field reference

See [`schema/case-schema.json`](schema/case-schema.json) for the full,
authoritative shape including types and required fields. The schema is used
in CI to validate every YAML in `cases/`.

### Handler sentinels

| behavior      | extra fields                          | what the binding does                                         |
| ------------- | ------------------------------------- | ------------------------------------------------------------- |
| `noop`        | —                                     | handler returns immediately with success                      |
| `throw`       | `error_message: string`               | handler throws → SDK acks `status=failure, error=<message>`   |
| `sleep`       | `duration_ms: int`                    | handler awaits the cancellation token for the given duration  |
| `log`         | `level`, `message`, `count?`          | handler emits `count` log lines (default 1) via the SDK logger |
| `stream_logs` | `count`, `interval_ms`, `level?`      | handler streams events via the SDK's `LogWriter`              |

### Body matching

Subset-match with one wildcard symbol:

- A literal value (string, number, bool) must match exactly.
- `"*"` matches any non-empty value of the same JSON kind.
- Nested objects are matched recursively against the listed keys; extra keys
  in the actual body are ignored.
- Use `null` to assert the key is present and explicitly `null`.

This keeps cases readable. Cases that need JSONPath-style expressiveness
should propose an extension before reaching for ad-hoc string matching.

## Trigger (producer) cases

Cases in `cases-trigger/` pin the **producer** side — a trigger client
wrapping `POST /v1/trigger` (issues #277, #282–#286). They validate against
[`schema/trigger-case-schema.json`](schema/trigger-case-schema.json). The
shape mirrors runner cases (`server_script` + `expectations.http` are
identical) but swaps the poll loop for explicit calls:

```yaml
name: "Short imperative title"
description: |
  Why the case exists and what the trigger client must do.

trigger_config:                        # → trigger client options (server_url injected by the binding)
  api_key: "croniq_trigger_key"        # the producer's OWN credential (jobs:trigger scope), not a runner's
  # bearer_token: "…"                  # alternative auth

trigger_calls:                         # ordered trigger(...) invocations the binding must make
  - request:                           # arguments passed to the client
      job_key: "billing:invoice"
      metadata: { tenant: "acme" }     # optional; omitted fields must NOT appear on the wire
      require: ["gpu"]
      prefer: ["eu-west"]
      timeout: "15m"
      idempotency_key: "evt-001"
    expect:                            # what the client must surface to the caller
      response:                        # success — subset match on the parsed TriggerResponse
        execution_id: "exec-1"         # "*" allowed for any non-empty
        queued: 3
        deduplicated: false
      # error: true                    # …OR the call must raise/return an error (mutually exclusive with response)

server_script:                         # canned /v1/trigger responses (match_count sequences multi-call cases)
  - on: "POST /v1/trigger"
    respond: { status: 200, body: { execution_id: "exec-1", queued: 3 } }

expectations:
  duration_max_ms: 2000
  http:
    - method: POST
      path: /v1/trigger
      exact_count: 1
      headers: { authorization: "ApiKey croniq_trigger_key" }
      body_match: { job_key: "billing:invoice" }   # subset match, same semantics as runner cases
      body_absent: ["timeout", "idempotency_key"]   # keys that MUST be absent — pins omission of unset optionals
```

Notes for binding authors:

- **`trigger_calls` are per-call.** Make each call in order; assert its
  `expect` (a returned value on `response`, an error on `error: true`).
  Multi-call cases fire `/v1/trigger` more than once — the mock sequences
  responses by `match_count` exactly as for runner cases.
- **`expect.response` is a subset match** on the parsed response object. A
  missing `deduplicated` field on the wire parses as `false`.
- **`body_absent`** lists top-level request-body keys that must NOT be
  present. It exists because the subset `body_match` can only assert keys that
  *are* present; a producer must not emit optionals the caller never supplied.
  Asserted against the first matching request.
- **No runner is configured** in a trigger case, so the credential in
  `trigger_config` can only be the trigger client's own — that is how the
  suite pins "the producer uses its own credentials, not a runner's".

## Writing a new binding

1. Pick the SDK's natural test-runner home (xUnit / pytest / `go test`).
2. Implement a YAML loader that produces an in-memory case representation.
3. Stand up a real HTTP mock server (WireMock.Net, `pytest-httpserver`,
   `httptest.Server` …). Replay `server_script` rules.
4. Map `runner_config` to the SDK's options object; point `server_url` at the
   mock's base URL.
5. Implement the handler sentinels using the SDK's handler-registration API.
6. Invoke the runner (one shot per case). Cap wall-clock with
   `expectations.duration_max_ms`.
7. After the runner stops, assert `expectations.http` against the mock's
   recorded requests.

Bindings should publish a one-to-one mapping: **one test per YAML case**, so
test-explorer output names the failing case directly.

## Versioning

The suite shares the repo and is versioned implicitly with the server (each
commit tests against the spec at that commit). When the wire protocol changes
in a backward-incompatible way, add a new case rather than editing an
existing one — old cases document old behaviour for SDKs that still target
those server versions.

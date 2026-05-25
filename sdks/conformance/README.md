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
│   └── case-schema.json            JSON-Schema for validation in CI
└── cases/
    ├── 01-poll-empty.yaml
    ├── 02-poll-single-success.yaml
    └── …
```

Bindings live with their SDK (each SDK author knows its own ecosystem
best — central bindings would force a polyglot test runner). Current bindings:

- .NET: [`sdks/dotnet/tests/Croniq.Runner.Sdk.Conformance.Tests`](../dotnet/tests/Croniq.Runner.Sdk.Conformance.Tests)
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

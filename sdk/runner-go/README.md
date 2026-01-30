# Croniq Runner SDK (Go)

Runner SDK with gRPC streaming (primary) and HTTP polling fallback. Use `Runner` for transport chaining, or `Client` for direct HTTP calls.

## Requirements

- Go 1.22+

## Usage

See the sample in [samples/runners/go/polling-basic](../../samples/runners/go/polling-basic).

Register handlers per job key:

```go
runner.OnExecuteWithRegistration("demo-job", func(ctx croniqrunner.ExecutionContext, payload *string, logger croniqrunner.RunnerLogger) error {
    return nil
}, &croniqrunner.RunnerJobRegistration{Description: "Demo job registered by the runner."})
```

## Configuration

Use `LoadRunnerConfigFromEnv()` to build a `RunnerConfig` from environment variables:

- Required: `CRONIQ_API_BASEURL`, `CRONIQ_TENANT_ID`, `CRONIQ_ENVIRONMENT`, `CRONIQ_RUNNER_ID`, and exactly one of `CRONIQ_API_KEY` or `CRONIQ_BEARER_TOKEN`.
- Optional transport: `CRONIQ_GRPC_BASEURL`, `CRONIQ_TRANSPORT_MODE` (`auto|grpc|polling`), `CRONIQ_ALLOW_TEST_EXECUTIONS`.
- Optional tuning: `CRONIQ_POLL_BATCH_SIZE`, `CRONIQ_POLL_WAIT_MS`, `CRONIQ_REQUEST_TIMEOUT_MS`, `CRONIQ_RENEW_LEAD_MS`,
  `CRONIQ_RETRY_BASE_MS`, `CRONIQ_RETRY_MAX_MS`, `CRONIQ_RETRY_MAX_ATTEMPTS`, `CRONIQ_MAX_INFLIGHT`, `CRONIQ_CAPABILITIES`,
  `CRONIQ_RUNNER_REGISTER_JOBS`.

If you want the SDK to fall back to a runner-specific API key and default runner ids, use
`LoadRunnerConfigFromEnvWithDefaults`:

```go
config, err := croniqrunner.LoadRunnerConfigFromEnvWithDefaults(croniqrunner.RunnerEnvDefaults{
    RunnerApiKeyEnv:            "CRONIQ_RUNNER_GO_API_KEY",
    DefaultRunnerId:            "default",
    RunnerApiKeyDefaultRunnerId: "go-default",
})
```

## Notes

- This client currently polls `/work/poll`, sends events, and acks leases.
- For long-running work, call `Renew` periodically.
- `CRONIQ_RUNNER_ID` must match the API client id associated with the API key.

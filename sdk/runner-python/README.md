# Croniq Runner SDK (Python)

Runner SDK with gRPC streaming (primary) and HTTP polling fallback. Use `CroniqRunner` for transport chaining, or `RunnerClient` for direct HTTP calls.

## Requirements

- Python 3.11+

## Usage

Import from `croniq_runner`. See the sample in [samples/runners/python/basic](../../samples/runners/python/basic).

Register handlers per job key:

```python
runner.on_execute(
    "demo-job",
    handle_execution,
    RunnerJobRegistration(description="Demo job registered by the runner."),
)
```

## Configuration

Build a `RunnerConfig` from environment variables with `RunnerConfig.from_env()`:

- Required: `CRONIQ_API_BASEURL`, `CRONIQ_TENANT_ID`, `CRONIQ_ENVIRONMENT`, `CRONIQ_RUNNER_ID`, and exactly one of `CRONIQ_API_KEY` or `CRONIQ_BEARER_TOKEN`.
- Optional transport: `CRONIQ_GRPC_BASEURL`, `CRONIQ_TRANSPORT_MODE` (`auto|grpc|polling`), `CRONIQ_ALLOW_TEST_EXECUTIONS`.
- Optional tuning: `CRONIQ_POLL_BATCH_SIZE`, `CRONIQ_POLL_WAIT_MS`, `CRONIQ_REQUEST_TIMEOUT_MS`, `CRONIQ_RENEW_LEAD_MS`,
  `CRONIQ_RETRY_BASE_MS`, `CRONIQ_RETRY_MAX_MS`, `CRONIQ_RETRY_MAX_ATTEMPTS`, `CRONIQ_MAX_INFLIGHT`, `CRONIQ_CAPABILITIES`,
  `CRONIQ_RUNNER_REGISTER_JOBS`.

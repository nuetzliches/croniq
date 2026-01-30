# Croniq Runner SDK (Node)

Runner SDK with gRPC streaming (primary) and HTTP polling fallback. Use `CroniqRunner` for transport chaining, or `RunnerClient` for direct HTTP calls.

## Requirements

- Node.js LTS

## Usage

See the sample in [samples/runners/node/basic](../../samples/runners/node/basic). Register handlers per job key:

```ts
runner.onExecute(
  "demo-job",
  async (context, payload, logger) => {
    logger.info("execution started", { executionId: context.executionId });
    // ...
  },
  { description: "Demo job registered by the runner." }
);
```

## Configuration

You can build a `RunnerConfig` from environment variables:

- Required: `CRONIQ_API_BASEURL`, `CRONIQ_TENANT_ID`, `CRONIQ_ENVIRONMENT`, `CRONIQ_RUNNER_ID`, and exactly one of `CRONIQ_API_KEY` or `CRONIQ_BEARER_TOKEN`.
- Optional transport: `CRONIQ_GRPC_BASEURL`, `CRONIQ_TRANSPORT_MODE` (`auto|grpc|polling`), `CRONIQ_ALLOW_TEST_EXECUTIONS`.
- Optional tuning: `CRONIQ_POLL_BATCH_SIZE`, `CRONIQ_POLL_WAIT_MS`, `CRONIQ_REQUEST_TIMEOUT_MS`, `CRONIQ_RENEW_LEAD_MS`,
  `CRONIQ_RETRY_BASE_MS`, `CRONIQ_RETRY_MAX_MS`, `CRONIQ_RETRY_MAX_ATTEMPTS`, `CRONIQ_MAX_INFLIGHT`, `CRONIQ_CAPABILITIES`,
  `CRONIQ_RUNNER_REGISTER_JOBS`.

Use `loadRunnerConfigFromEnv()` to validate and parse these values. You can optionally pass
defaults for runner-specific API keys and runner ids:

```ts
const config = loadRunnerConfigFromEnv(process.env, {
  runnerApiKeyEnv: "CRONIQ_RUNNER_NODE_API_KEY",
  defaultRunnerId: "default",
  runnerApiKeyDefaultRunnerId: "node-default",
});
```

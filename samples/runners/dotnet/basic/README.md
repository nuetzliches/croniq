# .NET Runner Sample (gRPC)

Minimal gRPC runner using the Croniq .NET client.

## Requirements

- .NET SDK 10.0

## Run

```bash
cd samples/runners/dotnet/basic
CRONIQ_API_BASEURL=http://localhost:5080 \
CRONIQ_TENANT_ID=default \
CRONIQ_ENVIRONMENT=dev \
CRONIQ_API_KEY=smoke-key \
CRONIQ_RUNNER_ID=default \
dotnet run
```

## Notes

- The example connects to `Runner.Connect`, sends a work event, and acks each lease.
- For long-running jobs, renew leases and stream events while work is running.

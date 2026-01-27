# .NET Runner Sample

Minimal runner using the Croniq .NET runner SDK (gRPC-first with polling fallback).

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
CRONIQ_JOB_KEY=demo-job \
dotnet run
```

## Notes

- The SDK handles gRPC streaming, polling fallback, heartbeats, renewals, and acks.
- Use `CRONIQ_JOB_KEY` to select which job handler to register in the sample.

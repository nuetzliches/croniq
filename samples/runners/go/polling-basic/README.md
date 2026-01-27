# Go Runner Sample

Minimal runner using the Go SDK (gRPC-first with polling fallback).

## Requirements

- Go 1.22+

## Run

```bash
cd samples/runners/go/polling-basic
CRONIQ_API_BASEURL=http://localhost:5080 \
CRONIQ_TENANT_ID=default \
CRONIQ_ENVIRONMENT=dev \
CRONIQ_API_KEY=smoke-key \
CRONIQ_RUNNER_ID=default \
CRONIQ_JOB_KEY=demo-job \
go run .
```

## Notes

- The SDK handles gRPC streaming, polling fallback, heartbeats, renewals, and acks.
- `CRONIQ_RUNNER_ID` must match the API client id associated with the API key.

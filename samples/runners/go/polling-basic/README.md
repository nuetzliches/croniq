# Go Runner Sample (Polling)

Minimal HTTP polling runner using the Go SDK.

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
go run .
```

## Notes

- The example polls `/work/poll`, sends a work event, and then acks each lease.
- For long-running jobs, call `Renew` while work is in progress.
- `CRONIQ_RUNNER_ID` must match the API client id associated with the API key.

Go HTTP Worker SDK (minimal)

Requirements:
- Go 1.21+

Run the example:

```bash
cd samples/worker-sdk-go
CRONIQ_API_BASEURL=http://localhost:5080 \
CRONIQ_TENANT_ID=default \
CRONIQ_ENVIRONMENT=dev \
CRONIQ_API_KEY=dev-key \
go run .
```

Notes:
- The example polls `/work/poll` and immediately acks each lease.
- For long-running jobs, call `Renew` while work is in progress.

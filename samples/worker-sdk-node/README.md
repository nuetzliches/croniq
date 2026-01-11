Node HTTP Worker SDK (minimal)

Requirements:
- Node 18+

Run the example:

```bash
cd samples/worker-sdk-node
CRONIQ_API_BASEURL=http://localhost:5080 \
CRONIQ_TENANT_ID=default \
CRONIQ_ENVIRONMENT=dev \
CRONIQ_API_KEY=smoke-key \
CRONIQ_RUNNER_ID=default \
npm install
npm start
```

Notes:
- The example polls `/work/poll`, sends a work event, and then acks each lease.
- For long-running jobs, call `renew` while work is in progress.
- Use `lease.executionId` to correlate logs/events with an execution.
- `CRONIQ_RUNNER_ID` must match the API client id associated with the API key.

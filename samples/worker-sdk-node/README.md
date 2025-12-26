Node HTTP Worker SDK (minimal)

Requirements:
- Node 18+

Run the example:

```bash
cd samples/worker-sdk-node
CRONIQ_API_BASEURL=http://localhost:5080 \
CRONIQ_TENANT_ID=default \
CRONIQ_ENVIRONMENT=dev \
CRONIQ_API_KEY=dev-key \
npm install
npm start
```

Notes:
- The example polls `/work/poll` and immediately acks each lease.
- For long-running jobs, call `renew` while work is in progress.

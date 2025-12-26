# Croniq HTTP Worker (Python)

Minimal example of a **polyglot worker loop** using the HTTP work endpoints:

- `POST /tenants/{tenantId}/work/poll`
- `POST /tenants/{tenantId}/work/renew`
- `POST /tenants/{tenantId}/work/ack`

## Prereqs

- A running Croniq API (devstack recommended)
- An API key with scope `work:execute` (devstack in-memory key works)

## Run with devstack (no built-in worker)

Start only the API profile (so the in-process .NET worker doesn’t steal leases):

```cmd
scripts\devstack-up.cmd --profile api
```

This uses values from `.env` (see `.env.example`). You typically want:

- `CRONIQ_API_BASEURL=http://localhost:5080`
- `CRONIQ_SMOKE_API_KEY=...`
- `CRONIQ_CORE_TENANT_ID=default`
- `CRONIQ_CORE_ENVIRONMENT=dev`

## Run the Python worker

```cmd
cd samples\http-worker-python
python -m venv .venv
.venv\Scripts\pip install -r requirements.txt

set CRONIQ_API_BASEURL=http://localhost:5080
set CRONIQ_SMOKE_API_KEY=smoke-key
set CRONIQ_CORE_TENANT_ID=default
set CRONIQ_CORE_ENVIRONMENT=dev

.venv\Scripts\python worker.py
```

## Notes

- `poll` uses long-polling by passing `waitForMs` (defaults to 25000ms).
- To simulate a long-running task and exercise renewals, set:

```cmd
set CRONIQ_WORKER_SIMULATE_SECONDS=45
set CRONIQ_WORKER_RENEW_EVERY_SECONDS=20
```

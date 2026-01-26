Python HTTP Runner SDK (minimal)

Requirements:
- Python 3.11+

Setup:

```bash
cd samples/worker-sdk-python
python -m venv .venv
.venv\\Scripts\\activate
pip install -r requirements.txt
```

Run the example:

```bash
CRONIQ_API_BASEURL=http://localhost:5080 \
CRONIQ_TENANT_ID=default \
CRONIQ_ENVIRONMENT=dev \
CRONIQ_API_KEY=smoke-key \
CRONIQ_RUNNER_ID=default \
python example.py
```

Notes:
- The example polls `/work/poll`, sends a work event, and then acks each lease.
- For long-running jobs, call `renew` while work is in progress.
- Use `lease.execution_id` to correlate logs/events with an execution.
- `CRONIQ_RUNNER_ID` must match the API client id associated with the API key.

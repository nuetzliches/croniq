# Python Runner Sample (Polling)

Minimal HTTP polling runner using the Python SDK.

## Requirements

- Python 3.11+

## Setup

```bash
cd samples/runners/python/basic
python -m venv .venv
.venv\Scripts\activate
pip install -r requirements.txt
```

## Run

```bash
CRONIQ_API_BASEURL=http://localhost:5080 \
CRONIQ_TENANT_ID=default \
CRONIQ_ENVIRONMENT=dev \
CRONIQ_API_KEY=smoke-key \
CRONIQ_RUNNER_ID=default \
python example.py
```

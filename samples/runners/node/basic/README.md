# Node Runner Sample (Polling)

Minimal HTTP polling runner using the Node SDK.

## Requirements

- Node.js LTS

## Setup

```bash
cd samples/runners/node/basic
npm install
```

## Run

```bash
CRONIQ_API_BASEURL=http://localhost:5080 \
CRONIQ_TENANT_ID=default \
CRONIQ_ENVIRONMENT=dev \
CRONIQ_API_KEY=smoke-key \
CRONIQ_RUNNER_ID=default \
npm run start
```

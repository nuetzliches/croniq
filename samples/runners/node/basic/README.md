# Node Runner Sample

Minimal runner using the Node SDK (gRPC-first with polling fallback).

## Requirements

- Node.js LTS

## Setup

```bash
cd samples/runners/node/basic
npm install
```

`npm install` also installs the local SDK dependencies under `sdk/runner-node` so the gRPC transport can load.

## Run

```bash
CRONIQ_API_BASEURL=http://localhost:5080 \
CRONIQ_TENANT_ID=default \
CRONIQ_ENVIRONMENT=dev \
CRONIQ_RUNNER_NODE_API_KEY=ak_node.dev-secret \
CRONIQ_RUNNER_ID=node-default \
CRONIQ_JOB_KEY=demo-job \
npm run start
```

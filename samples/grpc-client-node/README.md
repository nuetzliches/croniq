Node/TS gRPC Client Sample

Setup:

```bash
cd samples/grpc-client-node
npm install
```

Run:

```bash
CRONIQ_ENDPOINT=localhost:5080 \
CRONIQ_API_KEY=dev-key \
node index.js
```

Notes:

- Uses `@grpc/grpc-js` + `@grpc/proto-loader` (no codegen needed). The proto is loaded from `src/Croniq.Rpc.Client/Protos/scheduler.proto`.
- Metadata: `x-croniq-key` header is set automatically from env; set `CRONIQ_ENVIRONMENT`, `CRONIQ_TENANT_ID`, `CRONIQ_JOB_KEY` as needed.
- Default transport is plaintext (`createInsecure`). Swap to `grpc.credentials.createSsl` for TLS endpoints.

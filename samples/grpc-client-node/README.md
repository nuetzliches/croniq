Node/TS gRPC Client Sample

Setup:

```bash
cd samples/grpc-client-node
npm install
```

Run:

```bash
CRONIQ_ENDPOINT=localhost:5080 \
CRONIQ_API_KEY=smoke-key \
npm start
```

Notes:

- Uses `@grpc/grpc-js` + `@grpc/proto-loader` (no codegen needed). The proto is loaded from `src/Croniq.Rpc.Client/Protos/scheduler.proto`.
- Metadata: `x-croniq-key` header is set automatically from env; set `CRONIQ_JOB_KEY` to override the default job key (`ops:node-demo`).
- Default transport is plaintext (`createInsecure`). Swap to `grpc.credentials.createSsl` for TLS endpoints.

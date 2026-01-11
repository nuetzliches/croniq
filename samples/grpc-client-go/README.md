# Go gRPC Client Sample

Prerequisites:

- Go 1.21+
- `protoc` plus Go plugins (`google.golang.org/protobuf/cmd/protoc-gen-go`, `google.golang.org/grpc/cmd/protoc-gen-go-grpc`)

Generate stubs (from repo root):

```bash
protoc -I src/Croniq.Rpc.Client/Protos \
  --go_out=./samples/grpc-client-go --go_opt=paths=source_relative \
  --go-grpc_out=./samples/grpc-client-go --go-grpc_opt=paths=source_relative \
  src/Croniq.Rpc.Client/Protos/scheduler.proto
```

Run:

```bash
cd samples/grpc-client-go
go mod tidy
CRONIQ_ENDPOINT=localhost:5080 \
CRONIQ_API_KEY=smoke-key \
go run .
```

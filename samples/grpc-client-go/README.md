Go gRPC Client Sample

Voraussetzungen:

- Go 1.21+
- `protoc` + Go plugins (`google.golang.org/protobuf/cmd/protoc-gen-go`, `google.golang.org/grpc/cmd/protoc-gen-go-grpc`)

Stubs generieren (aus Repo-Root):

```bash
protoc -I src/Croniq.Rpc.Client/Protos \
  --go_out=./samples/grpc-client-go --go_opt=paths=source_relative \
  --go-grpc_out=./samples/grpc-client-go --go-grpc_opt=paths=source_relative \
  src/Croniq.Rpc.Client/Protos/scheduler.proto
```

Ausführen:

```bash
cd samples/grpc-client-go
go mod tidy
CRONIQ_ENDPOINT=localhost:5080 CRONIQ_API_KEY=dev-key go run .
```

# Python gRPC Client Sample

Prerequisites:

- Python 3.11+
- Packages: `grpcio`, `grpcio-tools`

Generate stubs (from repo root):

```bash
python -m grpc_tools.protoc -I src/Croniq.Rpc.Client/Protos --python_out=. --grpc_python_out=. src/Croniq.Rpc.Client/Protos/scheduler.proto
```

This creates `scheduler_pb2.py` and `scheduler_pb2_grpc.py` in the current directory.

Run:

```bash
CRONIQ_API_KEY=dev-key \
CRONIQ_ENDPOINT=localhost:5080 \
PYTHONPATH=. \
python client.py
```

Notes:

- Endpoint expects `host:port` (no `http://` prefix).
- The client sets `X-Croniq-Key` metadata; bearer tokens work via `Authorization: Bearer <token>`.

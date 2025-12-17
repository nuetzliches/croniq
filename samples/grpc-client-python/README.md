Python gRPC Client Sample

Voraussetzungen:

- Python 3.11+
- Pakete: `grpcio`, `grpcio-tools`

Stubs generieren (aus Repo-Root):

```bash
python -m grpc_tools.protoc -I src/Croniq.Rpc.Client/Protos --python_out=. --grpc_python_out=. src/Croniq.Rpc.Client/Protos/scheduler.proto
```

Dabei entstehen `scheduler_pb2.py` und `scheduler_pb2_grpc.py` im aktuellen Verzeichnis.

Ausführen:

```bash
X_Croniq_Key=dev-key \
PYTHONPATH=. \
python client.py
```

Hinweise:

- Endpoint erwartet Form `localhost:5080` (kein http:// Prefix).
- Metadata `X-Croniq-Key` wird im Client gesetzt; Bearer geht analog (`Authorization: Bearer <token>`).

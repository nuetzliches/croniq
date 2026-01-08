# Croniq.Sample.Dmz

This sample hosts the DMZ-side ingress:

- `Croniq.Api` in `WebhookAdminOnly` mode (webhook CRUD + health only).
- `Croniq.Webhooks` with `Ingress.DispatchMode=StoreOnly` so ingress persists events instead of executing jobs.
- gRPC ingress stream plus HTTP fallback (SSE/poll) via `MapCroniqWebhookIngressGrpc` for the internal relay worker.

The DMZ host stores webhooks + ingress events in its local SqlServer database and does not open outbound connections.

## Local smoke run

Use `scripts/smoke-dmz.ps1` to start two local SQL containers, apply migrations, launch the DMZ host plus an internal API host in remote mode, send a signed webhook, and wait for the execution to appear. Logs land in `logs/smoke-dmz-*.log`.

## Internal relay configuration (example)

Configure the internal API/worker host to use remote webhooks and enable the relay worker:

```
Croniq:
  Webhooks:
    Mode: Remote
    Remote:
      BaseUrl: https://dmz-croniq.example
      ApiKey: <dmz-admin-or-relay-key>
      TimeoutSeconds: 10
      StreamMode: Grpc
      StreamFallback: Sse
      MaxInflight: 100
      EnableRelay: true
```

The relay API key must include the `webhooks:ingress` scope (and be tenant/environment scoped).

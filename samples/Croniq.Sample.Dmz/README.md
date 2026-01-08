# Croniq.Sample.Dmz

This sample hosts the DMZ-side ingress:

- `Croniq.Api` in `WebhookAdminOnly` mode (webhook CRUD + health only).
- `Croniq.Webhooks` with `Ingress.DispatchMode=StoreOnly` so ingress persists events instead of executing jobs.
- gRPC ingress stream (`MapCroniqWebhookIngressGrpc`) for the internal relay worker.

The DMZ host stores webhooks + ingress events in its local SqlServer database and does not open outbound connections.

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
      MaxInflight: 100
      EnableRelay: true
```

The relay API key must include the `webhooks:ingress` scope (and be tenant/environment scoped).

---
layout: doc
---

# Feature Map

Use this map to jump between the consumer guides, deep-dive internals, and ops runbooks for each feature area.

| Feature | Guide | Deep Dive | Ops |
| --- | --- | --- | --- |
| Webhooks | [Webhooks](./guides/webhooks.md) | [Webhook Trigger Surface](./deep-dive/architecture.md#webhook-trigger-surface), [DMZ Ingress](./deep-dive/designs/dmz-ingress-remote-webhooks.md) | [Container images](./ops/container-images.md) |
| Schedules & Triggers | [Triggers](./guides/triggers.md) | [Scheduler semantics](./deep-dive/architecture.md#scheduler--execution-semantics), [Schedule calendars](./deep-dive/designs/schedule-calendars.md) | [Retention](./ops/retention.md) |
| Auth & Tokens | [Authentication](./guides/auth.md) | [Security baseline](./deep-dive/security.md) | [Troubleshooting](./ops/troubleshooting.md) |
| Workers & Runners | [Workers & Runners](./guides/workers-runners.md) | [Polyglot runner protocol](./deep-dive/designs/polyglot-runner-protocol.md) | [Container images](./ops/container-images.md) |
| Observability | N/A | [Observability](./deep-dive/observability.md) | [Troubleshooting](./ops/troubleshooting.md) |
| Persistence & Data | N/A | [Persistence](./deep-dive/persistence.md) | [Retention](./ops/retention.md) |

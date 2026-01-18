---
layout: doc
---

# Croniq Documentation

Croniq orchestrates distributed workloads for microservice platforms: register jobs once, enforce policies centrally, and stream telemetry into the same observability stack. A tenant-aware scheduler, SqlServer/Postgres-backed durability, and gRPC/REST gateways let platform teams coordinate releases, maintenance tasks, and recurring automations without bolting on per-service cron logic. Operators get one place for throttling and rotating credentials (audit logging is on the roadmap) - while developers stay productive with fluent SDKs and a Docker dev stack. Choose your path:

- [Introduction](./introduction/index.md) - What Croniq is, how to get started, and essential configuration.
- [Deployment modes](./introduction/deployment-modes.md) - Minimal samples vs a separated, self-hosted platform setup.
- [Guides](./guides/index.md) - Deepen your skills with authentication, policies, triggers, webhooks, workers/runners, gRPC, and handler patterns.
- [Feature map](./feature-map.md) - Jump from guides to deep dives and ops runbooks per feature.
- [Operations](./ops/index.md) - Troubleshoot deployments and keep Croniq healthy.
- [Deep Dive](./deep-dive/index.md) - Architecture plans, CI/CD workflows, dev stack, and observability internals.

> Ready to ship? Head straight to the [Quickstart](./introduction/quickstart.md) to run your first job in minutes.

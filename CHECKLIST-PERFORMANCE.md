# CHECKLIST-PERFORMANCE

Performance polish checklist for Croniq. Start with `docs/deep-dive/architecture.md`.

## Goals and baselines

- [ ] Define target SLOs (p95/p99 latency, throughput, worker poll time).
- [ ] Select representative scenarios (API schedule create, trigger, worker poll, webhook ingress).
- [ ] Capture baseline metrics (CPU, memory, GC, DB latency, error rate).
- [ ] Record environment details (hardware, runtime, config).

## Telemetry and profiling

- [ ] Verify OpenTelemetry spans cover end-to-end paths with consistent resource attributes.
- [ ] Create dashboards for latency histograms, queue depth, retries, and DB timings.
- [ ] Capture traces for slow requests and annotate the longest spans.
- [ ] Run CPU and allocation profiling on hot paths.
- [ ] Validate logging volume and level under load.

## Hotspot remediation

- [ ] Reduce allocations and avoid repeated parsing in hot loops.
- [ ] Review JSON serialization and string churn.
- [ ] Validate cache configuration and hit rates.
- [ ] Check DB query plans and indexes; reduce round trips.
- [ ] Tune concurrency limits (parallelism, queue sizes, ThreadPool).
- [ ] Re-measure and document improvements.

## Regression guard

- [ ] Add a repeatable load test recipe.
- [ ] Record baseline numbers and expected regression thresholds.
- [ ] Add perf-focused tests or scripts where practical.

## Pentest concept (Croniq)

- [ ] Define the pentest scope: API, UI, worker/runner endpoints, webhook ingress, devstack/CI assets.
- [ ] Inventory exposed services and ports for each deployment (ApiHost, UiHost, WorkerHost, WebhooksHost, storage).
- [ ] Model instance topology: multiple worker pools (per language runner), separate schedulers, multi-tenant data partitions.
- [ ] Create hardened test fixtures with least-privilege identities and explicit tenant IDs.
- [ ] Verify telemetry coverage (backend + frontend): OpenTelemetry traces, metrics, and logs carry tenant and correlation IDs.
- [ ] Ensure error log completeness: unhandled exceptions, dependency failures, retries, and dead-letter paths are captured.
- [ ] Add red-team style scenarios: credential misuse, token replay, job payload tampering, webhook spoofing, and runner isolation bypass.
- [ ] Capture before/after snapshots: request/trace samples, log counts, alert rates, and DB audit trails.
- [ ] Document remediation playbooks and retest criteria.

## Repeatable pentest scenarios

- [ ] API schedule create/update/delete with malformed payloads and size limits.
- [ ] Worker poll storm and backoff enforcement under queue pressure.
- [ ] Runner sandbox escape attempts with restricted file/network access.
- [ ] Webhook replay/spoofing with signature verification validation.
- [ ] UI auth flows: session expiration, CSRF, and privilege escalation checks.

## Open-source tools (candidates)

- [ ] OWASP ZAP (DAST) for API/UI fuzzing and security scans.
- [ ] OpenAPI fuzzers (e.g., RESTler or Schemathesis) for contract-driven negative testing.
- [ ] k6 or Locust for reproducible load + security scenario scripting.
- [ ] Trivy for container/image scanning in CI.
- [ ] Gitleaks for secret scanning in repo and build artifacts.

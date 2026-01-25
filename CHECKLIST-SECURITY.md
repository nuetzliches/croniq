# CHECKLIST-SECURITY

Security and pentest checklist for Croniq. Start with `docs/deep-dive/architecture.md`.

## Scope and topology

- [ ] Define the pentest scope: API, UI, worker/runner endpoints, webhook ingress, devstack/CI assets.
- [ ] Inventory exposed services and ports for each deployment (ApiHost, UiHost, WorkerHost, WebhooksHost, storage).
- [ ] Model instance topology: multiple worker pools (per language runner), separate schedulers, multi-tenant data partitions.
- [ ] Create hardened test fixtures with least-privilege identities and explicit tenant IDs.

## Telemetry and logging coverage

- [ ] Verify telemetry coverage (backend + frontend): OpenTelemetry traces, metrics, and logs carry tenant and correlation IDs.
- [ ] Ensure error log completeness: unhandled exceptions, dependency failures, retries, and dead-letter paths are captured.
- [ ] Validate log/trace sampling policies under load and during incident scenarios.

## Repeatable pentest scenarios

- [ ] API schedule create/update/delete with malformed payloads and size limits.
- [ ] Worker poll storm and backoff enforcement under queue pressure.
- [ ] Runner sandbox escape attempts with restricted file/network access.
- [ ] Webhook replay/spoofing with signature verification validation.
- [ ] UI auth flows: session expiration, CSRF, and privilege escalation checks.
- [ ] Token replay and stale-session handling across worker and UI boundaries.

## Baselines and regression guard

- [ ] Capture before/after snapshots: request/trace samples, log counts, alert rates, and DB audit trails.
- [ ] Record expected alert thresholds and regression boundaries.
- [ ] Document remediation playbooks and retest criteria.

## Open-source tools (candidates)

- [ ] OWASP ZAP (DAST) for API/UI fuzzing and security scans.
- [ ] OpenAPI fuzzers (e.g., RESTler or Schemathesis) for contract-driven negative testing.
- [ ] k6 or Locust for reproducible load + security scenario scripting.
- [ ] Trivy for container/image scanning in CI.
- [ ] Gitleaks for secret scanning in repo and build artifacts.

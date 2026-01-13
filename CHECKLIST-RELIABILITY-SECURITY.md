# CHECKLIST-RELIABILITY-SECURITY

Reliability and security polish checklist for Croniq. Start with `docs/deep-dive/architecture.md`.

## Reliability
- [ ] Validate cancellation and timeouts on all I/O and background loops.
- [ ] Ensure retries are bounded with backoff; no unbounded retry loops.
- [ ] Confirm idempotency and dead-letter behavior for webhooks and triggers.
- [ ] Verify health endpoints reflect readiness (DB availability) and liveness.
- [ ] Confirm graceful shutdown for worker/hosted services.
- [ ] Add tests for edge cases (timeouts, transient faults, shutdown).

## Security and compliance
- [ ] Confirm no secrets in source control; use `ISecretProvider`.
- [ ] Verify DataProtection key ring setup for webhook secret protection.
- [ ] Ensure tenant/caller identifiers are hashed in telemetry when enabled.
- [ ] Check scope enforcement and tenant isolation paths.
- [ ] Validate input aggressively with clear errors.
- [ ] Review log/trace fields for PII leakage and redact if needed.

## Documentation alignment
- [ ] Update docs for any behavior changes; remove claims not implemented.
- [ ] Record decisions in `docs/deep-dive/architecture.md` where needed.

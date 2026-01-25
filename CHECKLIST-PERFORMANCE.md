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

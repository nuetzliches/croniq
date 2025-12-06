# Croniq Kubernetes Plan

This document expands `CONCEPT.md` section 16 and captures the backlog required to satisfy the checklist item "Kubernetes Chart (charts/croniq) als Backlog-Platzhalter vorbereiten".

## Objectives

- Provide a single Helm chart (or Kustomize base) that deploys Croniq API, worker(s), optional UI, and dependencies (SQL/Xtraq, observability sidecars) across dev/stage/prod clusters.
- Keep the chart in sync with the Docker dev stack and CI pipelines to minimize configuration drift.
- Ship secure-by-default manifests (least privilege, TLS, resource limits) while remaining customizable via `values.yaml` overlays.

## Target Components

1. **Croniq API Deployment**
   - ASP.NET container with readiness/liveness probes (health endpoints) and config via ConfigMap/Secret.
   - Horizontal Pod Autoscaler (CPU + queue-length metric hook) optional.
2. **Croniq Worker Deployment**
   - Same image as API or dedicated worker image; uses leader election configmap if clustering is enabled.
3. **SQL Server / Xtraq**
   - For dev: StatefulSet with persistent volume + init job running `infra/sql/xtraq/apply.ps1` equivalent.
   - For production: support external SQL via connection string secret.
4. **Observability Stack (optional subchart)**
   - OTel Collector Deployment + ConfigMap, optional Grafana/Tempo via dependencies (can be toggled per environment).
5. **Ingress**
   - NGINX/Traefik ingress definitions with TLS; gRPC endpoint support.
6. **Secrets & Config**
   - `Croniq:Auth:Xtraq`, `Croniq:Persistence:Xtraq`, API key seeds, rate limiter config stored in Kubernetes Secrets (sealed-secrets/external secrets optional).
7. **Jobs/CronJobs**
   - Database migrations, policy cleanup, dead-letter sweeps.

## Design Principles

- Namespace scoped deployment; allow multiple Croniq instances with unique release names.
- Strict affinity/anti-affinity guidance to avoid API + SQL on same nodes in production.
- Resource requests/limits anchored from sizing guidance (CPU/RAM per API/worker/collector).
- Use `values.<env>.yaml` overlays for dev/stage/prod; document via `charts/croniq/README.md`.
- Provide PodSecurity preset (baseline) and ServiceAccount/RBAC minimal permissions (mostly for leader election + configmaps).

## Deliverables

- `charts/croniq/Chart.yaml` with dependencies.
- `values.yaml` (defaults) + `values.dev.yaml`, `values.stage.yaml`, `values.prod.yaml` examples.
- Templates for Deployments, StatefulSets, Services, Ingress, Jobs, HPA, ConfigMaps, Secrets.
- `scripts/chart-test.cmd/.sh` wrapping `helm lint` + `helm template` + `kubeconform`.
- CI job (likely nightly) running lint/render/validate.

## Backlog

- [ ] Initialize `charts/croniq` Helm chart skeleton (Chart.yaml, values, README) referencing container images from CI.
- [ ] Author templates for API + worker deployments (probes, env vars, secrets, volumes, service).
- [ ] Add optional SQL StatefulSet + PVC + init job for dev clusters.
- [ ] Provide ServiceAccount/RBAC + PodDisruptionBudget + HPA defaults.
- [ ] Implement Ingress templates with TLS annotations and gRPC support.
- [ ] Add optional OTel collector subchart values and Grafana dashboards configmaps.
- [ ] Document configuration examples (`docs/technical/kubernetes.md` linking to `values.*.yaml`).
- [ ] Integrate helm lint/render tests into CI (nightly) and document release flow for chart packaging.

Completing this backlog will allow the checklist item to be marked done once the chart exists and passes CI validation.

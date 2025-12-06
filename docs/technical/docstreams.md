# Croniq Documentation Streams Plan

This plan explains how we will establish the consumer and technical documentation streams (including the Quickstart) so the checklist item "Docs Streams aufsetzen (docs/consumer, docs/technical) inkl. Quickstart" can be completed. It inventories what already exists and defines the backlog for both audiences.

## Objectives

- Keep consumer docs focused on job authors and integrators (how to run Croniq, configure tenants, write jobs, operate APIs).
- Keep technical docs focused on maintainers/contributors (architecture, providers, deployment, testing, security, observability, CI/CD, dev stack, policies).
- Ensure every user journey starts with a Quickstart and ends with deeper references in the technical stream.
- Provide automated validation (lint + broken links) and establish doc ownership in pull requests.

## Current State Snapshot

- `docs/consumer/README.md` outlines topics and links to existing drafts (`quickstart.md`, `configuration.md`, `policies.md`, `triggers.md`). Quickstart already contains a detailed Hello Croniq workflow.
- `docs/technical/README.md` now links to the architectural addenda (testing, security, observability, policies, CI/CD, dev stack). Additional deep dives (persistence, job registration, auth internals) remain TODO.
- No doc linting CI yet; Quickstart references files such as `docs/technical/persistence.md` that still need to be created.

## Target Information Architecture

### Consumer Stream (`docs/consumer`)

1. `README.md` – navigation + persona guidance.
2. `quickstart.md` – first job & API trigger (already present, needs updates once dev stack ready).
3. `configuration.md` – environment variables, connection strings, auth modes (exists, extend with OIDC/API-key instructions once implemented).
4. `policies.md` – consumer view of retries/misfires/quotas with practical examples; references technical policy doc for internals.
5. `triggers.md` – descriptions of cron/interval/one-off triggers and CLI/API payloads.
6. `auth.md` – how to create API keys, manage tenants, integrate OIDC (new).
7. `devstack.md` (consumer variant) – how to start the Docker stack locally.
8. `troubleshooting.md` – FAQ, log locations, health checks.

### Technical Stream (`docs/technical`)

- Already contains architecture/detailed plans. Remaining additions:
  - `persistence.md` (Xtraq schema, stored procedures, migration process).
  - `job-registration.md` (core startup flow, DI, metadata sync).
  - `auth.md` (provider contracts, API endpoints) referencing the security baseline.
  - `release.md` or expand `ci.md` with release governance.

### Cross-linking

- Consumer docs should highlight “Learn more” sections pointing to technical deep dives.
- Technical docs reference consumer guides as the entry point for external teams.

## Processes & Tooling

- Add documentation linting to CI (`lychee` for links, `markdownlint-cli2` or `cspell`).
- Introduce `docs/_templates` for reusable callouts/examples if needed.
- Define doc owners in CODEOWNERS (e.g., `docs/consumer/* @doc-crew`, `docs/technical/* @maintainers`).
- Provide contribution guidelines in `docs/README.md` describing style (language, tone, code blocks, callouts).

## Backlog to Complete the Checklist Item

- [ ] Create missing consumer topics: `auth.md`, `devstack.md`, `troubleshooting.md`; align Quickstart references with actual files.
- [ ] Flesh out technical deep dives: `persistence.md`, `job-registration.md`, `auth.md` (internal), ensuring cross-links exist.
- [ ] Add docs linting + broken-link check to CI (`ci-nightly.yml` per plan) and provide instructions for local execution.
- [ ] Update `docs/consumer/README.md` with a table of personas/topics and include navigation to new files.
- [ ] Add `docs/README.md` (root) summarizing both streams and how to contribute.
- [ ] Ensure Quickstart references the Docker dev stack instructions once the stack exists.
- [ ] Document doc ownership and review expectations in `CONTRIBUTING.md` or CODEOWNERS.

Once these actions are delivered, the docs streams will be considered established and discoverable.

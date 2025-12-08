# Croniq Documentation Streams Plan

This plan explains how we will establish the consumer and technical documentation streams (including the Quickstart) so the checklist item "Docs Streams aufsetzen (docs root, docs/deep-dive) inkl. Quickstart" can be completed. It inventories what already exists, maps the personas, and defines the backlog for both audiences.

## Objectives

- Keep consumer docs focused on job authors and integrators (how to run Croniq, configure tenants, write jobs, operate APIs) while staying intentionally lean: fast onboarding, no deep debugging guidance, zero-config defaults. Every page must link to the relevant deep-dive topic instead of duplicating instructions.
- Keep technical docs focused on maintainers/contributors (architecture, providers, deployment, testing, security, observability, CI/CD, dev stack, policies).
- Ensure every user journey starts with a Quickstart and ends with deeper references in the technical stream.
- Provide automated validation (lint + broken links) and establish doc ownership in pull requests.
- Standardize diagramming on Mermaid for all new assets in `docs/deep-dive/` (sequence diagrams, architecture sketches, etc.) and phase out draw.io references.

## Current State Snapshot

- `docs/README.md` outlines topics and links to existing drafts (`quickstart.md`, `configuration.md`, `policies.md`, `triggers.md`). Quickstart already contains a detailed Hello Croniq workflow.
- `docs/deep-dive/README.md` now links to the architectural addenda (testing, security, observability, policies, CI/CD, dev stack). Additional deep dives (persistence, job registration, auth internals) remain TODO.
- No doc linting CI yet; Quickstart references files such as `docs/deep-dive/persistence.md` that still need to be created.

## Target Information Architecture

| Persona                       | Landing doc                            | Goal                                                                                         |
| ----------------------------- | -------------------------------------- | -------------------------------------------------------------------------------------------- |
| Job author / integrator       | `docs/README.md`                       | Reach Quickstart within minutes, configure basic auth/settings without deep debugging steps. |
| Platform/Ops engineer         | `docs/deep-dive/devstack.md` + `ci.md` | Run the Croniq dev stack, diagnose telemetry, and extend pipelines.                          |
| Croniq contributor/maintainer | `docs/deep-dive/README.md`             | Understand architecture, provider contracts, backlog plans, and governance expectations.     |

### Consumer Stream (docs root)

1. `README.md` – navigation + persona guidance.
2. `quickstart.md` – first job & API trigger (already present, needs updates once dev stack ready).
3. `configuration.md` – environment variables, connection strings, auth modes (exists, extend with OIDC/API-key instructions once implemented).
4. `policies.md` – consumer view of retries/misfires/quotas with practical examples; references technical policy doc for internals.
5. `triggers.md` – descriptions of cron/interval/one-off triggers and CLI/API payloads.
6. `auth.md` – how to create API keys, manage tenants, integrate OIDC (new, short, links to `./deep-dive/security.md`).
7. `troubleshooting.md` – FAQ, log locations, health checks (keeps instructions lean and links to `docs/deep-dive/devstack.md`, `observability.md`, `ci.md` for deeper diagnosis; begins with a "Fast path" section that sends users straight to the dev stack instructions when local repros are required).

> Guardrail: Consumer docs live directly under `docs/` so contributors see them first. Keep these files concise—just enough to get a schedule running—while pointing to `docs/deep-dive/*` (e.g., dev stack, observability, CI/CD) for diagnostics, provider internals, or infrastructure notes.

### Technical Stream (`docs/deep-dive`)

- Already contains architecture/detailed plans (ci, testing, supplychain, observability, devstack, policies, security, kubernetes, ui).
- Remaining additions:
  - `persistence.md` (SqlServer schema, EF migrations, DbContext model, migration workflow).
  - `job-registration.md` (startup flow, DI, metadata sync, Croniq.Sdk contracts).
  - `auth.md` (provider contracts, API endpoints) referencing the security baseline.
  - Release governance content (new `release.md` or an expanded section inside `ci.md`) describing signing, SBOM evidence, and verification commands.

### Cross-linking

- Consumer docs should highlight “Learn more” sections pointing to technical deep dives.
- Technical docs reference consumer guides as the entry point for external teams.

## Implementation Roadmap

1. **Foundation** – directory split and README updates are complete; remaining work: surface this plan from both READMEs and codify ownership in CODEOWNERS/CONTRIBUTING.
2. **Content parity** – deliver missing consumer files (`auth.md`, `troubleshooting.md`), refresh Quickstart/configuration, and finish the deep-dive backlog (`persistence`, `job-registration`, `auth`, release governance) so every "Learn more" link resolves.
3. **Tooling & validation** – host the docs via VitePress (npm project now lives in `docs/package.json`), add markdown lint + link checks (lychee, markdownlint-cli2) to `nightly.yml`, document local commands here, and provide optional `docs/_templates` callouts for consistent messaging. A manual-only GitHub Action (`docs-preview.yml`) already builds the site on demand until public hosting is approved.
4. **Governance** – document review expectations per stream, add a docs-status badge to `README.md`, and script a cross-link validator ensuring consumer docs only reference existing deep-dive files.

## Static Site Publishing (VitePress)

- The documentation site is powered by [VitePress](https://vitepress.dev/) with Mermaid enabled in `.vitepress/config.ts`.
- Run commands from the `docs/` directory: `npm install`, `npm run docs:dev`, `npm run docs:build`, `npm run docs:preview`.
- CI/GitHub Pages flow: `cd docs && npm ci && npm run docs:build`; publish the `.vitepress/dist` folder to `gh-pages`. A manual workflow (`.github/workflows/docs-preview.yml`) already executes these steps and uploads the artifact; automatic deploy remains disabled while the repo is private.
- Keep the navigation/sidebars in `docs/.vitepress/config.ts` aligned with the consumer/deep-dive structure and update them whenever new files are added.
- Mermaid is first-class (set via `mermaid: true`); diagrams render automatically both locally and on GitHub Pages.

## Processes & Tooling

- Add documentation linting to CI (`lychee` for links, `markdownlint-cli2` or `cspell`) and describe local usage (`lychee docs --fail`, `markdownlint-cli2 "docs/**/*.md"`).
- Introduce `docs/_templates` for reusable callouts/examples if needed.
- Define doc owners in CODEOWNERS (e.g., `docs/*.md @doc-crew`, `docs/deep-dive/* @maintainers`).
- Provide contribution guidelines in `docs/README.md` describing style (language, tone, code blocks, callouts), and capture review expectations inside CONTRIBUTING. Reference the VitePress workflow so contributors know where `package.json` lives.
- Author all new diagrams in Mermaid. For architecture views still stored in `architecture.drawio`, plan a migration path (export to Mermaid or recreate) and state the standard in CONTRIBUTING.

## Backlog to Complete the Checklist Item

- [ ] Create missing consumer topics: `auth.md`, `troubleshooting.md`, and update `quickstart.md`/`configuration.md` so all "Learn more" links resolve to live files.
- [ ] Deliver pending deep dives: `persistence.md`, `job-registration.md`, `auth.md`, plus a release-governance addendum (new `release.md` or an expanded `ci.md`).
- [ ] Add docs linting + broken-link checks to `nightly.yml` and document the local commands in this plan.
- [ ] Expand `docs/README.md` with a persona table/navigation cards mirroring the table above.
- [ ] Ensure Quickstart references both the Docker dev stack instructions (`./deep-dive/devstack.md`) and the upcoming `troubleshooting.md` page.
- [ ] Document doc ownership and review expectations in `CONTRIBUTING.md` or CODEOWNERS.
- [ ] Add `docs/_templates/` snippets for reusable callouts shared between consumer + deep-dive pages.
- [ ] Replace or redraw existing draw.io diagrams with Mermaid equivalents where practical and document the standard in CONTRIBUTING.
- [>] Add a GitHub Actions workflow that runs `npm ci && npm run docs:build` inside `docs/` and deploys the VitePress site to GitHub Pages (or an internal static host). Implemented as `.github/workflows/docs-preview.yml`, currently manual-only. When ready to publish, extend this workflow with a deploy step (e.g., actions/deploy-pages) and enable it on pushes/tags.

Once these actions are delivered, the docs streams will be considered established and discoverable.

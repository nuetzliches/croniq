# Croniq Documentation Streams Plan

This living note tracks how we keep the consumer-facing docs in sync with the deep-dive reference set. The streams and review expectations are active; only public publishing is deferred until the repo is public. We already run docs lint/link checks nightly and provide a manual preview workflow.

## Objectives

- Keep consumer docs (`docs/` root) short, task-driven, and opinionated so job authors can run Croniq within minutes.
- Capture architecture, provider contracts, deployment guidance, and backlog planning inside `docs/deep-dive/` for contributors and platform engineers.
- Cross-link both streams so every quickstart step offers a "Learn more" path into the corresponding deep dive, while technical docs point back to the canonical onboarding guides.
- Enforce quality via automated linting/link checks, shared templates, and documented review ownership.

## Streams Snapshot (December 2025)

### Consumer Stream (`docs/`)

| Area           | Files                                                                                  | Notes                                                                                                                 |
| -------------- | -------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| Landing + Hero | `index.md`, `README.md`                                                                | Hero page explains Croniq's value; repo README links to each section and preview commands.                            |
| Introduction   | `introduction/index.md`, `introduction/quickstart.md`, `introduction/configuration.md` | Quickstart drives a Hello Croniq scenario, references configuration, dev stack, and troubleshooting pages.            |
| Guides         | `guides/auth.md`, `guides/policies.md`, `guides/triggers.md`, `guides/webhooks.md`, `guides/workers-runners.md`, `guides/grpc.md`, `guides/handlers.md` | Covers auth modes, policy options, trigger payloads, webhooks, worker integration, and handler patterns with "Learn more" callouts into deep dives. |
| Operations     | `ops/troubleshooting.md`                                                               | Fast-path troubleshooting with links to dev stack, observability, and CI docs for deeper debugging.                   |

### Deep-Dive Stream (`docs/deep-dive/`)

| Topic              | Files                                                                           | Highlights                                                                                            |
| ------------------ | ------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| Overview           | `index.md`, `architecture.md`                                                   | Architecture reference plus service boundaries, SLOs, and system diagram (Mermaid).                   |
| Delivery & Release | `ci.md`, `release.md`, `supplychain.md`                                         | CI/nightly strategy, SemVer/SBOM playbook, signing, and compliance artifacts.                         |
| Runtime Internals  | `persistence.md`, `job-registration.md`, `auth.md`, `policies.md`, `testing.md` | Persistence schema, job discovery flow, auth provider contracts, policy engine, and testing approach. |
| Operations         | `devstack.md`, `observability.md`, `security.md`, `kubernetes.md`, `ui.md`      | Docker dev stack, telemetry stack, security posture, future Kubernetes/UI plans.                      |
| Governance         | `docstreams.md` (this file)                                                     | Tracks ownership, tooling, and outstanding backlog.                                                   |

## Sync Cadence & Owners

| Cadence | Task                                                                     | Owner(s)                                            | Tooling                                                   |
| ------- | ------------------------------------------------------------------------ | --------------------------------------------------- | --------------------------------------------------------- |
| Weekly  | Review merged consumer docs, mirror technical references (or vice versa) | Docs Crew (consumer) + Core Maintainers (deep dive) | `gh issue list --label docs`, VitePress preview artifact  |
| Release | Run full docs CI (lint, lychee, SBOM, accessibility) before tagging      | Release Captain                                     | `.github/workflows/docs-preview.yml`, `npm run docs:lint` |
| Nightly | Scheduled link check + Mermaid rendering validation                      | Automation Bot                                      | `.github/workflows/nightly.yml`                           |

Every PR touching `docs/**` must tag at least one representative from each stream. If a change only exists in one stream, open a follow-up issue tagged `docs-sync` so we never ship stale onboarding steps.

## End-to-End Workflow

1. **Open/triage** a `docs-sync` issue describing the source of truth (consumer vs deep dive) and the affected files.
2. **Edit in pairs**: mirror the change in both locations (e.g., update `guides/webhooks.md` _and_ `deep-dive/architecture.md`). Reuse shared snippets from `docs/_templates/` when possible.
3. **Run local quality gates**: `npm run docs:lint`, `lychee --config docs/.lychee.toml docs`, optional `npm run docs:build` to confirm Mermaid diagrams render.
4. **Request cross-stream review** so both the Docs crew and Core maintainers sign off.
5. **Ship + log**: mention the `docs-sync` issue in the PR description so the weekly triage board stays accurate.

## Templates, Mermaid & Callouts

- All diagrams must be written as inline Mermaid blocks so GitHub + VitePress have identical output.
- Use `_templates/learn-more-callout.md` (consumer -> deep dive) and `_templates/ops-warning.md` for consistent tone; additional templates will be added once the repo is public.
- Keep frontmatter minimal; every page should declare `title` and `description` to power search.

## Cross-Linking Guardrails

- Consumer pages stay lean: each section ends with a "Learn more" callout pointing at the matching deep-dive page (templates live under `docs/_templates/`).
- Deep-dive topics reference the consumer quickstart/configuration docs whenever readers should begin with user-facing instructions.
- Quickstart and Troubleshooting highlight `docs/deep-dive/devstack.md` and `observability.md` before asking contributors to debug locally.

## Tooling & Governance

- Run docs locally from the `docs/` directory:
  - `npm install`
  - `npm run docs:dev`
  - `npm run docs:build`
  - `npm run docs:preview`
- Quality gates:
  - `npm run docs:lint` executes `markdownlint-cli2` across the tree.
  - `lychee --config docs/.lychee.toml docs` checks links (this also runs nightly via `.github/workflows/nightly.yml`).
- Shared snippets sit in `docs/_templates/`; reuse them for callouts to keep tone and structure consistent.
- Review/ownership expectations are documented in `CONTRIBUTING.md` (Docs crew owns `docs/**`, Core maintainers own `docs/deep-dive/**` and automation).
- All diagrams must be authored as Mermaid code blocks directly inside the Markdown files so VitePress + GitHub renderings stay identical.

## Publishing & Automation

- `.github/workflows/docs-preview.yml` manually builds the VitePress site and uploads the artifact for reviewers.
- Nightly CI runs docs linting and link checks (`.github/workflows/nightly.yml`).
- Publishing is blocked until the repo is public; once unblocked, add a publish workflow (GitHub Pages or internal static host) on pushes/tags.

## Open Item

- [ ] (blocked until repo is public) Add a publish workflow to push `.vitepress/dist` after successful builds.

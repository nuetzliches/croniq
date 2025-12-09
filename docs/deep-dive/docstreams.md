# Croniq Documentation Streams Plan

This living note tracks how we keep the consumer-facing docs in sync with the deep-dive reference set. Both streams now exist and stay healthy through linting, ownership rules, and nightly validation. The only remaining backlog item is enabling an automated GitHub Pages deployment.

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
| Guides         | `guides/auth.md`, `guides/policies.md`, `guides/triggers.md`, `guides/handlers.md`     | Covers auth modes, policy options, trigger payloads, and handler patterns with "Learn more" callouts into deep dives. |
| Operations     | `ops/troubleshooting.md`                                                               | Fast-path troubleshooting with links to dev stack, observability, and CI docs for deeper debugging.                   |

### Deep-Dive Stream (`docs/deep-dive/`)

| Topic              | Files                                                                           | Highlights                                                                                            |
| ------------------ | ------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| Overview           | `index.md`, `architecture.md`                                                   | Architecture reference plus service boundaries, SLOs, and system diagram (Mermaid).                   |
| Delivery & Release | `ci.md`, `release.md`, `supplychain.md`                                         | CI/nightly strategy, SemVer/SBOM playbook, signing, and compliance artifacts.                         |
| Runtime Internals  | `persistence.md`, `job-registration.md`, `auth.md`, `policies.md`, `testing.md` | Persistence schema, job discovery flow, auth provider contracts, policy engine, and testing approach. |
| Operations         | `devstack.md`, `observability.md`, `security.md`, `kubernetes.md`, `ui.md`      | Docker dev stack, telemetry stack, security posture, future Kubernetes/UI plans.                      |
| Governance         | `docstreams.md` (this file)                                                     | Tracks ownership, tooling, and outstanding backlog.                                                   |

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
- Nightly CI already runs docs linting, link checks, SBOM scans, and smoke tests.
- Next improvement: trigger a publish workflow (GitHub Pages or internal static host) on pushes/tags so the docs stay live without manual intervention.

## Open Item

- [ ] Extend the `docs-preview` GitHub Action (or add a dedicated workflow) to publish `.vitepress/dist` automatically after successful builds.

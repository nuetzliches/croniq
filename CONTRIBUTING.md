# Contributing to Croniq

Thanks for helping evolve Croniq. This document captures the expectations for documentation pull requests along with the commands you need before opening a review.

## Quick Start

1. Fork or clone the repository and create a topic branch.
2. Install the documentation toolchain once: `cd docs && npm install`.
3. Run the docs site locally via `npm run docs:dev` while you edit. The VitePress project already lives in `docs/`.
4. Keep Croniq docs (`docs/introduction/**`, `docs/guides/**`, `docs/ops/**`) short and task-oriented. Link to `docs/deep-dive/*` rather than duplicating architecture decisions.
5. Capture deeper explanations, diagrams, and backlog notes in `docs/deep-dive/`.

## Required Checks for Documentation PRs

Run these commands from the `docs/` folder before pushing:

- `npm run docs:lint` – Markdown style validation via `markdownlint-cli2` (rules configured in `.markdownlint-cli2.jsonc`).
- `npm run docs:build` – Ensures VitePress can render every page.
- `lychee --config docs/.lychee.toml docs` – Optional local link sweep (install via `cargo install lychee` or run `docker run --rm -v %CD%:/work -w /work ghcr.io/lycheeverse/lychee lychee --config docs/.lychee.toml docs`). The nightly workflow enforces this automatically, but running it locally avoids surprises when adding many new links.

If you touch workflows or automation, also run `npm run docs:preview` to inspect the production build and re-run any affected GitHub Actions locally via [`act`](https://github.com/nektos/act) if possible.

## CI Workflow Quickstart

The canonical reference for Croniq CI/CD lives in [`docs/deep-dive/ci.md`](docs/deep-dive/ci.md). For fast local validation, reuse the same helper scripts the workflows call:

```pwsh
# Run individual suites (mirrors ci-pr.yml matrix)
pwsh ./scripts/ci/run-tests.ps1 -Project tests/Croniq.Core.Tests/Croniq.Core.Tests.csproj -DisplayName "Croniq.Core.Tests"

# Aggregate coverage + enforce gates
reportgenerator "-reports:coverage/**/coverage.cobertura.xml" "-targetdir:coverage/report" -reporttypes:JsonSummary
python scripts/ci/enforce_coverage_thresholds.py coverage/report/Summary.json

# Bring up the dev stack like nightly/release smoke
pwsh ./scripts/ci/compose-devstack.ps1 -Action Up
```

Terminate the stack via `pwsh ./scripts/ci/compose-devstack.ps1 -Action Down -CaptureLogs` when you are done. Always include relevant CI output (coverage summary, smoke logs) in PR descriptions when touching workflows or automation.

## Documentation Ownership & Review

Always request a reviewer from the stream that owns the files you touched:

| Scope       | Paths                                                           | Reviewers                                             |
| ----------- | --------------------------------------------------------------- | ----------------------------------------------------- |
| Croniq docs | `docs/introduction/**`, `docs/guides/**`, `docs/ops/**`         | Tag the Docs crew (`@nuetzliches/docs`)               |
| Deep-dive   | `docs/deep-dive/**`                                             | Tag the Core maintainers (`@nuetzliches/maintainers`) |
| Site chrome | `docs/.vitepress/**`                                            | Docs crew + Core maintainers                          |
| Automation  | `.github/workflows/docs-*.yml`, `.github/workflows/nightly.yml` | Core maintainers                                      |

If a GitHub team does not exist yet, call it out in the PR and assign the relevant maintainers manually. Documentation PRs should not merge without an approval from the owning stream.

## Style Guardrails

- Prefer ASCII in prose/code snippets unless the target file already relies on Unicode characters.
- Introduce a concept once, then link to a deep-dive topic for background reading.
- Keep headers short and action-oriented; surface “Learn more” links whenever you branch into the other stream.
- Reference shared examples from `docs/_templates/` instead of duplicating callouts once that directory is populated.
- New diagrams must use Mermaid code fences. For legacy draw.io assets, document the conversion approach in the PR description until they are fully migrated.

## Docstreams & Workflow

- `docs/deep-dive/docstreams.md` stays the canonical backlog for active documentation streams (Croniq docs vs. deep-dive). Even while the public repo is pending, mention the stream you are contributing to in the PR description so review ownership stays clear.
- When a doc PR spans both streams, split the commits or call out the affected sections explicitly to avoid conflating consumer guidance with deep technical notes.
- Future automation will validate docstreams metadata automatically; until then, reviewers enforce it manually.

See `docs/deep-dive/docstreams.md` for the living backlog that tracks the remaining documentation work streams.

## UI Contributions

- The Croniq admin UI architecture, guardrails, and open questions live in [`src/Croniq.Ui/docs/deep-dive/designs/angular-ui-concept.md`](src/Croniq.Ui/docs/deep-dive/designs/angular-ui-concept.md); reference it before proposing frontend changes.
- Track implementation progress in [`CHECKLIST-UI.md`](CHECKLIST-UI.md) and update checkboxes when a delivery phase lands.
- UI work targets Angular 21 + Tailwind; follow the official Angular Tailwind integration guidance at [https://next.angular.dev/guide/tailwind](https://next.angular.dev/guide/tailwind) when modifying builders or content scanning settings.
- When using AI assistance, lean on Angular's "Develop with AI" workflows [https://next.angular.dev/ai/develop-with-ai](https://next.angular.dev/ai/develop-with-ai) plus the AI design patterns catalog [https://next.angular.dev/ai/design-patterns](https://next.angular.dev/ai/design-patterns) to keep generated components idiomatic and reviewed before merging.
- The Angular MCP server described in the concept document is optional and dev-only; start it with `npm run mcp` (or the "Angular MCP Server" VS Code task) inside `src/Croniq.Ui` and capture any new scaffolding recipes in `.vscode/tasks.json` or docs so other contributors can reproduce them.

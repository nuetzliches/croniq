# Contributing to Croniq

Thanks for helping evolve Croniq. This document captures the expectations for documentation pull requests along with the commands you need before opening a review.

## Quick Start

1. Fork or clone the repository and create a topic branch.
2. Install the documentation toolchain once: `cd docs && npm install`.
3. Run the docs site locally via `npm run docs:dev` while you edit. The VitePress project already lives in `docs/`.
4. Keep consumer content (`docs/*.md`) short and task-oriented. Link to `docs/deep-dive/*` rather than duplicating architecture details.
5. Capture deeper explanations, diagrams, and backlog notes in `docs/deep-dive/`.

## Required Checks for Documentation PRs

Run these commands from the `docs/` folder before pushing:

- `npm run docs:lint` – Markdown style validation via `markdownlint-cli2` (rules configured in `.markdownlint-cli2.jsonc`).
- `npm run docs:build` – Ensures VitePress can render every page.
- `lychee --config docs/.lychee.toml docs` – Optional local link sweep (install via `cargo install lychee` or run `docker run --rm -v %CD%:/work -w /work ghcr.io/lycheeverse/lychee lychee --config docs/.lychee.toml docs`). The nightly workflow enforces this automatically, but running it locally avoids surprises when adding many new links.

If you touch workflows or automation, also run `npm run docs:preview` to inspect the production build and re-run any affected GitHub Actions locally via [`act`](https://github.com/nektos/act) if possible.

## Documentation Ownership & Review

Always request a reviewer from the stream that owns the files you touched:

| Scope          | Paths                                                           | Reviewers                                             |
| -------------- | --------------------------------------------------------------- | ----------------------------------------------------- |
| Consumer docs  | `docs/*.md`, `docs/consumer/**`                                 | Tag the Docs crew (`@nuetzliches/docs`)               |
| Deep-dive docs | `docs/deep-dive/**`, `docs/technical/**`                        | Tag the Core maintainers (`@nuetzliches/maintainers`) |
| Site chrome    | `docs/.vitepress/**`                                            | Docs crew + Core maintainers                          |
| Automation     | `.github/workflows/docs-*.yml`, `.github/workflows/nightly.yml` | Core maintainers                                      |

If a GitHub team does not exist yet, call it out in the PR and assign the relevant maintainers manually. Documentation PRs should not merge without an approval from the owning stream.

## Style Guardrails

- Prefer ASCII in prose/code snippets unless the target file already relies on Unicode characters.
- Introduce a concept once, then link to a deep-dive topic for background reading.
- Keep headers short and action-oriented; surface “Learn more” links whenever you branch into the other stream.
- Reference shared examples from `docs/_templates/` instead of duplicating callouts once that directory is populated.
- New diagrams must use Mermaid code fences. For legacy draw.io assets, document the conversion approach in the PR description until they are fully migrated.

See `docs/deep-dive/docstreams.md` for the living backlog that tracks the remaining documentation work streams.

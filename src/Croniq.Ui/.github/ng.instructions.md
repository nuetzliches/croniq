# Croniq UI – Angular Workflow Instructions

These instructions complement `.github/copilot.instructions.md` and capture repository-specific workflow rules for the Angular workspace.

## Angular UI (Croniq.Ui) Workflow

- When working in the Angular workspace under `src/Croniq.Ui`, start the Angular MCP server first (VS Code task **Angular MCP Server** or `npm run mcp`).
- Prefer the MCP tooling to discover project structure and confirm best practices before making Angular code changes.
- Prefer `computed()` and `linkedSignal()` for reactive/derived UI state. Avoid `effect()` unless you are performing an imperative side effect (e.g., DOM APIs, logging, integration glue); if you must use an effect, keep it small and document why.
- Keep UX/documentation artifacts (e.g., wireframes in `src/Croniq.Ui/docs/deep-dive/designs/`) in sync with the OpenAPI snapshot when adding UI flows.

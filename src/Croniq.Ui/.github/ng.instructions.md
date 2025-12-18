# Croniq UI – Angular Workflow Instructions

These instructions complement `.github/copilot.instructions.md` and capture repository-specific workflow rules for the Angular workspace.

## Angular UI (Croniq.Ui) Workflow

- When working in the Angular workspace under `src/Croniq.Ui`, start the Angular MCP server first (VS Code task **Angular MCP Server** or `npm run mcp`).
- Prefer the MCP tooling to discover project structure and confirm best practices before making Angular code changes.
- Prefer `computed()` and `linkedSignal()` for reactive/derived UI state. Avoid `effect()` unless you are performing an imperative side effect (e.g., DOM APIs, logging, integration glue); if you must use an effect, keep it small and document why.
- Keep UX/documentation artifacts (e.g., wireframes in `src/Croniq.Ui/docs/deep-dive/designs/`) in sync with the OpenAPI snapshot when adding UI flows.
- Follow Angular's official coding style guide for naming and structure: https://angular.dev/style-guide
- Standalone import hygiene: avoid importing `CommonModule`. Import only the standalone directives/pipes you use (e.g. `DatePipe`) and prefer built-in control flow (`@if`, `@for`, `@switch`).
- Avoid Angular lifecycle hook methods (`ngOnInit`, `ngOnDestroy`, etc.). Prefer:
  - Route guards/resolvers for navigation and preloading
  - signals + `computed()` for state
  - `takeUntilDestroyed(inject(DestroyRef))` for teardown
    (Enforced by ESLint.)

## Accessibility (A11y)

- Prefer Angular Aria (`@angular/aria`) for common WAI-ARIA patterns (tabs, menus, toolbars, listbox, etc.) instead of maintaining custom keyboard/ARIA/focus code.
- Follow the official Angular Aria guides for structure and behavior:
  - Installation: https://angular.dev/guide/aria/overview#installation
  - Tabs: https://angular.dev/guide/aria/tabs

### Tabs (Angular Aria)

- Use `@angular/aria/tabs` for any tabbed UI; do not hand-roll ARIA/keyboard logic.
- Keep the tabs selection API consistent across pages:
  - Maintain `selectedTab` as a signal.
  - Provide a `setSelectedTab(nextTab)` method that applies `$event` and falls back to the first configured tab.
  - In templates, bind `(selectedTabChange)="setSelectedTab($event)"` (avoid duplicating fallback logic inline).

## Component granularity

- Create components/directives in the smallest useful unit (Pages are the exception and may compose multiple smaller building blocks).
- Avoid “page-sized” shared components; prefer headless primitives (e.g. Angular Aria directives) + tiny presentational components.

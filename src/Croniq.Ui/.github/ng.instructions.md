# Croniq UI – Angular Workflow Instructions

These instructions complement `.github/copilot.instructions.md` and capture repository-specific workflow rules for the Angular workspace.

## Angular UI (Croniq.Ui) Workflow

- When working in the Angular workspace under `src/Croniq.Ui`, start the Angular MCP server first (VS Code task **Angular MCP Server** or `npm run mcp`).
- Prefer the MCP tooling to discover project structure and confirm best practices before making Angular code changes.
- Prefer `computed()` and `linkedSignal()` for reactive/derived UI state. Avoid `effect()` unless you are performing an imperative side effect (e.g., DOM APIs, logging, integration glue); if you must use an effect, keep it small and document why.
- **State Synchronization**: Use `linkedSignal()` to synchronize local state with inputs (e.g., resetting a form when an input changes). Avoid using `effect()` for this purpose, as it indicates a reactive design issue.
- Keep UX/documentation artifacts (e.g., wireframes in `src/Croniq.Ui/docs/deep-dive/designs/`) in sync with the OpenAPI snapshot when adding UI flows.
- Follow Angular's official coding style guide for naming and structure: https://angular.dev/style-guide
- **Components & Directives**:
  - **Standalone**: All components are standalone by default in Angular v19+. **DO NOT** set `standalone: true` in the `@Component` decorator.
  - **Imports**: **DO NOT** import `CommonModule`. Import specific dependencies (e.g., `DatePipe`, `JsonPipe`) directly. Avoid NgModule imports in general (e.g., prefer standalone CDK directives like `CdkVirtualScrollViewport`/`CdkVirtualForOf` over `ScrollingModule`).
  - **Change Detection**: Always use `ChangeDetectionStrategy.OnPush`.
  - **Inputs/Outputs**: Use the `input()` and `output()` functions instead of `@Input()` and `@Output()` decorators.
  - **Dependency Injection**: Use the `inject()` function for all dependencies. Avoid constructor injection.
- **Forms (Signal Forms)**:
  - We use the **Signal Forms** API (`@angular/forms/signals`) for all new forms.
  - **Do not** use `ReactiveFormsModule` (`FormGroup`, `FormControl`) or `FormsModule` (`ngModel`) unless strictly necessary for legacy support.
  - **Pattern**:
    1.  **View Model**: Define a specific interface for the form model (e.g., `ScheduleFormModel`) to handle type mismatches between the API (often nullable) and the UI (strict strings/booleans). Avoid using API types directly in the form signal if they contain `null`.
    2.  **Mapper**: Create a pure function outside the class to map the Input/API model to the View Model.
    3.  **State**: Use `linkedSignal()` to initialize the form model from an input signal. This ensures the form resets automatically when the input changes.
    4.  **Form Definition**: Use the `form()` function to bind the signal to validation rules.
    5.  **Template Binding**: Use the `[field]` directive to bind inputs to form fields. Avoid `$any()` casts in templates by ensuring the View Model is strictly typed.
- Avoid Angular lifecycle hook methods (`ngOnInit`, `ngOnDestroy`, etc.). Prefer:
  - Route guards/resolvers for navigation and preloading
  - signals + `computed()` for state
  - `takeUntilDestroyed(inject(DestroyRef))` for teardown
    (Enforced by ESLint.)
- **Route-bound selection (required)**:
  - For list/detail pages, bind the selected entity to a query param and keep it in sync.
  - Use `cq-data-grid` with `idKey`, `selectedId`, and `selectedIdChange` to drive selection.
  - Read initial selection from `ActivatedRoute.queryParamMap`, update it via `Router.navigate` with `queryParamsHandling: 'merge'`.
  - Never rely on component-local selection state alone for cross-page navigation.

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

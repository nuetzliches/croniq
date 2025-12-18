# Croniq UI – AI System Instructions

> Repo-specific Angular workflow rules live in `.github/ng.instructions.md`.

## Persona and Tone

- You are an expert in Angular v20+, TypeScript, RxJS, and scalable enterprise frontends.
- Optimize for maintainability, performance, and accessibility; prefer concise, well-structured answers with actionable steps.
- Give runnable examples that align with this repository when code is requested.

## TypeScript Guardrails

- Assume strict type checking; prefer type inference when obvious and avoid `any`. Use `unknown` plus type narrowing when a value is uncertain.
- Favor enums, discriminated unions, and readonly types for shapes shared across the app.
- Never silence the compiler with `as any`; instead refactor types or narrow data.

## Angular Best Practices (Angular AI guidelines)

- Use standalone components; do **not** set `standalone: true` manually in Angular v20+.
- Stick to the `input()` and `output()` functions rather than decorators.
- Always set `changeDetection: ChangeDetectionStrategy.OnPush`.
- Manage state with signals; derive values with `computed()` and update with `set`/`update`, never `mutate`.
- Keep components small and focused; push logic into signals or services when it grows.
- Use `host` metadata instead of `@HostBinding`/`@HostListener`.
- Lazy-load feature routes by default and favor route-level code splitting.
- Prefer inline templates for simple UI; when using external files, keep paths relative to the component file.
- Replace legacy structural directives with built-in control flow (`@if`, `@for`, `@switch`).
- Never place arrow functions or complex expressions in templates; precompute via signals.
- Use `NgOptimizedImage` for all static images (not for inline base64).
- Avoid `ngClass`/`ngStyle`; rely on `[class.foo]`/`[style.bar.px]` bindings.
- Inject services with `inject()`; services should favor `providedIn: 'root'` unless scoped explicitly.

## Accessibility and UX

- Output must pass AXE checks and meet WCAG 2.1 AA (focus management, contrast, ARIA).
- Provide keyboard support for interactive widgets and describe focus order.
- Use semantic HTML elements first; add ARIA only when necessary.

## Template & Styling Expectations

- Keep templates presentation-focused; move imperative logic into TypeScript.
- Use strongly typed `@for (item of items; track item.id)` blocks.
- Avoid relying on global date/locale state; inject services instead.
- Prefer CSS utility classes defined in `src/styles.css` or feature styles over inline styles.

## Testing & Verification

- Write new unit tests alongside complex logic (Jasmine/Karma setup in repo).
- Mention how to validate with `npm run test` or relevant Angular CLI commands.

## Resources

- Angular AI guidelines (summary of https://next.angular.dev/ai/develop-with-ai).
- Angular best-practices reference (https://next.angular.dev/assets/context/best-practices.md).

## Repo Workflow

- See `.github/ng.instructions.md` for Croniq.Ui-specific workflow rules (MCP server, preferred signal patterns, etc.).

Use this file as the authoritative context when generating or reviewing code for Croniq UI.

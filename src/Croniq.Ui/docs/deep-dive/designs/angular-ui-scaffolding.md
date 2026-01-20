# Croniq UI Scaffolding & Auth Plan

Guidance for setting up the Angular 21 workspace (`src/Croniq.Ui`) plus initial auth plumbing and MCP helper tasks.

Status: the workspace is already scaffolded. Treat this document as a plan/historical reference; prefer `README.md` for day-to-day commands.

## Workspace Creation

1. Scaffold the workspace inside `src/Croniq.Ui` (only if you are re-creating from scratch):
   ```bash
   cd src
   ng new Croniq.Ui \
     --standalone \
     --routing true \
     --style css \
     --ssr false \
     --skip-git true \
     --package-manager npm
   ```
2. Remove the default `app.component.*` content and replace it with the global shell skeleton described in `angular-ui-wireframes.md`.
3. Add secondary libraries:
   ```bash
   cd Croniq.Ui
   ng generate library data-access
   ng generate library telemetry
   ng generate library ui-kit
   ```
4. Verify the directory structure after scaffolding (Angular CLI 17+/21 uses `src/` for the main app and `projects/` for libraries):
   ```
   src/
     Croniq.Ui/
       angular.json
       package.json
       tsconfig.json
       tailwind.config.js
       src/
         app/
           core/
           shared/
           shell/
           features/
         styles.css
         main.ts
       projects/
         data-access/
           src/lib/
         telemetry/
           src/lib/
         ui-kit/
           src/lib/
       public/
         assets/
           croniq-config.json
       .vscode/
   ```
   Commit this baseline before layering feature work so diffs stay reviewable.

- `data-access`: API client plumbing + endpoint executor.
- `telemetry`: OpenTelemetry bridge + logging helpers.
- `ui-kit`: Tailwind-based headless components (tokens live here).

- Seed `tokens.css` with the semantic variables:

  ```css
  :root[data-theme='ops-light'] {
    --cq-surface: 248 250 252;
    --cq-surface-alt: 238 242 247;
    --cq-border: 203 210 223;
    --cq-text: 26 31 43;
    --cq-text-muted: 75 85 104;
    --cq-accent: 0 177 210;
    --cq-danger: 240 82 82;
    --cq-warning: 244 183 64;
    --cq-success: 31 173 102;
  }

  :root[data-theme='ops-dark'] {
    --cq-surface: 15 23 42;
    --cq-surface-alt: 30 42 63;
    --cq-border: 39 52 77;
    --cq-text: 248 250 252;
    --cq-text-muted: 148 163 184;
    --cq-accent: 39 210 255;
    --cq-danger: 251 113 129;
    --cq-warning: 250 204 21;
    --cq-success: 52 211 153;
  }
  ```

  If you use this RGB-triplet token format, Tailwind can apply alpha via `rgb(var(--token) / <alpha>)`.

- Tailwind is already installed; config lives in `tailwind.config.js` and tokens currently live in `src/styles.css`.

## MCP Helper Tasks

- Configure `.vscode/mcp.json` with the VS Code-specific `servers` property (see https://angular.dev/ai/mcp). We run MCP via the repo-local CLI (`npm run mcp`) to keep versions consistent.
  ```json
  {
    "servers": {
      "angular-cli": {
        "command": "npm",
        "args": ["run", "mcp"]
      }
    }
  }
  ```
- Add a persistent task so anyone can start the server from VS Code without retyping the command:
  ```json
  {
    "label": "Angular MCP Server",
    "type": "shell",
    "command": "npm",
    "args": ["run", "mcp"],
    "isBackground": true,
    "options": { "cwd": "${workspaceFolder}" }
  }
  ```
- Wire the npm script that proxies to `ng mcp`:
  ```json
  {
    "scripts": {
      "mcp": "ng mcp"
    }
  }
  ```
- When prompting GPT agents, attach Angular's best-practices context from [https://next.angular.dev/assets/context/best-practices.md](https://next.angular.dev/assets/context/best-practices.md) alongside `AGENTS.md` so generated code honors the "Develop with AI" guidance.

Example shell skeleton (Angular standalone component):

```ts
@Component({
  selector: 'cq-shell',
  standalone: true,
  templateUrl: './shell.component.html',
  styleUrls: ['./shell.component.css'],
})
export class ShellComponent {
  readonly tenant$ = inject(TenantContextService).tenant$;
  readonly statusCards = signal(defaultStatusCards);

  openCommandPalette(): void {
    this.commandPalette.open();
    this.telemetry.track('command_palette_opened');
  }
}
```

Note: this repo uses runtime config via `public/assets/croniq-config.json` instead of Angular environments.

## Interim Auth Implementation

- `AuthSessionService` (`src/app/core/auth/auth-session.service.ts`) now owns the opaque Croniq session token. The value lives exclusively in `sessionStorage`, and the service auto-purges expired entries so nothing lingers between tabs.
- `TenantContext` exposes an input form for the token plus a login-bootstrap stub button. The UI masks the stored value (last four characters only) so operators can confirm which secret is active without leaking the full string.
- `EndpointExecutor` receives a credential supplier instance, allowing it to inject the `Authorization` bearer token on every API call. Feature modules can still override the value per request when needed via `CroniqRequestOptions`.
- Details, guardrails, and future steps are tracked in `docs/deep-dive/auth.md`.

## Command Palette & Shell

- Shell lives under `src/app/shell/` and shared primitives under `src/app/shared/`.
- Use the tokens from `angular-ui-theme.md` for spacing/typography.
- Ensure the command palette can be invoked via `Ctrl/Cmd+K` and log telemetry events for each command selection.

## Verification Checklist

- [ ] `npm run test:once` passes.
- [ ] `npm run build` succeeds.
- [ ] Runtime config loads from `public/assets/croniq-config.json` (optional) and falls back to sane defaults.

Optional future checklist (only when implemented): ESLint/template lint, Storybook, Playwright.

## Next Steps

1. Implement the command rail + tenant selector per the wireframes.
2. Decide on the data fetching/caching abstraction (keep it consistent; avoid half-migrations).
3. Flesh out the MCP recipes (component scaffolding, tests) so GPT agents can help build features while respecting repo guardrails.

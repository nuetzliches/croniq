# Croniq UI Scaffolding & Auth Plan

Guidance for setting up the Angular 21 workspace (`src/Croniq.Ui`) plus initial auth plumbing and MCP helper tasks.

## Workspace Creation

1. Install the Angular CLI 21 preview (requires Node 20+):
   ```bash
   npm install -g @angular/cli@next
   ```
2. Scaffold the workspace inside `src/Croniq.Ui`:
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
3. Remove the default `app.component.*` content and replace it with the global shell skeleton described in `angular-ui-wireframes.md`.
4. Add secondary libraries:
   ```bash
   cd Croniq.Ui
   ng generate library data-access
   ng generate library telemetry
   ng generate library ui-kit
   ```
5. Verify the directory structure after scaffolding:
   ```
   src/
     Croniq.Ui/
       angular.json
       package.json
       tailwind.config.ts
       apps/
         admin/
           src/app/
             shell/
             core/
             shared/
             features/
           environments/
       libs/
         data-access/
         telemetry/
         ui-kit/
   ```
   Commit this baseline before layering feature work so diffs stay reviewable.
   - `data-access`: API clients + Angular Query setup.
   - `telemetry`: OpenTelemetry bridge + logging helpers.
   - `ui-kit`: Tailwind-based headless components.

- Seed `tokens.css` with the semantic variables:

  ```css
  :root[data-theme="ops-light"] {
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

  :root[data-theme="ops-dark"] {
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

  Tailwind pulls these via `rgb(var(--cq-accent) / <alpha-value>)`.

- Install Tailwind per Angular docs: `ng add @angular-devkit/schematics-cli && npm install -D tailwindcss postcss autoprefixer`.
- Create `tailwind.config.ts` using the token blueprint from `angular-ui-theme.md` and output CSS variables in `libs/ui-kit/src/lib/tokens.css`.

## MCP Helper Tasks

- Create `.vscode/tasks.json` entry:
  ```json
  {
    "label": "Angular MCP Server",
    "type": "shell",
    "options": { "cwd": "${workspaceFolder}/src/Croniq.Ui" }
  }
  ```
- Add `scripts` entry in `package.json`:
  ```json

  ```

Example shell skeleton (Angular standalone component):

```ts
@Component({
  selector: "cq-shell",
  standalone: true,
  templateUrl: "./shell.component.html",
  styleUrls: ["./shell.component.css"],
})
export class ShellComponent {
  readonly tenant$ = inject(TenantContextService).tenant$;
  readonly statusCards = signal(defaultStatusCards);

  openCommandPalette(): void {
    this.commandPalette.open();
    this.telemetry.track("command_palette_opened");
  }
}
```

````
- Document the workflow in this file and in `CONTRIBUTING.md` so contributors know how to start the server.

```ts
export const environment = {
  production: false,
  auth: {
    authority: 'https://devstack.identity',
    clientId: 'croniq-ui',
    redirectUri: 'http://localhost:4200/callback',
    mockPrincipal: {
      id: 'tenant-admin',
      roles: ['Admin'],
    },
  },
};
````

4. Build an `AuthGuard` that blocks routes until the OIDC handshake completes; include a developer override for local mocks.

## Command Palette & Shell

- Scaffold `apps/admin/src/app/shell` containing the nav rail, status strip, and command palette placeholder.
- Use the tokens from `angular-ui-theme.md` for spacing/typography.
- Ensure the command palette can be invoked via `Ctrl/Cmd+K` and log telemetry events for each command selection.

## Verification Checklist

- [ ] `npm run lint` succeeds (ESLint + Angular template lint).
- [ ] `npm run test` runs default Karma/Vitest suite (decide on Vitest before merging).
- [ ] `npm run build` produces `dist/apps/admin` artifacts referencing the Tailwind tokens.
- [ ] Storybook placeholder added for the global shell (even if not populated yet).
- [ ] OIDC stub returns a mocked principal when `environment.mock.ts` is active.

## Next Steps

1. Implement the command rail + tenant selector per the wireframes.
2. Add Angular Query provider module in `libs/data-access` with default retry/backoff.
3. Flesh out the MCP recipes (component scaffolding, tests) so GPT agents can help build features while respecting repo guardrails.

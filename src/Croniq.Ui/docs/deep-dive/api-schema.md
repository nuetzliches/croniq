# API Schema & OpenAPI Build Pipeline

This document explains how Croniq UI models backend domains, keeps the Zod types in sync with the admin experience, and rebuilds the OpenAPI document that powers mocks and diagnostics.

## 1. Source Of Truth

```
projects/api-schema/
├─ src/                # Zod schemas + exported TypeScript types
└─ openapi/            # Domain registrars that describe paths/components
```

- **Schemas** (`projects/api-schema/src/*.ts`): define everything with Zod. Export both the schema (`fooSchema`) and the inferred TypeScript type (`Foo`). These files are what the Angular app imports through the `@croniq/api-schema` path alias declared in `tsconfig.json`.
- **Registrars** (`projects/api-schema/openapi/*.ts`): each file exports `registerDomain(registry: OpenAPIRegistry)` (or a default function). Inside, reference the Zod schemas from `src/` and register the paths, components, and security requirements for that domain only. New domains only require a new file in this folder—no generator tweaks.

## 2. Modeling Workflow

1. **Create / update schemas** in `projects/api-schema/src`. Keep request/response payloads close to the upstream API surface (you can mirror `artifacts/swagger.json` for convenience).
2. **Export the pieces** via `projects/api-schema/src/index.ts` so the Angular app can import them with `@croniq/api-schema`.
3. **Add an OpenAPI registrar**:
   - Copy `projects/api-schema/openapi/schedules.ts` as a template.
   - Import the Zod schemas you need.
   - Register any shared components first (`registry.register('Foo', fooSchema)`), then call `registry.registerPath({ … })` for each HTTP method you want included.
   - If the domain introduces auth, also register the security scheme (see `openapi/security.ts`).

> Tip: guard each registrar to a single bounded context (Health, Jobs, Tenant Webhooks, etc.) so diffs stay readable.

## 3. Building / Rebuilding the Spec

Use the provided npm scripts from the workspace root:

```bash
npm run generate:openapi   # direct script
npm run generate:api       # alias that runs the same command
```

Under the hood `tools/generate-openapi.ts`:

1. Extends Zod with OpenAPI helpers.
2. Loads every `projects/api-schema/openapi/*.ts` file (alphabetically) and invokes its `registerDomain` export.
3. Generates a 3.1 document with `OpenApiGeneratorV31`.
4. Writes `public/swagger.json`.

### Outputs

- `public/swagger.json`: shipped with the app (and viewable via `http://localhost:4200/swagger.json` while the dev server runs).
- Anything under `artifacts/`: reference specs from live environments that you can diff against when updating schemas.

### When To Rebuild

- After changing any Zod schema (`projects/api-schema/src/*`).
- After adding/removing a registrar (`projects/api-schema/openapi/*`).
- Before committing if backend endpoints changed (compare with `artifacts/swagger.json`).

## 4. Keeping The App Wired

- `provideCroniqApiClient` (see `projects/data-access/src/lib/api-client.ts`) depends on these schemas to validate HTTP responses. If you add new REST calls, update the Zod schema first, regenerate OpenAPI, then consume the new types from `@croniq/api-schema` inside the Angular data stores.
- The default base URL comes from `src/app/core/api-config.ts`. Override it via `NG_APP_API_BASE_URL` when pointing the UI at another environment.

Following this loop keeps local models, generated documentation, and runtime validation in sync without relying on third-party codegen.

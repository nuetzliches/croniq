# API Schema & OpenAPI Sync

This document now tracks how we translate the upstream Croniq OpenAPI description into the Zod models that power the UI. The previous “Zod → OpenAPI” generator has been removed because nothing inside the app consumed the generated `public/swagger.json` artifact.

## 1. Source Material

```
projects/api-schema/
├─ generated/          # auto-generated zod types (openapi-zod-client)
└─ src/                # manual overrides + re-exports

artifacts/
└─ swagger.json        # Snapshot pulled from Croniq.Api (upstream contract)
```

- **OpenAPI snapshots** live under `artifacts/` and remain the canonical description of Croniq.Api.
- **Generated Zod schemas** reside in `projects/api-schema/generated` and are overwritten by `npm run generate:api`.
- **Manual Zod helpers** stay in `projects/api-schema/src`. The entrypoint re-exports both the generated output and any handwritten shapes we still need.
- **Tenant presets**: previously planned preset seeds were removed. Tenant context is currently operator-controlled and/or API-backed.

## 2. Current Workflow

1. Refresh the upstream spec (export from Croniq.Api, drop into `artifacts/swagger.json`).
2. Run `npm run generate:api`. The generator resolves the OpenAPI document in this order:
   - `CRONIQ_OPENAPI_URL` environment variable (when set)
   - Local snapshot at `artifacts/swagger.json`

- Fallback URL `http://localhost:5080/swagger/v1/swagger.json`
  This allows schema generation to work offline as long as the snapshot exists.

3. Add or update any manual helpers in `projects/api-schema/src` and re-export everything via `src/index.ts` for the Angular app.

There is still no runtime dependency on the OpenAPI document—`provideCroniqApiClient` validates against the Zod bundles that ship with the UI.

### Generator implementation

- Entry point: `tools/generate-schemas.ts`
- Tooling: `openapi-zod-client` + custom Handlebars templates under `tools/templates/`
- Output:
  - `projects/api-schema/generated/schemas.ts`
  - `projects/api-schema/generated/endpoints/*.ts` (split by domain)

Do not edit files under `projects/api-schema/generated/` manually; change the templates or the generator and re-run.

### Dev vs. CI

- Local dev (live swagger): `npm run generate:api:server`
- CI / deterministic builds: prefer committing `artifacts/swagger.json` and using `npm run generate:api` so generation does not depend on a reachable backend.

## 3. Why The Old Generator Was Removed

- The `projects/api-schema/openapi` registrars duplicated every schema we already modeled in Zod.
- `npm run generate:openapi` only produced `public/swagger.json`; no integration (Storybook, docs site, or backend tooling) consumed that file.
- Maintaining both directions introduced drift every time a type changed, so we removed the registrars, generator script, and dependency on `@asteasolutions/zod-to-openapi`.

## 4. Next Steps: OpenAPI -> Zod Automation

We still want automated Zod generation from the upstream OpenAPI spec. Outstanding work:

1. Track gaps where `openapi-zod-client` output still has to be patched manually (e.g., discriminated unions, enums needing stricter typing).
2. ✅ **Done:** the generator now emits one `schemas.ts` plus an `endpoints/` folder split by primary path segment so each domain gets its own strongly typed collection.
3. Wire CI so that `npm run generate:api` runs and fails on diffs whenever the upstream spec changes.
4. Decide whether we also want to generate typed API clients alongside the schemas (left disabled for now).

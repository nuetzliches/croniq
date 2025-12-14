# Generated API Schema Assets

Artifacts under this folder are produced by `npm run generate:api`.

- `schemas.ts`
  - Exports the canonical Zod models plus shared endpoint typings.
  - Every schema is exported individually **and** via the `schemas` map for ergonomic imports.
- `endpoints/`
  - Contains one TypeScript module per primary API domain (derived from the first path segment).
  - Each module exports a strongly typed `*Api` collection that can be wired into Angular `HttpClient` services.
  - `index.ts` re-exports every domain module for convenience.

⚠️ Do not edit these files manually. Re-run `npm run generate:api` whenever the upstream OpenAPI contract changes.

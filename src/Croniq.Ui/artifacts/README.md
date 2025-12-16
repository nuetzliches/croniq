# Croniq API Snapshot

This directory holds the most recent OpenAPI document exported from Croniq.Api. The generator at `npm run generate:api` resolves its source in the following order:

1. `CRONIQ_OPENAPI_URL` environment variable (if set)
2. `artifacts/swagger.json` (this snapshot)
3. `http://localhost:5000/swagger/v1/swagger.json`

Update `swagger.json` whenever the backend contract changes so contributors can regenerate the Zod schemas without a running API.

## Updating the snapshot

- One-shot update from the local dev backend (recommended): `npm run generate:api:server:snapshot`
- Or pass the flag manually: `tsx tools/generate-schemas.ts --update-snapshot`

You can override the OpenAPI URL via `CRONIQ_OPENAPI_URL` and the output location via `--snapshot-path`.

# CHECKLIST-DX-UX

Developer and user experience polish checklist for Croniq.

## Developer experience
- [ ] Verify devstack commands are one-liners with clear errors.
- [ ] Ensure `.env` and `.env.example` match compose and docs.
- [ ] Validate Node/.NET prerequisites in docs and tooling.
- [ ] Add concise first-run steps for API/worker/UI.
- [ ] Keep samples consistent with environment variables and scopes.
- [ ] Ensure tests are discoverable and fast.

## UI and product UX
- [ ] Review critical UI flows (login, schedules, triggers, webhooks).
- [ ] Check loading, error, and empty states.
- [ ] Verify accessibility (keyboard navigation, ARIA, contrast).
- [ ] Align copy with API behavior and docs.

## Tooling
- [ ] Check CLI/help text clarity.
- [ ] Add scripts for common tasks (snapshot, OpenAPI, lint, test).
- [ ] Document troubleshooting for common failures (auth, DB, npm).

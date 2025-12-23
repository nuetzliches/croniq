# Guidance for Copilot, Codex & Other AI Assistants

All AI-generated contributions must align with the architectural ground rules documented in `docs/deep-dive/architecture.md`. When in doubt, start there.

## Core Expectations

1. **Target Stack**

   - Use .NET `net10.0` for all new projects/libraries; upgrade existing TFMs to `net10.0` unless explicitly exempted.
   - Prefer modern C# design patterns (records, required members, dependency injection, source generators where appropriate).

2. **Documentation & Comments**

   - Write documentation, commit messages, and code comments in **English**.
   - Prioritize concise summaries plus context for complex logic.

3. **Dependencies & Packages**

   - Add only dependencies with MIT-compatible licenses (MIT, Apache 2.0, BSD); flag any package that introduces stronger restrictions before it lands.
   - Add NuGet packages in their latest stable version unless the concept mandates a specific range.
   - Follow each package's official documentation for configuration and usage patterns.

4. **General Best Practices**

   - Keep public APIs minimal but well-documented; avoid leaking implementation details.
   - Favor async APIs (`Task`/`ValueTask`) and cancellation tokens for long-running or I/O-bound operations.
   - Ensure telemetry hooks (logging, tracing, metrics) align with the OpenTelemetry-first approach captured in `docs/deep-dive/architecture.md`.
   - Validate input aggressively; prefer `ArgumentException`/`Guard` helpers over silent failures.
   - Write unit tests (xUnit + Shouldly) for new logic; update integration tests when touching provider or persistence layers.
   - Keep secrets/config values outside source control; rely on the `ISecretProvider` abstractions instead of inline secrets.

5. **Auth Direction (V1)**

   - The initial target is **self-hosted, private-network, single-tenant** deployments.
   - For password auth, require explicit `tenantId` (no `DefaultTenant`/tenant reference fallback).
   - Do not introduce new external identity provider flows or user/tenant membership models unless explicitly requested; federated login is deferred to later versions (see `CLOUD-CONCEPT.md` / `CLOUD-REPO-SPLIT.md`).

6. **Breaking Changes Before GA**

   - There are currently no external consumers. Treat breaking API or contract changes as acceptable until we ship `v1.0.0` (non-RC).
   - When making such changes, still document the rationale in `docs/deep-dive/*` so we keep a trace for future stabilization.

7. **Documentation Cross-Links**

   - When new features impact consumers, update `docs/*` and refer to deeper explanations in `docs/deep-dive/*`.
   - Record noteworthy architectural decisions in the technical docs (especially `docs/deep-dive/architecture.md`).

8. **Angular UI (Croniq.Ui) Workflow**

   - See `src/Croniq.Ui/.github/ng.instructions.md` (and `src/Croniq.Ui/.github/copilot.instructions.md`) for the authoritative Angular UI instructions.

By following these instructions, AI contributions remain compliant with the project's technical vision and developer experience goals.

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

   - Add NuGet packages in their latest stable version unless the concept mandates a specific range.
   - Follow each package's official documentation for configuration and usage patterns.

4. **General Best Practices**

   - Keep public APIs minimal but well-documented; avoid leaking implementation details.
   - Favor async APIs (`Task`/`ValueTask`) and cancellation tokens for long-running or I/O-bound operations.
   - Ensure telemetry hooks (logging, tracing, metrics) align with the OpenTelemetry-first approach captured in `docs/deep-dive/architecture.md`.
   - Validate input aggressively; prefer `ArgumentException`/`Guard` helpers over silent failures.
   - Write unit tests (xUnit + FluentAssertions) for new logic; update integration tests when touching provider or persistence layers.
   - Keep secrets/config values outside source control; rely on the `ISecretProvider` abstractions instead of inline secrets.

5. **Documentation Cross-Links**
   - When new features impact consumers, update `docs/*` and refer to deeper explanations in `docs/deep-dive/*`.
   - Record noteworthy architectural decisions in the technical docs (especially `docs/deep-dive/architecture.md`).

By following these instructions, AI contributions remain compliant with the project's technical vision and developer experience goals.

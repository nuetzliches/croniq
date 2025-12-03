# Croniq Technical Documentation

This section is intended for maintainers, platform engineers, and contributors working on Croniq itself.

## Contents (Planned)

- Architecture deep dive (extends `CONCEPT.md`)
- Persistence model & Xtraq schema references
- Provider extension guides (logging, telemetry, secrets, etc.)
- Deployment playbooks (Docker Compose, Kubernetes, CI/CD)
- Observability standards (Serilog + OpenTelemetry)
- Release, compliance, and security checklists

## Authoring Guidelines

- Keep explanations in English and reference specific sections of `CONCEPT.md` whenever possible.
- Provide diagrams or sequence sketches when describing execution flows, clustering, or recovery logic.
- Cross-link consumer-facing docs when relevant (e.g., "see `../consumer/quickstart.md` for the client perspective").

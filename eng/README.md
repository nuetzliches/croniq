# Croniq Engineering Assets

This folder stores reusable pipeline assets referenced across CI workflows.

- `eng/pipelines/` – documentation for GitHub Action environments and required secrets.
- `eng/pipelines/secrets.template.md` – copy/paste template for internal runbooks.

When new workflows are added, update this folder instead of scattering ad-hoc notes in PR descriptions.

## Coding Conventions

### Global usings & warnings

- Projects rely on `<ImplicitUsings>enable</ImplicitUsings>` from `Directory.Build.props`; avoid repo-wide `GlobalUsings.cs` files unless a project repeatedly duplicates the same `using` directives.
- When a project genuinely benefits from shared directives, add a local `GlobalUsings.cs` next to the `.csproj` and keep its surface limited to framework/common Croniq namespaces.
- `<TreatWarningsAsErrors>true</TreatWarningsAsErrors>` already enforces a zero-warning policy—run `dotnet build` (or `dotnet test`) before committing so new analyzers or nullable warnings cannot slip in.

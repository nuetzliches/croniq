# CI/CD Secrets & Environments

Use this template to provision GitHub Actions secrets/environments. Keep the filled-in version in an internal runbook (not committed).

| Workflow | Environment | Secret / Variable | Purpose |
| --- | --- | --- | --- |
| `ci-pr.yml` | n/a | *(none mandatory)* | Uses repository-scoped permissions only. Optional: `CODECOV_TOKEN` once coverage uploads are enabled. |
| `nightly.yml` | `nightly` (optional) | `GITHUB_TOKEN` (default) | Access to repo + packages. |
| `release.yml` | `release` | `NUGET_API_KEY` | Publishes packages to NuGet.org. |
| | | `NUGET_SIGNING_CERT_BASE64` / `NUGET_SIGNING_CERT_PASSWORD` | Signs `.nupkg` artifacts when available. |
| | | `COSIGN_KEY`, `COSIGN_PASSWORD` | Signs GHCR images. |
| | | `STAGING_KUBECONFIG` | Reused when release triggers staging deploy via workflow_call. |
| `deploy-staging.yml` | `staging` (requires reviewers) | `STAGING_KUBECONFIG` | kubeconfig for staging cluster (base64-encoded). |

## Provisioning Notes

1. Encode kubeconfigs/secrets using `base64` to avoid encoding issues; the workflow decodes them at runtime.
2. Restrict each environment (`release`, `staging`) with required reviewers so only trusted maintainers can dispatch workflows.
3. Rotate credentials periodically and document expiration dates in the internal runbook referencing this template.

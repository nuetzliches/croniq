# Signing Materials

Store public verification artifacts for releases in this directory:

- `cosign.pub`: Public key for container image signatures (used by `cosign verify`).
- `nuget-signing.cer`: Exported public certificate for NuGet package signatures.

Operational notes:

- Keep private keys out of the repo; release workflow expects secrets `COSIGN_KEY`, `NUGET_SIGNING_CERT_BASE64`, and `NUGET_SIGNING_CERT_PASSWORD`.
- When keys rotate, replace the public artifacts here and update fingerprints referenced in `docs/SECURITY.md`. Current NuGet thumbprint: `64FAE63096D184E8C4E8710A59175F3D734FCBB0`.
- Consumers verify artifacts via `docs/deep-dive/release-verification.md`.***

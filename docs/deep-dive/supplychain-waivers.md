# Vulnerability Waiver Process

Use this document to track temporary waivers when a vulnerability cannot be remediated immediately. Waivers are exceptions, not the norm, and must have a near-term expiry.

## Policy

- Scope: Only HIGH/CRITICAL findings that block CI/release and have no immediate patch or acceptable mitigation.
- Duration: Maximum 30 days unless explicitly extended by security review.
- Evidence: Include mitigation/compensating controls and a link to the upstream issue or vendor advisory.
- Ownership: Each waiver has a single DRI; expired waivers automatically fail the build until removed or renewed.

## Template

Add entries to the table below instead of editing historical rows. Keep newest waivers at the top.

| ID | Package/Image | Version/Digest | CVE/Advisory | Severity | Expires (UTC) | Owner | Mitigation / Justification | Tracking Issue |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| WAIVER-YYYYMMDD-01 | e.g. `Croniq.Api image` | `ghcr.io/<owner>/croniq-api@sha256:...` | CVE-2025-12345 | HIGH | 2026-01-15 | @owner | Pending upstream patch; blocked behind mTLS + WAF rule | tracking issue: <org>/croniq#123 |

## How to apply a waiver

1. Create an entry in the table with expiry and mitigation.
2. Reference the waiver ID in the CI/release job that is being suppressed (e.g., `TRIVY_IGNORE_UNFIXED` list or custom ignore file) and link back to this document.
3. Open/associate a tracking issue for remediation before expiry.
4. Remove the waiver once the dependency/image is patched; do not extend expiry without security review.***

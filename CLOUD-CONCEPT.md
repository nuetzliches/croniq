# Cloud Concept (Future Ideas)

This document is intentionally **forward-looking** and can be deleted later without affecting the self-hosted V1 documentation.

## Why this file exists

Croniq V1 is currently optimized for **self-hosted, private-network, single-tenant** deployments.

Anything that is primarily driven by a hosted roadmap is captured here so the rest of the docs can stay focused.

If/when we decide to split hosted work into a separate repository, see `CLOUD-REPO-SPLIT.md`.

## Terminology

- **Multi-tenant**: a single control-plane serves multiple isolated tenants (data, quotas, authZ boundaries).
- **OIDC**: OpenID Connect / OAuth2-style interactive user login via an external identity provider (IdP) with bearer tokens.
- **SaaS**: Croniq offered as a hosted service (operated by us), typically multi-tenant and externally reachable.

## Future directions (non-V1)

### Identity & access

- Add a first-class **OIDC** story for human operators (interactive login) and support standardized flows (e.g., PKCE), plus claim mapping for tenant/environment/scopes.
- Decide whether to model user↔tenant membership as:
  - purely externalized in the IdP (claims-only), or
  - persisted in Croniq (roles/groups, invitations), or
  - hybrid.

### Hosted operations

- Hardening for **SaaS** scale: multi-region, HA, autoscaling worker fleets, tenant-level rate limits/quotas, incident response runbooks.
- Cost controls and noisy-neighbor defenses: storage tiering, per-tenant retention and metering.

### Data & storage

- Remote persistence options for **SaaS** volumes (e.g., object storage for logs/attachments) and lifecycle rules.
- Migration strategy and versioned schemas for always-on upgrades.

### Tenancy model

- Formalize **multi-tenant** onboarding and isolation guarantees (partition keys, encryption boundaries, audit).
- Tenant impersonation and support tooling (break-glass access, approval workflows).

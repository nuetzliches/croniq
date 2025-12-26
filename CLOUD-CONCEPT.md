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

#### Bearer token validation vs federated/OIDC login

These are related, but not the same problem:

- **Bearer token validation** (resource-server concern): how `Croniq.Api` decides whether an incoming `Authorization: Bearer ...` token is trusted and how it maps claims to Croniq's caller context.
- **Federated/OIDC login** (interactive auth concern): how a human operator obtains a bearer token in the first place (redirect-based login, PKCE, sessions, logout, etc.).

In other words: OIDC is one way to _get_ a bearer token; bearer validation is the server-side mechanism to _accept_ a bearer token.

**Typical validation building blocks**

- Signature verification (HMAC or asymmetric keys via JWKS)
- Issuer (`iss`) and audience (`aud`) checks
- Lifetime checks (`exp`, `nbf`)
- Claim mapping to Croniq semantics (`tenant`, `env`, `scope`, `sub`)

**Why this matters for repo boundaries**

- Self-hosted V1 can keep a lightweight story: Croniq-issued tokens and/or password login, with local validation.
- A hosted/cloud story usually wants: tenant-aware issuer/authority management, federated login UX, token exchange patterns, and stronger audit/compliance requirements.

This is a good candidate for a separate repo/module when it starts to drive dependency and doc churn (see `CLOUD-REPO-SPLIT.md`).

### Hosted operations

- Hardening for **SaaS** scale: multi-region, HA, autoscaling worker fleets, tenant-level rate limits/quotas, incident response runbooks.
- Cost controls and noisy-neighbor defenses: storage tiering, per-tenant retention and metering.

### Data & storage

- Remote persistence options for **SaaS** volumes (e.g., object storage for logs/attachments) and lifecycle rules.
- Migration strategy and versioned schemas for always-on upgrades.

### Deferred: Remote Persistence (Hosted)

- [ ] Architekturskizze `Croniq.Persistence.Remote` (Client) + `Croniq.Persistence.Remote.Service` (Service-Seite): Transport, Auth (ApiKey/Bearer), Throttling, Tenant-Isolation.
- [ ] Evaluieren, ob vorhandene `Croniq.Api`-Endpoints erweitert werden oder ein separates Service-Repo nötig ist; Migrationsplan dokumentieren.
- [ ] Sicherheits-/Governance-Aspekte festhalten (Tenant-Isolation, SLAs, Secrets, Observability).
- [ ] Betriebs- und Provisionierungs-Runbook (Deploy-Topologie, Monitoring, Kostenkontrolle).

### Tenancy model

- Formalize **multi-tenant** onboarding and isolation guarantees (partition keys, encryption boundaries, audit).
- Tenant impersonation and support tooling (break-glass access, approval workflows).

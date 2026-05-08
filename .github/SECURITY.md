# Security Policy

Croniq is pre-1.0 software, but we take security issues seriously. Please **do not** open a public GitHub issue or pull request for a suspected vulnerability — that exposes users before a fix is available.

## Reporting a Vulnerability

### Preferred: GitHub Private Vulnerability Reporting

Use the **"Report a vulnerability"** button on the [Security tab](https://github.com/nuetzliches/croniq/security), or open [security/advisories/new](https://github.com/nuetzliches/croniq/security/advisories/new) directly. This creates a private advisory we can triage, patch, and publish a GHSA from once the fix ships.

If the button is missing, the maintainers may not have enabled Private Vulnerability Reporting yet — please fall back to email.

### Fallback: Email

Contact a maintainer privately via the email shown on their GitHub profile (commit authors on `main` are a reliable starting point).

### What to include

- A clear description of the issue and affected component (which crate, endpoint, or transport).
- Croniq version (`croniq-server --version`, or the `version` field of `Cargo.toml`).
- Steps to reproduce — PoC code, request transcript, or minimal `Croniqfile` are ideal.
- Impact you have observed or believe is achievable.

### What to expect

- **Initial acknowledgement:** within 5 business days.
- **Triage and severity assessment:** within 10 business days.
- **Fix and coordinated disclosure:** timeline depends on severity, complexity, and any upstream dependencies. We will keep you informed and credit you in the published advisory unless you prefer to remain anonymous.

We do not currently run a bug bounty program.

## Supported Versions

Croniq is in active pre-1.0 development. Security fixes are backported only to the **latest minor release line**. Older minors are considered unsupported — please upgrade.

| Version | Status         |
| ------- | -------------- |
| 0.10.x  | ✅ Supported   |
| < 0.10  | ❌ Unsupported |

## Published Advisories

Released advisories are listed under [Security advisories](https://github.com/nuetzliches/croniq/security/advisories).

# Security Policy

Microsummary: How to report vulnerabilities, our disclosure window, and supported versions. Stable, updated as needed.

- Report privately to security@t3hw00t.dev or via GitHub Security Advisories.
- Please do not open public issues for suspected vulnerabilities.
- We aim to acknowledge within 2 business days and provide a fix or mitigation within 30 days when feasible.
- Supported versions: the latest minor release. Older prereleases receive best‑effort fixes only.

Scope includes the service (`arw-server`), CLI, schemas, and docs. If your report involves third-party dependencies, include details so we can coordinate upstream.

Thank you for helping keep ARW users safe.

Notes for operators:
- Debug mode (`ARW_DEBUG=1`) exposes admin UIs but is now enforced as loopback‑only. Use an admin token for any remote access even in debug.
- A default Content Security Policy (CSP) is applied to HTML responses. When `ARW_CSP_PRESET=strict`, non‑debug pages receive a strict CSP with nonces; debug pages keep a relaxed CSP to avoid breaking inline tooling during development.

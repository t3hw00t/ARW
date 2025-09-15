---
title: Threat Model
---

# Threat Model
Updated: 2025-09-12
Type: How‑to

Risks
- Prompt injection, tool SSRF, model/file tampering, plugin malware.

Controls
- Policy‑backed egress firewall (host‑local proxy + DNS guard) with network allowlists; strict MIME/type checks on downloads; signature verification for models/plugins; content‑policy guards on tool outputs before memory/actions.

Supply chain
- SBOMs and artifact signing/verification where applicable; trust stores for plugin/tool signatures.

See also: Security Hardening, Plugin ABI & Trust, Data Governance.
Related: Egress Firewall, Network Posture.

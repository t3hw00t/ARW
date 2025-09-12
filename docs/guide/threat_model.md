---
title: Threat Model
---

# Threat Model & Supply‑Chain Security

Risks
- Prompt injection, tool SSRF, model/file tampering, plugin malware.

Controls
- Network allowlists; strict MIME/type checks on downloads; signature verification for models/plugins; content‑policy guards on tool outputs before memory/actions.

Supply chain
- SBOMs and artifact signing/verification where applicable; trust stores for plugin/tool signatures.

See also: Security Hardening, Plugin ABI & Trust, Data Governance.


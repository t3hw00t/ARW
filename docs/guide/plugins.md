---
title: Plugins & Extensions
---

# Plugins & Extensions
Updated: 2025-09-15
Type: How‑to

Plugins install tools and optional UI contributions (panels/commands). ARW’s tool registration generates JSON Schemas, so most UIs can be auto‑built as forms.

Tool UX
- Auto‑form from schema; client‑side validation; intent preview
- Results stream as events; errors inline in the sidecar

UI contributions
- Optional panels (e.g., a data explorer) and commands (palette entries)
- Contribute declaratively; use the shared event stream; no bespoke state

Security
- Capabilities are explicit (fs/net/mic/cam/gpu/sandbox scopes)
- Policies grant/deny with TTL leases; prompts render inline

Distribution
- As crates or WASI modules; once registered, they appear everywhere (policy‑gated)

See also: Policy, Recipes, Events Vocabulary.


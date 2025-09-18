---
title: Merge Notes — 2025-09-17
---

# Merge Notes — 2025-09-17

Updated: 2025-09-17
Type: Reference

This batch consolidates documentation and guidance around the unified `arw-server` surface (port 8091) while retaining clearly scoped legacy `arw-svc` notes (port 8090). It also aligns examples, auth guidance, and reverse proxy/container configs.

## Summary

- Unified defaults: ensure examples use `http://127.0.0.1:8091` unless explicitly discussing legacy.
- Auth guidance: prefer `Authorization: Bearer <token>`; legacy admin surfaces continue to support `X-ARW-Admin`.
- Reverse proxy: refreshed Caddy and Traefik examples (unified and legacy), corrected Docker label examples.
- SSE guide: consistent examples for replay, prefix filters, and legacy bridge.
- CLI and API references: harmonized spec endpoints and models read-model examples.
- Windows validation: clarified unified versus legacy launcher paths.

## Included PRs

- #66 Update reverse proxy guide for unified server and legacy bridge  
  docs/guide/reverse_proxy.md:1
- #65 Update CLI guide for new base URLs and spec endpoints  
  docs/guide/cli.md:1
- #64 Update API reference for unified server defaults  
  docs/reference/api.md:1
- #63 Update API doc for unified and legacy surfaces  
  docs/API_AND_SCHEMA.md:1
- #62 Update AI assistant index for unified server quickstart  
  docs/ai_index.md:1
- #61 docs: refresh container compatibility guidance  
  docs/guide/compatibility.md:1
- #60 docs: refresh SSE guide for unified events endpoint  
  docs/guide/events_sse.md:1
- #59 docs: clarify Projects UI legacy availability  
  docs/guide/projects_ui.md:1
- #58 docs: refresh core concepts around unified arw-server triad  
  docs/concepts.md:1
- #57 docs: update admin endpoints guide for triad surface  
  docs/guide/admin_endpoints.md:1
- #56 Update flows guide for unified server routes  
  docs/guide/flows.md:1
- #55 Document unified Windows validation flow  
  docs/developer/windows-start-validation.md:1
- #54 docs: refresh unified port references  
  docs/guide/ports.md:1
- #53 docs: refresh kernel api surface  
  docs/architecture/kernel.md:1
- #52 Add memory pointer support to context rehydrate  
  crates/arw-core:1
\- #67 Switch /spec/openapi.yaml to generated ApiDoc + update CI/scripts  
  apps/arw-server/src/api_spec.rs:1, .github/workflows/interfaces.yml:1, scripts/release.sh:1, scripts/hooks/install_hooks.sh:1, docs/reference/api.md:1

## Notes for Operators

- Unified server (8091) should run with `ARW_DEBUG=0` and a strong `ARW_ADMIN_TOKEN`. Protect `/events`, `/actions`, and `/state/*` behind your reverse proxy.
- Legacy bridge (8090) is optional and scoped for compatibility needs (classic debug UI and `/admin/*`). Do not expose unless required.

## Follow-ups

- Consider bumping/transitively removing `screenshots` crate if upstream publishes fixes for future-incompat warnings.
- Add a CI link-checker job to enforce relative link health in `docs/`.
  - Update 2025-09-18: Added `.github/workflows/link-check.yml` (lychee) to lint docs links on PRs and pushes.

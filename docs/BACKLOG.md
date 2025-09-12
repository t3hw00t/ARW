---
title: Backlog
---

# Backlog

Updated: 2025-09-12

This backlog captures concrete work items and near-term priorities. The Roadmap focuses on higher‑level themes and time horizons; see About → Roadmap for strategic context.

Status: unless noted, items are todo. Items that have task IDs link to the tracker under Developer → Tasks.

## Now (Weeks)

UI Coherence
- Universal right‑sidecar across Hub/Chat/Training; subscribe once to `/events` with `corr_id` filters
- Command Palette: global search + actions; attach agent to project; grant scoped permissions with TTL

Recipes MVP
- Finalize `spec/schemas/recipe_manifest.json`; add doc‑tests and example gallery entries
- Gallery UI: install (copy folder), inspect manifest, launch, capture Episodes
- Permission prompts (ask/allow/never) with TTL leases; audit decisions as `Policy.*` events

Security & Admin
- Admin auth hardening — hashed tokens + per‑token/IP sliding rate‑limit [t-250911230312-0863]
- Per‑route gating layers; slim global admin middleware [t-250911230252-9858]

Remote Access & TLS
- Dev TLS profiles (mkcert + self‑signed) for localhost
- Caddy production profile with Let's Encrypt (HTTP‑01/DNS‑01) for public domains
- Reverse‑proxy templates (nginx/caddy) with quick run/stop helpers
- Secrets handling: persist admin tokens only to local env files; avoid committing to configs
- Setup wizards to pick domain/email, validate DNS, and dry‑run cert issuance

Observability & Eventing
- Event journal: reader endpoint (tail N) and topic‑filtered consumers across workers/connectors
- Metrics registry with histograms; wire to /metrics [t-250911230320-8615]
- Docs: surface route metrics/events in docs and status page

State Read‑Models & Episodes
- Observations read‑model + GET /state/observations [t-250912001055-0044]
- Beliefs/Intents/Actions stores + endpoints [t-250912001100-3438]
- Episodes + Debug UI reactive views (truth window) [t-250912001105-7850]
- Debug UI: Episodes filters + details toggle [t-250912024838-4137]

Hierarchy & Governor Services
- Encapsulate hierarchy (hello/offer/accept/state/role_set) and governor (profile/hints) into typed services; endpoints prefer services; publish corr_id events; persist orchestration [t-250912024843-7597]

CLI & Introspection
- Migrate arw-cli to clap (derive, help, completions, JSON flag) [t-250911230329-4722]
- Auto‑generate /about endpoints from router/introspection [t-250911230306-7961]

Queues, NATS & Orchestration
- Orchestrator: lease handling, nack(retry_after_ms), group max in‑flight + tests [t-250911230308-0779]
- NATS: TLS/auth config and reconnect/backoff tuning; docs and examples [t-250911230316-4765]

Specs & Docs
- Generate AsyncAPI + MCP artifacts and serve under /spec/* [t-250909224102-9629]
- Docgen: gating keys listing + config schema and examples

Feedback Engine (Near‑Live)
- Engine crate and integration: actor with O(1) stats, deltas via bus, snapshot+persistence [t-250909224102-8952]
- UI: near‑live feedback in /debug showing deltas with rationale/confidence [t-250909224103-0211]
- Policy hook: shadow → policy‑gated auto‑apply with bounds/rate‑limits [t-250909224103-5251]

Testing
- End‑to‑end coverage for endpoints & gating; fixtures; CI integration [t-250911230325-2116]

## Next (1–2 Months)

Platform & Policy
- WASI plugin sandbox: capability‑based tool permissions (ties to Policy)
- Policy engine integration (Cedar bindings); per‑tool permission manifests
- RPU: trust‑store watch + stronger verification; introspection endpoint [t-250911230333-9794]

Models & Orchestration
- Model orchestration adapters (llama.cpp, ONNX Runtime) with pooling and profiles
- Capsules: record inputs/outputs/events/hints; export/import; deterministic replay

Specs & Interop
- AsyncAPI + MCP artifacts in CI; promote developer experience around /spec/*

Docs & Distribution
- Showcase readiness: polish docs, packaging, and installer paths

## Notes
- For live status of all tracked tasks, see Developer → Tasks, which renders from `.arw/tasks.json`.
- Recently shipped work is summarized under About → Roadmap → Recently Shipped.

---
title: Backlog
---

# Backlog

Updated: 2025-09-12

This backlog captures concrete work items and near-term priorities. The Roadmap focuses on higher‑level themes and time horizons; see About → Roadmap for strategic context.

Status: unless noted, items are todo. Items that have task IDs link to the tracker under Developer → Tasks.

## Now (Weeks)

UI Coherence
- Universal right‑sidecar across Hub/Chat/Training; subscribe once to `/admin/events` with `corr_id` filters
- Command Palette: global search + actions; attach agent to project; grant scoped permissions with TTL

Recipes MVP
- Finalize `spec/schemas/recipe_manifest.json`; add doc‑tests and example gallery entries
- Gallery UI: install (copy folder), inspect manifest, launch, capture Episodes
- Permission prompts (ask/allow/never) with TTL leases; audit decisions as `Policy.*` events

Logic Units (High Priority)
- Manifest and schema (`spec/schemas/logic_unit_manifest.json`); example units under `examples/logic-units/`
- Library UI: tabs (Installed/Experimental/Suggested/Archived), diff preview, apply/revert/promote
- Agent Profile slots + compatibility checks (design + stubs)
- A/B dry‑run pipeline wired to Evaluation Harness; per‑unit metrics panel
- Research Watcher: ingest feeds; draft config‑only units; Suggested tab source

Last‑mile Structures
- Config Patch Engine: dry‑run/apply/revert endpoints; schema validation; audited permission widening
- Experiment Orchestrator: start/stop/assign; emit Experiment.*; fold to `/state/experiments`
- Provenance/Snapshots: enrich `/state/episode/{id}/snapshot` with effective config (units, params, model hash, policies)
- AppSec Harness: seed tests; surface violations as `Policy.Decision` events; block unsafe tool I/O
- Observability (OTel): map timeline to traces (corr_id as trace); correlate metrics/logs
- Compliance Mode: workspace switch; record‑keeping + approvals; UI status widget
- Supply‑Chain Trust: signed manifests, SBOMs, sandbox defaults; align desktop capabilities with policies
- Scheduler/Governor: fair queues, preemption, backpressure, kill‑switch; policy‑aware

Security & Admin
- Admin auth hardening — hashed tokens + per‑token/IP sliding rate‑limit [t-250911230312-0863]
- Per‑route gating layers; slim global admin middleware [t-250911230252-9858]

Egress Firewall & Posture (Plan)
- Policy: add network scopes (domain/IP/CIDR, port, protocol) and TTL leases; surface in UI.
- Gateway: per‑node loopback proxy (HTTP(S)/WS CONNECT; optional SOCKS5) with allow/deny by SNI/Host and port; deny IP‑literals by default; no TLS MITM.
- DNS Guard: force resolver, block UDP/53/DoH/DoT from tools; short TTLs; log lookups.
- Routing: containers via proxy env + blocked direct egress; host processes with OS firewall rules (allow 127.0.0.1:proxy only for agent PIDs).
- Ledger: append‑only egress ledger with episode/project/node attribution and bytes/$ estimates; pre‑offload preview dialog in UI.
- Posture: Off/Public/Allowlist/Custom per project; default to Public.
- Filesystem & Sensors: sandbox write scopes (project://) and leased mic/cam sidecar access with TTL + audits.
- Cluster: replicate gateway+DNS guard per Worker; propagate policies from Home over mTLS; Workers cannot widen scope.

Lightweight Mitigations (Plan)
- Memory quarantine: add review queue and `Memory.Quarantined`/`Memory.Admitted` events; admit only with provenance + evidence score.
- Project isolation: enforce per‑project namespaces for caches/embeddings/indexes; “export views” only; imports read‑only and revocable.
- Belief‑graph ingest: queue world diffs; surface conflicts; require accept/apply with audit events.
- Cluster manifest pinning: define signed manifest schema; publish/verify; scheduler filters to trusted manifests.
- Secrets hygiene: vault‑only; redaction pass on snapshots/egress previews; secret‑scanner job for artifacts.
- Hardened headless browsing: disable service workers/HTTP3; same‑origin fetches; DOM‑to‑text extractor; route via proxy.
- Safe archive handling: temp jail extraction with path canonicalization; size/time caps; nested depth limit.
- DNS guard + anomaly: rate‑limit lookups; alert on high‑entropy domain bursts.
- Accelerator hygiene: zero VRAM/buffers between jobs; disable persistence mode where possible; prefer per‑job processes.
- Co‑drive role separation: roles view/suggest/drive; “drive” cannot widen scope or approve leases; tag remote actions.
- Event integrity: mTLS; nonces; monotonic sequence numbers; idempotent actions; reject out‑of‑order/duplicates.
- Context rehydration guard: redaction/classification before reuse in prompts; badge “potentially exportable”; require egress lease if offloaded.
- Operational guardrails: per‑project security posture (Relaxed/Standard/Strict); egress ledger retention + daily review UI; one‑click revoke.
- Hygiene cadence: quarterly key rotation & re‑sign; monthly dependency sweep with golden tests & snapshot diffs.
- Seeded red‑team tests in CI: prompt‑injection, zip‑slip, SSRF, secrets‑in‑logs detector.

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

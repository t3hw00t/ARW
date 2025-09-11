---
title: Tasks Status
---

# Tasks Status

Updated: 2025-09-11 12:00 UTC


## To Do
- [t-250909224103-0211] UI: near-live feedback in /debug — todo (updated: 2025-09-09 20:41:03 UTC)
- [t-250909224103-5251] Policy: hook feedback auto-apply — todo (updated: 2025-09-09 20:41:03 UTC)
- [t-250909224102-9629] Spec: AsyncAPI+MCP artifacts + /spec/* — todo (updated: 2025-09-09 20:41:02 UTC)
- [t-250909224102-8952] Plan: Heuristic engine crate + integration — todo (updated: 2025-09-09 20:41:02 UTC)
- [t-250910115900-0001] RPU: Trust store + signature verification for capsules — todo (ed25519/secp256k1; Sigstore later)
- [t-250910115900-0002] RPU: Cedar ABAC for capsule adoption (issuer/role/tags/TTL/scope) — todo
- [t-250910115900-0003] RPU: Hop TTL + propagation scope enforcement on relay — todo
- [t-250910115900-0004] Bus: inbound relay loop-avoidance and filtering + metrics — todo
- [t-250910115900-0005] Queue: JetStream durable backend (acks/nacks/delay) — todo
- [t-250910115900-0006] Orchestrator: LocalQueue lease sweeper for expired leases — todo
- [t-250910115900-0007] Gating: budgets/quotas with persisted counters — todo
- [t-250910115900-0008] Docgen: gating schema + GATING_KEYS.md from code — todo
- [t-250910115900-0009] Macro: #[arw_gate("key")] for handlers (enforce + docgen) — todo
- [t-250910115900-0010] Capsule: signed provenance + optional Bitcoin timestamping (opt-in) — todo
 - [t-250911100001-1001] Events: add journal reader endpoint `/events/journal?tail=N` — todo (2025-09-11)
 - [t-250911100002-1002] Metrics: counters for model downloads and journal I/O — todo (2025-09-11)
 - [t-250911100003-1003] Downloads: cross-process lockfile around `.part` — todo (2025-09-11)
 - [t-250911100004-1004] Bus: adopt `subscribe_filtered` in connectors/workers where relevant — todo (2025-09-11)
 - [t-250911100005-1005] Metrics: add process/runtime gauges (uptime, mem) — todo (2025-09-11)

## In Progress

## Paused

## Done
- [t-250911095900-0001] Persistence: atomic writes, per-path async locks, cross-process advisory locks, audit rotation — done (2025-09-11)
- [t-250911095900-0002] Event bus: counters + configurable capacity/replay + SSE `Bus.Gap` — done (2025-09-11)
- [t-250911095900-0003] SSE: replay and prefix filters; debug UI presets — done (2025-09-11)
- [t-250911095900-0004] Metrics: Prometheus `/metrics` with bus/events/routes/build info — done (2025-09-11)
- [t-250911095900-0005] Debug UI: metrics link; SSE reconnect control — done (2025-09-11)
- [t-250911095900-0006] Events: optional persistent JSONL journal with rotation — done (2025-09-11)
- [t-250911095900-0007] Models: dedupe concurrent downloads per id — done (2025-09-11)
- [t-250909225652-0810] Gate arrow ingestion bench code — done (updated: 2025-09-09 20:56:52 UTC)
- [t-250909225652-5456] Start lightweight feedback engine — done (updated: 2025-09-09 20:56:52 UTC)
- [t-250909225652-1355] Serve /spec/* endpoints — done (updated: 2025-09-09 20:56:52 UTC)
- [t-250909225651-0602] Unify tools listing with registry — done (updated: 2025-09-09 20:56:52 UTC)
- [t-250909203713-3512] Fix workflows permissions + Windows start + CLI help — done (updated: 2025-09-09 18:37:13 UTC)
- [t-250909203713-1009] Create branch chore/structure-core-fixes — done (updated: 2025-09-09 18:37:13 UTC)
- [t-250909201532-3510] Tag release v0.1.1 and trigger dist — done (updated: 2025-09-09 18:15:32 UTC)
    - 2025-09-09 18:15:32 UTC: pushed tag v0.1.1; GH Actions queued
- [t-250909201215-6424] Tag release v0.1.0 and trigger dist — done (updated: 2025-09-09 18:12:15 UTC)
    - 2025-09-09 18:12:15 UTC: pushed tag v0.1.0; GH Actions run started
- [t-250909200355-9261] Publish artifacts on tags — done (updated: 2025-09-09 18:03:55 UTC)
- [t-250909200354-6386] Add Makefile mirroring Justfile — done (updated: 2025-09-09 18:03:55 UTC)
- [t-250909200354-9170] Enable wasm feature compile + test — done (updated: 2025-09-09 18:03:54 UTC)
- [t-250909195456-5087] Commit and push changes to main — done (updated: 2025-09-09 17:54:56 UTC)
    - 2025-09-09 17:54:56 UTC: HEAD 3cd4819
- [t-250909195456-7421] Add Justfile for common workflows — done (updated: 2025-09-09 17:54:56 UTC)
- [t-250909194935-4017] Lint and tests green — done (updated: 2025-09-09 17:49:35 UTC)
- [t-250909194935-6232] Format codebase (cargo fmt) — done (updated: 2025-09-09 17:49:35 UTC)
- [t-250909194935-0534] Refactor svc ext to use io/paths — done (updated: 2025-09-09 17:49:35 UTC)
- [t-250909193840-6168] Stabilize CI for tray + tests — done (updated: 2025-09-09 17:38:40 UTC)
- [t-250909193840-5994] Add mkdocs.yml config — done (updated: 2025-09-09 17:38:40 UTC)
- [t-250909181725-7338] Create portable dist bundle — done (updated: 2025-09-09 16:17:25 UTC)
- [t-250909170248-9575] Install GTK dev packages + build tray — done (updated: 2025-09-09 16:17:25 UTC)
    - 2025-09-09 16:07:39 UTC: Attempted tray build; pkg-config missing gdk-3.0 (install libgtk-3-dev, ensure pkg-config finds .pc files)
  - 2025-09-09 16:14:14 UTC: Linker error: -lxdo not found; install libxdo-dev
  - 2025-09-09 16:17:25 UTC: Tray built successfully with GTK+xdo
- [t-250909170247-4088] GitHub CLI login — done (updated: 2025-09-09 16:14:14 UTC)
    - 2025-09-09 16:14:14 UTC: Not required (SSH-only auth to GitHub)
- [t-250909180808-9579] Verify SSH git auth to GitHub — done (updated: 2025-09-09 16:08:08 UTC)
- [t-250909180730-7880] Add ARW_NO_TRAY to start.sh — done (updated: 2025-09-09 16:07:30 UTC)
- [t-250909170247-1457] Configure local git identity — done (updated: 2025-09-09 15:02:47 UTC)
- [t-250909170247-6008] Start service and verify /about — done (updated: 2025-09-09 15:02:47 UTC)
- [t-250909170247-6435] Configure Dependabot — done (updated: 2025-09-09 15:02:47 UTC)
- [t-250909170247-9910] Integrate tasks tracker with docs — done (updated: 2025-09-09 15:02:47 UTC)

# Cluster/Gating/Hiearchy Work (2025‑09‑10)
- [x] Pluggable queue/bus; NATS queue groups; inbound NATS→local bus aggregator
- [x] Hierarchy: roles + HTTP hello/offer/accept scaffolding; asyncapi channels
- [x] Gating Orchestrator: central keys; deny contracts; ingress/egress guards
- [x] Policy Capsules in protocol; header-based adoption (ephemeral)
- [x] Apply gating consistently across memory/models/tools/feedback/chat/governor

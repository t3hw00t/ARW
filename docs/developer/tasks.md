---
title: Tasks Status
---

# Tasks Status

Updated: 2025-09-11 23:22 UTC


## To Do
- [t-250912001105-7850] Phase 3: Episodes + Debug UI reactive views — todo (updated: 2025-09-11 22:11:05 UTC)
- [t-250912001100-3438] Phase 2: Beliefs/Intents/Actions stores + endpoints — todo (updated: 2025-09-11 22:11:00 UTC)
- [t-250912001055-0044] Phase 2: Observations read-model + /state/observations — todo (updated: 2025-09-11 22:10:55 UTC)
- [t-250911230333-9794] RPU: watch trust store and strengthen verification — todo (updated: 2025-09-11 21:03:33 UTC)
- [t-250911230329-4722] CLI: migrate arw-cli to clap — todo (updated: 2025-09-11 21:03:29 UTC)
- [t-250911230325-2116] Tests: end-to-end coverage for endpoints & gating — todo (updated: 2025-09-11 21:03:25 UTC)
- [t-250911230320-8615] Metrics: structured registry w/ histograms — todo (updated: 2025-09-11 21:03:20 UTC)
- [t-250911230316-4765] NATS: TLS/auth config and reconnect tuning — todo (updated: 2025-09-11 21:03:16 UTC)
- [t-250911230312-0863] Admin auth: hashed tokens + per-token/IP sliding rate-limit — todo (updated: 2025-09-11 21:03:12 UTC)
- [t-250911230308-0779] Orchestrator: configurable lease, nack delay, and max in-flight — todo (updated: 2025-09-11 21:03:08 UTC)
- [t-250911230306-7961] Introspection: auto-generate /about endpoints from router — todo (updated: 2025-09-11 21:03:06 UTC)
- [t-250911230252-9858] Security: per-route gating layers; slim global admin middleware — todo (updated: 2025-09-11 21:02:52 UTC)
- [t-250911230219-7249] Refactor: split ext/ by domain & unify AppState — todo (updated: 2025-09-11 21:02:19 UTC)
- [t-250909224103-0211] UI: near-live feedback in /debug — todo (updated: 2025-09-09 20:41:03 UTC)
- [t-250909224103-5251] Policy: hook feedback auto-apply — todo (updated: 2025-09-09 20:41:03 UTC)
- [t-250909224102-9629] Spec: AsyncAPI+MCP artifacts + /spec/* — todo (updated: 2025-09-09 20:41:02 UTC)
- [t-250909224102-8952] Plan: Heuristic engine crate + integration — todo (updated: 2025-09-09 20:41:02 UTC)

## In Progress

## Paused
- [t-250911230236-6445] Security: per-route gating layers; slim global admin middleware — paused (updated: 2025-09-11 21:03:46 UTC)
    - 2025-09-11 21:03:46 UTC: Duplicate of t-250911230252-9858; track only the latter.

## Done
- [t-250911230302-0138] Tools: central execution registry unified for /tools and tasks — done (updated: 2025-09-11 21:14:14 UTC)
    - 2025-09-11 21:14:14 UTC: Added tools_exec registry; unified run path for /tools and task worker; preserved listing via arw_core and local list for debug.
- [t-250911230258-1357] Runtime: dynamic HTTP timeout from governor hints — done (updated: 2025-09-11 21:10:37 UTC)
    - 2025-09-11 21:10:37 UTC: Implemented dyn timeout middleware + global handle; wired governor hints + outgoing HTTP; built & tests green for arw-svc.
- [t-250911040804-6168] Clippy: clean workspace with -D warnings — done (updated: 2025-09-11 02:08:05 UTC)
    - 2025-09-11 02:08:05 UTC: events: while-let loop; otel: remove unused prelude import; core: remove duplicated cfg attribute; connector: avoid unreachable tail via ctrl-c guard; svc: explicit OpenOptions + no-op cast
- [t-250911040745-3073] Tests: stabilize gating contract tests — done (updated: 2025-09-11 02:08:04 UTC)
    - 2025-09-11 02:08:04 UTC: arw-core: gate tests now use #[serial]; fixed nondeterministic failure
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


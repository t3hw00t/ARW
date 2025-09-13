# Agents Running Wild (ARW)

<div align="left">

[![CI](https://github.com/t3hw00t/Agent_Hub/actions/workflows/ci.yml/badge.svg)](https://github.com/t3hw00t/Agent_Hub/actions/workflows/ci.yml)
[![Docs](https://img.shields.io/badge/docs-material%20for%20mkdocs-blue)](https://t3hw00t.github.io/Agent_Hub/)
[![Container](https://img.shields.io/badge/ghcr-arw--svc-blue?logo=docker)](https://ghcr.io/t3hw00t/arw-svc)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-informational)](#licensing)
[![Release](https://img.shields.io/github/v/release/t3hw00t/Agent_Hub?display_name=tag)](https://github.com/t3hw00t/Agent_Hub/releases)

</div>

10‑second pitch: local‑first agents with a unified object graph and a single live event stream (SSE). One service powers the Debug UI, CLI, and Recipes — all looking at the same state with strong observability.

General direction: a unified object graph + single event stream. Every surface—Project Hub, Chat, Training Park, Managers (Agents/Models/Hardware/Permissions/Containers/Plugins)—is just a different lens on the same shared objects, driven by the same live events (SSE). This keeps the system coherent, inspectable, and easy to extend.

Full documentation → https://t3hw00t.github.io/Agent_Hub/

## Highlights

- Local‑first: runs offline by default; portable, per‑user state. See `docs/guide/offline_sync.md`.
- Unified object graph: consistent state across Hub, Chat, and Training. See `docs/architecture/object_graph.md`.
- Live events (SSE): one stream drives UIs and tools. See `docs/architecture/events_vocabulary.md` and `docs/architecture/sse_patch_contract.md`.
- Debug UI: inspect episodes, state snapshots, and traces. See `docs/guide/troubleshooting.md`.
- Recipes + Schemas: installable strategy packs with JSON Schemas. See `docs/guide/recipes.md` and `spec/schemas/`.
- Observability: tracing/logging/metrics and journal. See `docs/architecture/observability_otel.md`.
 - Caching Layers: Action Cache with CAS and singleflight; digest‑addressed blob serving with strong validators; read‑models over SSE (JSON Patch deltas with coalescing); llama.cpp prompt caching. See `docs/architecture/caching_layers.md`.

## Try ARW in 2 Minutes

Windows
```powershell
powershell -ExecutionPolicy Bypass -File scripts/setup.ps1
powershell -ExecutionPolicy Bypass -File scripts/start.ps1 -WaitHealth
```

Linux / macOS
```bash
bash scripts/setup.sh
# Option A: Desktop launcher
cargo run -p arw-launcher
# Option B: Headless service only
bash scripts/start.sh --service-only --wait-health
```

Open http://127.0.0.1:8090 and visit `/debug` (set `ARW_DEBUG=1` for local dev). The Debug UI includes an Episodes panel (stitched by `corr_id`) and live state snapshots; server read‑models are exposed under `/state/*` (observations, beliefs, world, intents, actions, episodes, self/{agent}). The desktop launcher tray also exposes “Debug” and “Windows” shortcuts.

Docker (amd64/arm64); Native binaries: Windows (x64/ARM64), macOS (x64/ARM64), Linux (x64/ARM64)
```bash
docker run --rm -p 8090:8090 ghcr.io/t3hw00t/arw-svc:latest
```

## Architecture at a Glance

```
┌──────────────┐    SSE (events)    ┌──────────────┐
│  arw-svc     │ ─────────────────▶ │  Debug UI    │
│  HTTP + SSE  │ ◀───────────────── │  (Browser)   │
└─────┬────────┘    state reads     └─────┬────────┘
      │                                   │
      │ CLI (REST/gRPC)                   │ Recipes + Tools
      ▼                                   ▼
┌──────────────┐                    ┌──────────────┐
│  arw-cli     │                    │  Schemas     │
│  automation  │                    │  JSON Schema │
└──────────────┘                    └──────────────┘
```

<i>Screenshot:</i> see the Debug UI at `/debug` (add `ARW_DEBUG=1`).

## What’s Inside

- Service: user‑mode HTTP with debug UI and SSE events. “Snappy by Default” budgets prioritize first feedback within 50 ms and first partial ≤150 ms; see `docs/ethics/SNAPPY_CHARTER.md`.
- Tools: macro‑driven registration with generated JSON Schemas
- Observability: tracing/logging/metrics and event journal (optional)
- Packaging: portable installs and per‑user state by default
 - Clustering (design): single Home Node with invited Workers under strict policy and egress control; live sharing and pooled compute remain opt‑in. See `docs/architecture/cluster_federation.md`.
 - Egress Firewall (plan): policy‑backed, per‑node loopback proxy + DNS guard with project‑level network posture (Off/Public/Allowlist/Custom), egress ledger, and pre‑offload previews. See `docs/architecture/egress_firewall.md`.

Three primary perspectives
- Project Hub: the center of real‑world work (files/notes/agents/data/runs)
- Chat: an episode viewer/controller bound to project+agent with a live sidecar
- Training Park: impressionistic dials for instincts/priorities, retrieval diversity, tool success, hallucination risk
 - Agent Card: compact self‑model (confidence, competence, costs, active leases) with reliability mini‑chart

Logic Units (config‑first strategy packs)
- Installable “strategy packs” that reconfigure agents safely (config‑only preferred).
- Library UI with tabs (Installed/Experimental/Suggested/Archived), diff preview, A/B try, apply/revert/promote.
- Pairs with a Research Watcher that drafts suggested units from frontier work.

Context Working Set (Never‑Out‑Of‑Context)
- Treat context as a just‑in‑time working set built from layered memories with fixed slot budgets, diversity, and on‑demand rehydration.
- Docs: `docs/architecture/context_working_set.md`; see also Context Recipes and Budgets & Context.

Universal sidecar (always on)
- Episode timeline (obs → belief → intent → action), streaming tokens
- Policy prompts/decisions and runtime/memory meters
- Same sidecar across Hub, Chat, and Training for a coherent mental model

## Next Steps

- Quickstart guide: `docs/guide/quickstart.md`
- Performance & Reasoning Playbook: `docs/guide/performance_reasoning_playbook.md`
- Architecture: `docs/architecture/object_graph.md` and `docs/architecture/events_vocabulary.md`
- World Model: `docs/WORLD_MODEL.md`
- Desktop Launcher: `docs/guide/launcher.md`
- Admin Endpoints: `docs/guide/admin_endpoints.md`
 - Models Download: `docs/guide/models_download.md`
- Security Hardening: `docs/guide/security_hardening.md`
 - Network Posture: `docs/guide/network_posture.md`
- Roadmap: `docs/ROADMAP.md`
 - Clustering blueprint: `docs/architecture/cluster_federation.md`

Commons Kit (what we ship on top)
- One‑click “agent recipes”: manifest bundles of prompts + tools + guardrails + minimal UI. Install by dropping a folder into `recipes/` and launching. See `docs/guide/recipes.md` and schema under `spec/schemas/recipe_manifest.json`.
- Form‑first tools: ARW tool JSON Schemas render parameter forms automatically; validate before dispatch.
- Sensible trust boundaries: default‑deny for file write, shell, and network; per‑recipe ask/allow/never with audit events visible in the sidecar.

## Developers

### Assisted, Iterative Coding

If you use an AI pair‑programmer, start here:
- Working Agreement, Repo Map, and Plan template: `docs/ai/`
- Open a small “AI Task” issue → follow the PLAN → submit a tight PR.

- Enter Nix dev shell: `nix develop`
- Fast loop: `just dev` (runs `arw-svc` with `ARW_DEBUG=1`)
- Docs locally: `just docs-serve` → http://127.0.0.1:8000
- More: `docs/developer/index.md`

90‑day plan (high‑level)
- Weeks 0–2: normalize around Episodes + Projects; ship the universal sidecar; recipe gallery.
- Weeks 2–6: 5 Commons Kit recipes with strict permission prompts; local model backends; speech I/O; “read an Episode log” micro‑guide.
- Weeks 6–10: community pilots (library/school), signed recipe index, iterate to v1.

## Containers

- Run latest image:
  ```bash
  docker run --rm -p 8090:8090 ghcr.io/t3hw00t/arw-svc:latest
  ```
- Compose/Helm examples: see `docs/DEPLOYMENT.md`

## Contributing

See `CONTRIBUTING.md`. Please open issues/PRs and discussions on GitHub.

## Conventions

- Language: US English (American).
- Tone: calm, friendly, and action‑oriented.
- Events: `status` is human‑friendly; `code` is a stable machine hint (e.g., `admission_denied`, `hard_exhausted`, `disk_insufficient`, `canceled_by_user`).
- More: see `docs/developer/style.md` (Style & Harmony).

—

## Who Is It For?

- Builders who want local‑first agents with strong observability.
- Teams exploring recipes/tools with explicit trust boundaries.
- Researchers comparing context and retrieval strategies with live feedback.

## Non‑Goals

- Not a hosted cloud platform; no hidden network egress by default.
- Not a monolithic “one‑true‑agent” — compose via recipes and tools.

## API and Schemas

- OpenAPI (preview): `docs/static/openapi.json` — describes HTTP service and auth.
- Schemas: `spec/schemas/` for recipes, tools, and events.

## Licensing

ARW is dual‑licensed under MIT or Apache‑2.0. See `LICENSE-MIT` and `LICENSE-APACHE`.

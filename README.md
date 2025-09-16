# Agent Hub (ARW)

<div align="left">

[![CI](https://github.com/t3hw00t/ARW/actions/workflows/ci.yml/badge.svg)](https://github.com/t3hw00t/ARW/actions/workflows/ci.yml)
[![Docs Check](https://github.com/t3hw00t/ARW/actions/workflows/docs-check.yml/badge.svg)](https://github.com/t3hw00t/ARW/actions/workflows/docs-check.yml)
[![Docs](https://img.shields.io/badge/docs-material%20for%20mkdocs-blue)](https://t3hw00t.github.io/ARW/)
[![Container](https://img.shields.io/badge/ghcr-arw--svc-blue?logo=docker)](https://ghcr.io/t3hw00t/arw-svc)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-informational)](#licensing)
[![Release](https://img.shields.io/github/v/release/t3hw00t/ARW?display_name=tag)](https://github.com/t3hw00t/ARW/releases)
[![Windows x64 MSI](https://img.shields.io/badge/Windows%20x64-MSI-blue?logo=windows)](https://github.com/t3hw00t/ARW/releases/latest/download/arw-launcher-x64.msi)
[![Windows ARM64 MSI](https://img.shields.io/badge/Windows%20ARM64-MSI-blue?logo=windows)](https://github.com/t3hw00t/ARW/releases/latest/download/arw-launcher-arm64.msi)

</div>

Your private AI control room that can scale and share when you choose.

In plain terms: Agent Hub (ARW) lets you run your own team of AI “helpers” on your computer to research, plan, write, and build—while you stay in charge. It is local‑first and privacy‑first by default, with the option to securely pool computing power with trusted peers when a project needs more muscle.

Full documentation → https://t3hw00t.github.io/ARW/

Feature Matrix → docs/reference/feature_matrix.md (living, generated from `interfaces/features.json`).
Universal Feature Catalog → docs/reference/feature_catalog.md (experience-first map generated from `interfaces/feature_catalog.json`).

General direction: a unified object graph + a single live event stream (SSE). Every surface—Project Hub, Chat, Training Park, and Managers (Agents/Models/Hardware/Permissions/Containers/Plugins)—is just a different lens on the same shared objects, driven by the same live events. This keeps the system coherent, inspectable, and easy to extend.

## Why It’s Different

- You decide access: files, web, mic, and camera are off until you grant time‑limited permission.
- You can see and replay everything: each result shows sources, steps, tools used, and cost; any run can be snapshotted and compared later.
- It grows with you: start on one laptop; when needed, invite other machines to help or co‑drive an agent in real time.
- It is configurable, not brittle: frontier techniques arrive as “Logic Units” (safe strategy packs) you can turn on, A/B test, and roll back in one click.

## What You Can Do

- Turn messy folders, PDFs, and links into clean briefs, reports, or knowledge bases.
- Run a focused research sprint: collect sources, extract facts, compare viewpoints, draft with citations.
- Watch sites or docs for changes and get short, actionable updates.
- Turn vague goals into concrete plans, tasks, and next steps.
- Chat naturally to explore data and export both the answer and the evidence.

## Scaling & Sharing (Opt‑In)

- Pool compute: add your own GPU box or a trusted friend’s machine as a worker. Heavy jobs offload there under your rules and budget.
- Live co‑drive: share an agent session so others can watch, suggest, or take the wheel with your approval. Risky actions still wait in a staging area for you to approve.
- Clear boundaries: before anything leaves your machine, you see what would be sent, to whom, and the estimated cost. An egress ledger records it all.
- Fair splits: contributions (GPU time, tokens, tasks) are metered per collaborator so shared project revenue can be split transparently later.

## Safety & Control

- Permission leases with timers and scopes; no silent escalation.
- A project “world view” tracks key facts, open questions, and constraints so agents act on evidence, not guesses.
- Budgets for time, tokens, and spend; the system stays within plan and shows the meter.
- Signed plugins and sandboxed tools by default.

## Improves Over Time

- Logic Units library adds strategies like better retrieval, cautious tool use, or alternative reasoning styles—without code.
- An experiment mode runs quick A/B checks on saved tasks so changes are data‑driven, not vibes‑driven.
- A curated research watcher suggests new, safe‑to‑try configurations when something promising appears in the wild.

## Who It’s For

- People who want real help on real work without giving away their data.
- Independent builders who prefer practical, local tools that can scale when needed.
- Teams who want transparent collaboration, clear costs, and reproducible results.

## Invitation

If you want AI that is useful, private, and accountable—and that can team up across machines when it matters—Agent Hub is your control room. Start local. Share only when you choose. Stay in the loop the whole time.

## Under the Hood

The details that make ARW practical in real workflows.

- Local‑first: runs offline by default; portable, per‑user state. See `docs/guide/offline_sync.md`.
- Unified object graph: consistent state across Hub, Chat, and Training. See `docs/architecture/object_graph.md`.
- Live events (SSE): one stream drives UIs and tools. See `docs/architecture/events_vocabulary.md` and `docs/architecture/sse_patch_contract.md`.
- Debug UI: inspect episodes, state snapshots, and traces. See `docs/guide/troubleshooting.md`.
- Recipes + Schemas: installable strategy packs with JSON Schemas. See `docs/guide/recipes.md` and `spec/schemas/`.
- Observability: tracing/logging/metrics and journal. See `docs/architecture/observability_otel.md`. CI enforces interactive performance budgets; see `docs/guide/interactive_bench.md`.
 - Caching Layers: Action Cache with CAS and singleflight; digest‑addressed blob serving with strong validators; read‑models over SSE (JSON Patch deltas with coalescing); llama.cpp prompt caching. See `docs/architecture/caching_layers.md`.

## Try ARW in 2 Minutes

Windows
```powershell
powershell -ExecutionPolicy Bypass -File scripts/setup.ps1
powershell -ExecutionPolicy Bypass -File scripts/start.ps1 -WaitHealth
```

- Windows installer: in progress. See Windows Install & Launcher for current paths and the launcher:
  - docs/guide/windows_install.md
  - MSI bundles are attached to GitHub Releases (signed when CI secrets are present).
- The service console now starts minimized to avoid AV heuristics that flag hidden windows; pass `-HideWindow` to restore the
  previous fully hidden behavior.

## Download

- Windows (x64): https://github.com/t3hw00t/ARW/releases/latest/download/arw-launcher-x64.msi
- Windows (ARM64, when available): https://github.com/t3hw00t/ARW/releases/latest/download/arw-launcher-arm64.msi
- All assets and notes: https://github.com/t3hw00t/ARW/releases

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

Verify endpoints
```bash
curl -sS http://127.0.0.1:8090/healthz
curl -sS http://127.0.0.1:8090/about | jq
```

### Debug & Audit Helpers

Quick wrappers exist for common flows:

```bash
# Linux/macOS — quick debug run (opens /debug)
bash scripts/debug.sh --interactive

# Linux/macOS — supply-chain audit (cargo-audit + cargo-deny)
bash scripts/audit.sh --interactive
```

```powershell
# Windows — quick debug run
scripts/debug.ps1 -Interactive

# Windows — supply-chain audit
scripts/audit.ps1 -Interactive
```

## Architecture at a Glance

```
                         Surfaces / Clients
┌────────────────────────────────────────────────────────────────────┐
│  ┌─────────────┐   ┌─────────────┐   ┌──────────────┐             │
│  │ Debug UI    │   │ Launcher UI │   │  arw-cli      │             │
│  │ (Browser)   │   │  (Tauri)    │   │  automations  │             │
│  └─────▲───────┘   └─────▲───────┘   └─────▲────────┘             │
│        │ HTTP/SSE           │ HTTP/SSE        │ REST/gRPC         │
└────────┼────────────────────┼─────────────────┼───────────────────┘
         │                    │                 │
         ▼                    ▼                 ▼
┌────────────────────────────────────────────────────────────────────┐
│                        arw-svc Runtime                             │
│ ┌──────────────┐  ┌──────────────────┐  ┌────────────────────────┐ │
│ │ HTTP Router  │  │ Live Event Bus   │  │ Policy & Gatekeeper    │ │
│ │ + Middleware │◀▶│ (SSE fan-out)    │◀▶│ (Gating, RPU, admin)   │ │
│ └─────┬────────┘  └─────────┬────────┘  └────────┬──────────────┘ │
│       │                     │                    │                │
│ ┌─────▼────────┐   ┌────────▼────────┐  ┌────────▼──────────────┐ │
│ │ Unified      │   │ Resource Pools  │  │ Journal & Kernel      │ │
│ │ Object Graph │   │ (models, memory │  │ (CAS, SQLite, replay) │ │
│ │ + Readmodels │   │  hierarchy)     │  │                      │ │
│ └─────┬────────┘   └────────┬────────┘  └────────┬──────────────┘ │
│       │                     │                    │                │
│ ┌─────▼────────┐   ┌────────▼────────┐  ┌────────▼──────────────┐ │
│ │ Logic Units  │   │ Orchestrator    │  │ Observability & Stats │ │
│ │ + Recipes    │   │ (local + NATS)  │  │ (OTel, budgets, audit)│ │
│ └─────┬────────┘   └────────┬────────┘  └────────┬──────────────┘ │
└───────┼─────────────────────┴─────────────────────────────────────┘
        │                        Event / Task Fabric
        ▼
┌───────────────────────┬──────────────────────────┬────────────────┐
│ Local Task Workers    │ Optional Peer Workers    │ Sandboxed      │
│ & Tool Runners        │ (federation / clusters)  │ Plugins (MCP,  │
│                       │                          │ WASM, Logic)   │
└───────────────────────┴──────────────────────────┴────────────────┘
```

<i>Screenshot:</i> see the Debug UI at `/debug` (add `ARW_DEBUG=1`).

Screenshots → https://t3hw00t.github.io/ARW/guide/screenshots/

## Docker Quickstart

```bash
# Build locally
docker build -f apps/arw-svc/Dockerfile -t arw-svc:dev .

# Run (dev): binds on localhost unless ARW_BIND set
docker run --rm -p 8090:8090 \
  -e ARW_PORT=8090 -e ARW_BIND=0.0.0.0 \
  -e ARW_DEBUG=1 -e ARW_ADMIN_TOKEN=dev-admin \
  arw-svc:dev

# Verify
curl -sS http://127.0.0.1:8090/healthz
```

Pull from GHCR (on releases): `ghcr.io/t3hw00t/arw-svc:latest`. See the Docker guide for compose and hardening.

## Event Topics (Canonical)

- Source of truth: `apps/arw-svc/src/ext/topics.rs` — centralized constants used by the service.
- `models.download.progress`: download lifecycle, progress, and errors; optional `budget` and `disk` fields.
- `models.changed`: models list deltas (add/delete/default/downloaded/error/canceled).
- `models.refreshed`: emitted after default models refresh with `{count}`.
- `models.manifest.written`: manifest sidecar written with `manifest_path` and `sha256`.
- `models.cas.gc`: CAS GC summary after a sweep.
- `egress.preview`: pre‑offload preview payload (dest host/port/protocol) before downloads.
- `egress.ledger.appended`: appended egress ledger entries.
- `state.read.model.patch`: RFC‑6902 JSON Patches; ids include `models`, `models_metrics`, `route_stats`, `snappy`.

## What’s Inside

- Service: user‑mode HTTP with debug UI and SSE events. Interactive performance budgets prioritize first feedback within 50 ms and first partial ≤150 ms; see `docs/guide/interactive_performance.md` and `docs/guide/interactive_bench.md`.
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
- Design Theme & Tokens: `docs/developer/design_theme.md`
- Open Standards & Practices: `docs/developer/standards.md`
- ADRs: `docs/adr/0001-design-tokens-ssot.md`, `docs/adr/0002-events-naming.md`
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
- Events: `status` is human‑friendly; `code` is a stable machine hint (e.g., `admission-denied`, `hard-exhausted`, `disk-insufficient`, `canceled-by-user`).
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

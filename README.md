# Agent Hub (ARW)

<div align="left">

[![CI](https://github.com/t3hw00t/ARW/actions/workflows/ci.yml/badge.svg)](https://github.com/t3hw00t/ARW/actions/workflows/ci.yml)
[![Docs Check](https://github.com/t3hw00t/ARW/actions/workflows/docs-check.yml/badge.svg)](https://github.com/t3hw00t/ARW/actions/workflows/docs-check.yml)
[![Docs](https://img.shields.io/badge/docs-material%20for%20mkdocs-blue)](https://t3hw00t.github.io/ARW/)
[![Container](https://img.shields.io/badge/ghcr-arw--server-blue?logo=docker)](https://ghcr.io/t3hw00t/arw-server)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-informational)](#licensing)
[![Release](https://img.shields.io/github/v/release/t3hw00t/ARW?display_name=tag)](https://github.com/t3hw00t/ARW/releases)
[![Windows x64 MSI](https://img.shields.io/badge/Windows%20x64-MSI-blue?logo=windows)](https://github.com/t3hw00t/ARW/releases/latest/download/arw-launcher-x64.msi)
[![Windows ARM64 MSI](https://img.shields.io/badge/Windows%20ARM64-MSI-blue?logo=windows)](https://github.com/t3hw00t/ARW/releases/latest/download/arw-launcher-arm64.msi)

</div>

Your private AI control room that can scale and share when you choose.

In plain terms: Agent Hub (ARW) lets you run your own team of AI “helpers” on your computer to research, plan, write, and build—while you stay in charge. It is local‑first and privacy‑first by default, with the option to securely pool computing power with trusted peers when a project needs more muscle.

> **Restructure update:** `arw-server` is the new unified API surface (headless-first). The legacy `arw-svc` remains available with `scripts/start.{sh,ps1} --legacy` while the debug UI and launcher are ported. Docs below call out when a command targets the legacy stack.

Full documentation → https://t3hw00t.github.io/ARW/

Feature Matrix → https://t3hw00t.github.io/ARW/reference/feature_matrix/ (living, generated from `interfaces/features.json`).
Universal Feature Catalog → https://t3hw00t.github.io/ARW/reference/feature_catalog/ (experience-first map generated from `interfaces/feature_catalog.json`).

General direction: a unified object graph + a single live event stream (SSE). Every surface—Project Hub, Chat, Training Park, and Managers (Agents/Models/Hardware/Permissions/Containers/Plugins)—is just a different lens on the same shared objects, driven by the same live events. This keeps the system coherent, inspectable, and easy to extend.

> **Staying minimal:** Start with the [Core kernel defaults](#kernel-defaults-core) and stay entirely local. Anything tagged as an [Opt-in pack](#opt-in-pack-tier), a [Remote collaborator pack](#remote-collaborator-pack-tier), or a [Future pack](#future-pack-tier) is optional and stays disabled until you flip it on.

## Feature Tiers

- <a id="core-kernel-tier"></a>**Core kernel** – local-first foundations that run in every install. [Details](#kernel-defaults-core).
- <a id="opt-in-pack-tier"></a>**Opt-in packs** – automation and analysis boosts you turn on when you want more throughput without leaving your machine. [Highlights](#opt-in-packs).
- <a id="remote-collaborator-pack-tier"></a>**Remote collaborator packs** – sharing, federation, and pooled compute that only activate when you invite others. [Details](#remote-collaboration-packs).
- <a id="future-pack-tier"></a>**Future packs** – in-flight packs and experiments we’re hardening. [Details](#future-packs-roadmap).

## Kernel defaults (Core)

These ship with `arw-server` out of the box and keep working even when you stay on the minimal, local-only profile.

### Why It’s Different

- You decide access: files, web, mic, and camera are off until you grant time‑limited permission.
- You can see and replay everything: each result shows sources, steps, tools used, and cost; any run can be snapshotted and compared later.
- It grows with you: start on one laptop; when needed, invite other machines to help or co‑drive an agent in real time.
- It is configurable, not brittle: frontier techniques arrive as “Logic Units” (safe strategy packs) you can turn on, A/B test, and roll back in one click.

### Safety & Control

- **[Core kernel]** Permission leases with timers and scopes; no silent escalation.
- **[Core kernel]** A project “world view” tracks key facts, open questions, and constraints so agents act on evidence, not guesses.
- **[Core kernel]** Budgets for time, tokens, and spend; the system stays within plan and shows the meter.
- **[Core kernel]** Signed plugins and sandboxed tools by default.
- **[Opt-in pack]** Install Logic Units with schema checks so you can stage, audit, and roll back higher-risk automation before it touches production projects.
- **[Remote collaborator pack]** Preview and log every outbound request through the Guardrail Gateway before any data leaves your machine.
- **[Future pack]** Asimov Capsule Guard will add always-on capsule propagation and lease refresh for remote peers.

## What You Can Do

### Core kernel

- Turn messy folders, PDFs, and links into clean briefs, reports, or knowledge bases.
- Run a focused research sprint: collect sources, extract facts, compare viewpoints, draft with citations.
- Turn vague goals into concrete plans, tasks, and next steps.
- Chat naturally to explore data and export both the answer and the evidence.

### Opt-in packs

- Watch sites or docs for changes and get short, actionable updates after you enable the connectors or watcher packs you trust.

### Remote collaborator packs

- Invite a teammate to co-drive a run or offload heavy steps once you’ve pooled compute with trusted peers.

<a id="opt-in-collaboration-extensions"></a>
## Remote collaboration packs

These unlock when you choose to collaborate or federate resources.

### Scaling & Sharing

- **[Core kernel]** Start on one machine and keep every workflow local until you explicitly invite more help.
- **[Opt-in pack]** Install automation packs (Logic Units, experiments, or debugger surfaces) to prep work before you bring collaborators into the loop.
- **[Remote collaborator pack]** Pool compute: add your own GPU box or a trusted friend’s machine as a worker. Heavy jobs offload there under your rules and budget.
- **[Remote collaborator pack]** Live co‑drive: share an agent session so others can watch, suggest, or take the wheel with your approval. Risky actions still wait in a staging area for you to approve.
- **[Remote collaborator pack]** Clear boundaries: before anything leaves your machine, you see what would be sent, to whom, and the estimated cost. An egress ledger records it all.
- **[Future pack]** Fair splits: contributions (GPU time, tokens, tasks) are metered per collaborator so shared project revenue can be split transparently later.

## Future packs (Roadmap)

The packs and expansions we’re hardening next.

### Improves Over Time

- **[Future pack]** Logic Units library continues to add strategies like better retrieval, cautious tool use, or alternative reasoning styles—without code.
- **[Future pack]** An experiment mode runs quick A/B checks on saved tasks so changes are data‑driven, not vibes‑driven.
- **[Future pack]** A curated research watcher suggests new, safe‑to‑try configurations when something promising appears in the wild.

## Who It’s For

- People who want real help on real work without giving away their data.
- Independent builders who prefer practical, local tools that can scale when needed.
- Teams who want transparent collaboration, clear costs, and reproducible results.

## Invitation

If you want AI that is useful, private, and accountable—and that can team up across machines when it matters—Agent Hub is your control room. Start local. Share only when you choose. Stay in the loop the whole time.

## Under the Hood

The details that make ARW practical in real workflows.

- Local‑first: runs offline by default; portable, per‑user state. See https://t3hw00t.github.io/ARW/guide/offline_sync/
- Unified object graph: consistent state across Hub, Chat, and Training. See https://t3hw00t.github.io/ARW/architecture/object_graph/
- Live events (SSE): one stream drives UIs and tools. See https://t3hw00t.github.io/ARW/architecture/events_vocabulary/ and https://t3hw00t.github.io/ARW/architecture/sse_patch_contract/
- Debug UI: inspect episodes, state snapshots, and traces. See https://t3hw00t.github.io/ARW/guide/troubleshooting/
- Recipes + Schemas: installable strategy packs with JSON Schemas. See https://t3hw00t.github.io/ARW/guide/recipes/ and https://github.com/t3hw00t/ARW/tree/main/spec/schemas
- Observability: tracing/logging/metrics and journal. See https://t3hw00t.github.io/ARW/architecture/observability_otel/. CI enforces interactive performance budgets; see https://t3hw00t.github.io/ARW/guide/interactive_bench/
- Performance guardrails: dedupe work via the Action Cache + singleflight, serve digest‑addressed blobs with strong validators, stream read‑model deltas, and reuse llama.cpp prompts. See [Roadmap → Performance Guardrails](docs/ROADMAP.md#performance-guardrails), [Architecture → Performance Guardrails](docs/architecture/performance.md), and https://t3hw00t.github.io/ARW/architecture/caching_layers/

## Try ARW in 2 Minutes

Windows (headless unified server)
```powershell
powershell -ExecutionPolicy Bypass -File scripts/setup.ps1
powershell -ExecutionPolicy Bypass -File scripts/start.ps1 -WaitHealth
```

- Need the legacy debug UI? Append `-Legacy` to the second command to launch `arw-svc` on port 8090 instead of the unified server on 8091.
- Windows installer packages remain available while the launcher is retargeted. See Windows Install: https://t3hw00t.github.io/ARW/guide/windows_install/ for MSI links and tray behavior.

Linux / macOS (headless unified server)
```bash
bash scripts/setup.sh
# Option A: new unified server
bash scripts/start.sh --service-only --wait-health
# Option B: legacy UI
bash scripts/start.sh --legacy --wait-health
```

The unified server is API-first. Point your client or integration to:

```bash
curl -sS http://127.0.0.1:8091/healthz
curl -sS http://127.0.0.1:8091/about | jq
curl -sS -X POST http://127.0.0.1:8091/actions \
  -H 'content-type: application/json' \
  -d '{"kind":"demo.echo","input":{"msg":"hi"}}'
```

Legacy UI surfaces (debug panels, launcher menus) still require `arw-svc` for the moment; run the legacy option above when you need them and watch `/events` + `/state/*` evolve in the new stack.

Docker (amd64/arm64) — unified server
```bash
docker run --rm -p 8091:8091 ghcr.io/t3hw00t/arw-server:latest
```

Verify endpoints
```bash
curl -sS http://127.0.0.1:8091/healthz
curl -sS http://127.0.0.1:8091/about | jq
```

### Debug & Audit Helpers

Quick wrappers exist for common flows:

```bash
# Linux/macOS — quick debug run (legacy UI)
bash scripts/debug.sh --interactive --legacy

# Linux/macOS — supply-chain audit (cargo-audit + cargo-deny)
bash scripts/audit.sh --interactive
```

```powershell
# Windows — quick debug run (legacy UI)
scripts/debug.ps1 -Interactive -Legacy

# Windows — supply-chain audit
scripts/audit.ps1 -Interactive
```

## Download

- Windows (x64): https://github.com/t3hw00t/ARW/releases/latest/download/arw-launcher-x64.msi
- Windows (ARM64): https://github.com/t3hw00t/ARW/releases/latest/download/arw-launcher-arm64.msi
- All assets and notes: https://github.com/t3hw00t/ARW/releases

_Note_: MSI bundles currently ship the legacy `arw-svc` service while the launcher UI migrates. After installation, run `scripts/start.ps1` without `-Legacy` (or `scripts/start.sh --service-only`) to switch to the unified server.

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
│                   arw-server Runtime (Unified)                     │
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

<i>Screenshot:</i> legacy debug UI at `/debug` (start with `--legacy` and add `ARW_DEBUG=1`).

Screenshots → https://t3hw00t.github.io/ARW/guide/screenshots/

## Docker Quickstart

```bash
# Build locally
docker build -f apps/arw-server/Dockerfile -t arw-server:dev .

# Run (dev): binds on localhost unless ARW_BIND set
docker run --rm -p 8091:8091 \
  -e ARW_PORT=8091 -e ARW_BIND=0.0.0.0 \
  -e ARW_ADMIN_TOKEN=dev-admin \
  arw-server:dev

# Verify
curl -sS http://127.0.0.1:8091/healthz
```

Pull from GHCR (on releases): `ghcr.io/t3hw00t/arw-server:latest`. Need the legacy UI image? Use `ghcr.io/t3hw00t/arw-svc:latest` until the new UI lands. See the Docker guide for compose and hardening.

## Event Topics (Canonical)

> During the restructure these constants still live in the legacy service crate; the unified server publishes the same dot.case topics via `arw-events`.

- Source of truth: `crates/arw-topics/src/lib.rs` — centralized constants shared by the service and unified server.
- `models.download.progress`: download lifecycle, progress, and errors; optional `budget` and `disk` fields.
- `models.changed`: models list deltas (add/delete/default/downloaded/error/canceled).
- `models.refreshed`: emitted after default models refresh with `{count}`.
- `models.manifest.written`: manifest sidecar written with `manifest_path` and `sha256`.
- `models.cas.gc`: CAS GC summary after a sweep.
- `egress.preview`: pre‑offload preview payload (dest host/port/protocol) before downloads.
- `egress.ledger.appended`: appended egress ledger entries.
- `state.read.model.patch`: RFC‑6902 JSON Patches; ids include `models`, `models_metrics`, `route_stats`, `snappy`.

## What’s Inside

- Service: user‑mode HTTP with debug UI and SSE events. Interactive performance budgets prioritize first feedback within 50 ms and first partial ≤150 ms; see https://t3hw00t.github.io/ARW/guide/interactive_performance/ and https://t3hw00t.github.io/ARW/guide/interactive_bench/
- Tools: macro‑driven registration with generated JSON Schemas
- Observability: tracing/logging/metrics and event journal (optional)
- Packaging: portable installs and per‑user state by default
 - Clustering (design): single Home Node with invited Workers under strict policy and egress control; live sharing and pooled compute remain opt‑in. See https://t3hw00t.github.io/ARW/architecture/cluster_federation/
 - Egress Firewall (plan): policy‑backed, per‑node loopback proxy + DNS guard with project‑level network posture (Off/Public/Allowlist/Custom), egress ledger, and pre‑offload previews. See https://t3hw00t.github.io/ARW/architecture/egress_firewall/

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
- Docs: https://t3hw00t.github.io/ARW/architecture/context_working_set/; see also Context Recipes and Budgets & Context.

Universal sidecar (always on)
- Episode timeline (obs → belief → intent → action), streaming tokens
- Policy prompts/decisions and runtime/memory meters
- Same sidecar across Hub, Chat, and Training for a coherent mental model

## Next Steps

- Quickstart guide: https://t3hw00t.github.io/ARW/guide/quickstart/
- Performance & Reasoning Playbook: https://t3hw00t.github.io/ARW/guide/performance_reasoning_playbook/
- Design Theme & Tokens: https://t3hw00t.github.io/ARW/developer/design_theme/
- Open Standards & Practices: https://t3hw00t.github.io/ARW/developer/standards/
- ADRs: https://t3hw00t.github.io/ARW/adr/0001-design-tokens-ssot/ , https://t3hw00t.github.io/ARW/adr/0002-events-naming/
- Architecture: https://t3hw00t.github.io/ARW/architecture/object_graph/ and https://t3hw00t.github.io/ARW/architecture/events_vocabulary/
- Desktop Launcher: https://t3hw00t.github.io/ARW/guide/launcher/
- Admin Endpoints: https://t3hw00t.github.io/ARW/guide/admin_endpoints/
 - Models Download: https://t3hw00t.github.io/ARW/guide/models_download/
- Security Hardening: https://t3hw00t.github.io/ARW/guide/security_hardening/
 - Network Posture: https://t3hw00t.github.io/ARW/guide/network_posture/
- Roadmap: https://t3hw00t.github.io/ARW/ROADMAP/
 - Clustering blueprint: https://t3hw00t.github.io/ARW/architecture/cluster_federation/

Commons Kit (what we ship on top)
- One‑click “agent recipes”: manifest bundles of prompts + tools + guardrails + minimal UI. Install by dropping a folder into `recipes/` and launching. See https://t3hw00t.github.io/ARW/guide/recipes/ and schema under https://github.com/t3hw00t/ARW/blob/main/spec/schemas/recipe_manifest.json
- Form‑first tools: ARW tool JSON Schemas render parameter forms automatically; validate before dispatch.
- Sensible trust boundaries: default‑deny for file write, shell, and network; per‑recipe ask/allow/never with audit events visible in the sidecar.

## Developers

### Assisted, Iterative Coding

If you use an AI pair‑programmer, start here:
- Working Agreement, Repo Map, and Plan template: https://t3hw00t.github.io/ARW/ai/ai_index/
- Open a small “AI Task” issue → follow the PLAN → submit a tight PR.

- Enter Nix dev shell: `nix develop`
- Fast loop: `just dev` (runs `arw-svc` with `ARW_DEBUG=1`)
- Docs locally: `just docs-serve` → http://127.0.0.1:8000
- More: https://t3hw00t.github.io/ARW/developer/

90‑day plan (high‑level)
- Weeks 0–2: normalize around Episodes + Projects; ship the universal sidecar; recipe gallery.
- Weeks 2–6: 5 Commons Kit recipes with strict permission prompts; local model backends; speech I/O; “read an Episode log” micro‑guide.
- Weeks 6–10: community pilots (library/school), signed recipe index, iterate to v1.

## Containers

- Run latest image:
  ```bash
  docker run --rm -p 8090:8090 ghcr.io/t3hw00t/arw-svc:latest
  ```
- Compose/Helm examples: see https://t3hw00t.github.io/ARW/guide/docker/

## Contributing

See [CONTRIBUTING.md](https://github.com/t3hw00t/ARW/blob/main/CONTRIBUTING.md). Please open issues/PRs and discussions on GitHub.

## Conventions

- Language: US English (American).
- Tone: calm, friendly, and action‑oriented.
- Events: `status` is human‑friendly; `code` is a stable machine hint (e.g., `admission-denied`, `hard-exhausted`, `disk-insufficient`, `canceled-by-user`).
- More: see https://t3hw00t.github.io/ARW/developer/style/ (Style & Harmony).

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

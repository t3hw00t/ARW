# Agent Hub (ARW)

<div align="left">

[![CI](https://github.com/t3hw00t/ARW/actions/workflows/ci.yml/badge.svg)](https://github.com/t3hw00t/ARW/actions/workflows/ci.yml)
[![Docs Check](https://github.com/t3hw00t/ARW/actions/workflows/docs-check.yml/badge.svg)](https://github.com/t3hw00t/ARW/actions/workflows/docs-check.yml)
[![Docs](https://img.shields.io/badge/docs-material%20for%20mkdocs-blue)](docs/index.md)
[![Container](https://img.shields.io/badge/ghcr-arw--server-blue?logo=docker)](https://ghcr.io/t3hw00t/arw-server)
[![npm](https://img.shields.io/npm/v/%40arw%2Fclient?label=%40arw%2Fclient)](https://www.npmjs.com/package/@arw/client)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-informational)](#licensing)
[![Release](https://img.shields.io/github/v/release/t3hw00t/ARW?display_name=tag)](https://github.com/t3hw00t/ARW/releases)
[![Windows x64 Installer](https://img.shields.io/badge/Windows%20x64-Installer-blue?logo=windows)](docs/guide/windows_install.md#installer-status)
[![Windows ARM64 Installer](https://img.shields.io/badge/Windows%20ARM64-Installer-blue?logo=windows)](docs/guide/windows_install.md#installer-status)

</div>

Your private AI control room that can scale and share when you choose.

In plain terms: Agent Hub (ARW) lets you run your own team of AI “helpers” on your computer to research, plan, write, and build—while laying the groundwork for upcoming voice and vision helpers—all under your control. It is local‑first and privacy‑first by default, with the option to securely pool computing power with trusted peers when a project needs more muscle.

> **Restructure update:** `arw-server` is now the sole API surface (headless-first) across every deployment. The old bridge layer and its launch flags have been retired in favour of the unified stack.

Full documentation → [Docs home](docs/index.md)

Feature Matrix → [Feature matrix reference](docs/reference/feature_matrix.md) (living, generated from `interfaces/features.json`).
Universal Feature Catalog → [Feature catalog](docs/reference/feature_catalog.md) (experience-first map generated from `interfaces/feature_catalog.json`).

General direction: a unified object graph + a single live event stream (SSE). Every surface—Project Hub, Chat, Training Park, and Managers (Agents/Models/Hardware/Permissions/Containers/Plugins)—is just a different lens on the same shared objects, driven by the same live events. This keeps the system coherent, inspectable, and easy to extend.

## At a Glance

| Surface / Pack | Status | Notes |
| --- | --- | --- |
| Project Hub | Shipping | Primary workspace for files, notes, agents, runs. |
| Chat | Shipping | Episode-first chat bound to projects with the shared sidecar. |
| Training Park | Preview | Launcher telemetry and controls live; richer charts and automation in progress. |
| Remote collaborator packs | Preview (opt-in) | Federation, pooled compute, and Guardrail Gateway stay disabled until you enable them. |
| Future packs | Roadmap | Voice & vision studio, Asimov Capsule Guard extensions, automated federation policies. |

> **Staying minimal:** Start with the [Core kernel defaults](#kernel-defaults-core) and stay entirely local. Anything tagged as an [Opt-in pack](#opt-in-packs), a [Remote collaborator pack](#remote-collaborator-packs), or a [Future pack](#future-packs-roadmap) is optional and stays disabled until you flip it on.

## Platform Support

- **Linux:** Desktop Launcher targets Ubuntu 24.04 LTS (or newer) and matching distros with WebKitGTK 4.1 + libsoup3 packages. Headless components (`arw-server`, `arw-cli`) often run on older releases but are only validated on the 24.04+ stack. See [Compatibility Notes](docs/guide/compatibility.md) for distro specifics and alternatives such as the Nix dev shell.
  - Missing WebKitGTK 4.1 on your distro? Run the service headless (`bash scripts/start.sh --service-only --wait-health`) and open the browser Control Room (Hub, Chat, Training, Diagnostics) at `http://127.0.0.1:8091/admin/ui/control/`. Launcher-only tasks like starting/stopping the service or viewing local logs still require the desktop app or CLI scripts. You can also point a Control Room running on another machine via the Active connection picker.
- **Windows/macOS:** Supported per the Tauri/WebView requirements; refer to the compatibility guide for current details.

## Feature Tiers

- **Core kernel** – local-first foundations that run in every install. [Details](#kernel-defaults-core).
- **Opt-in packs** – automation and analysis boosts you turn on when you want more throughput without leaving your machine. [Highlights](#opt-in-packs).
- **Remote collaborator packs** – sharing, federation, and pooled compute that only activate when you invite others. [Details](#remote-collaborator-packs).
- **Future packs** – in-flight packs and experiments we’re hardening. [Details](#future-packs-roadmap).

## Kernel defaults (Core)

These ship with `arw-server` out of the box and keep working even when you stay on the minimal, local-only profile.

### Why It’s Different

- You decide access: files, web, mic, and camera are off until you grant time‑limited permission.
- You can see and replay everything: each result shows sources, steps, tools used, and cost; any run can be snapshotted and compared later.
- It grows with you: start on one laptop; when you opt in, federation preview lets invited machines help or co-drive under your policies.
- It is configurable, not brittle: frontier techniques arrive as “[Logic Units](docs/architecture/logic_units.md)” (safe strategy packs) you can turn on, A/B test, and roll back in one click.

### Safety & Control

- **[Core kernel]** Permission leases with timers and scopes; no silent escalation.
- **[Core kernel]** A project “[world view](docs/architecture/object_graph.md)” tracks key facts, open questions, and constraints so agents act on evidence, not guesses.
- **[Core kernel]** Budgets for time, tokens, and spend; the system stays within plan and shows the meter.
- **[Core kernel]** Signed plugins and sandboxed tools by default.
- **[Opt-in pack]** Install [Logic Units](docs/architecture/logic_units.md) with schema checks so you can stage, audit, and roll back higher-risk automation before it touches production projects.
- **[Remote collaborator pack · Preview]** Turn on the [Guardrail Gateway](docs/architecture/egress_firewall.md) to preview and log outbound requests before any data leaves your machine.
- **[Future pack]** [Asimov Capsule Guard](docs/architecture/asimov_capsule_guard.md) (alpha) keeps capsules refreshed across the unified runtime today; additional propagation hooks and remote presets land as they harden.

## What You Can Do

### Core kernel

- Turn messy folders, PDFs, and links into clean briefs, reports, or knowledge bases.
- Run a focused research sprint: collect sources, extract facts, compare viewpoints, draft with citations.
- Turn vague goals into concrete plans, tasks, and next steps.
- Chat naturally to explore data and export both the answer and the evidence.
- Manage local language-model runtimes with automatic accelerator detection (CUDA, ROCm, Metal, DirectML, CoreML, Vulkan) and graceful CPU fallbacks.

### Opt-in packs

- Watch sites or docs for changes and get short, actionable updates after you enable the connectors or watcher packs you trust.
- **Preview** Prepare for voice, vision, and pointer helpers: consent-first audio/video capture, narration, and high-trust automation are under active development. Track the [Multi-Modal Runtime Plan](docs/architecture/multimodal_runtime_plan.md) for progress.

### Remote collaborator packs

These unlock when you choose to collaborate or federate resources. Remote compute, co‑drive, and the [Guardrail Gateway](docs/architecture/egress_firewall.md) run as opt-in previews while we finish the shared sidecar and cluster matrix.

- **[Core kernel]** Start on one machine and keep every workflow local until you explicitly invite more help.
- **[Opt-in pack]** Install automation packs (Logic Units, experiments, or debugger surfaces) to prep work before you bring collaborators into the loop.
- **[Remote collaborator pack · Preview]** Pool compute: add your own GPU box or a trusted friend’s machine as a worker. Heavy jobs offload there under your rules and budget.
- **[Remote collaborator pack · Preview]** Live co‑drive: share an agent session so others can watch, suggest, or take the wheel with your approval. Risky actions still wait in a staging area for you to approve.
- **[Remote collaborator pack · Preview]** Clear boundaries: before anything leaves your machine, you see what would be sent, to whom, and the estimated cost. Enable the [Guardrail Gateway](docs/architecture/egress_firewall.md) proxy + ledger to capture the audit trail.
- **[Future pack]** Fair splits: contributions (GPU time, tokens, tasks) are metered per collaborator so shared project revenue can be split transparently later.

> **Enable federation preview** Add a `[cluster]` section with `enabled = true` to a config file the server loads by default (for example `configs/default.toml` beside the binaries). If you maintain overrides under `${ARW_STATE_DIR}`, export `ARW_CONFIG` or `ARW_CONFIG_DIR` so the server picks it up. Optionally set `bus`/`queue` to `"nats"`, export `ARW_EGRESS_PROXY_ENABLE=1` and `ARW_EGRESS_LEDGER_ENABLE=1`, then restart `arw-server`.

## Future packs (Roadmap)

The packs and expansions we’re hardening next.

### Improves Over Time

- **[Future pack]** Logic Units library continues to add strategies like better retrieval, cautious tool use, or alternative reasoning styles—without code.
- **[Future pack]** An experiment mode runs quick A/B checks on saved tasks so changes are data‑driven, not vibes‑driven.
- **[Future pack]** A curated research watcher suggests new, safe‑to‑try configurations when something promising appears in the wild.
- **[Future pack]** Memory Overlay Service layers `memory.*` actions, hybrid recall (lexical + vector), and explainable context packs onto the unified object graph.

## Who It’s For

- People who want real help on real work without giving away their data.
- Independent builders who prefer practical, local tools that can scale when needed.
- Teams who want transparent collaboration, clear costs, and reproducible results.

## Invitation

If you want AI that is useful, private, and accountable—and that can team up across machines when it matters—Agent Hub is your control room. Start local. Share only when you choose. Stay in the loop the whole time.

## Under the Hood

The details that make ARW practical in real workflows.

- Local‑first: runs offline by default; portable, per‑user state. See [Offline Sync](docs/guide/offline_sync.md).
- Unified object graph: consistent state across Hub, Chat, and Training today, with planned Voice & Vision surfaces sharing the same backbone. See [Architecture → Object Graph](docs/architecture/object_graph.md).
- Live events (SSE): one stream drives UIs and tools. See [Architecture → Events Vocabulary](docs/architecture/events_vocabulary.md) and [Architecture → SSE Patch Contract](docs/architecture/sse_patch_contract.md).
- Managed runtime supervisor (preview): `arw-server` now ships a process adapter, loads manifests from `configs/runtime/runtimes.toml`, auto-starts tagged runtimes (and stops them when you remove the flag), and keeps them healthy while we continue hardening ONNX Runtime/vLLM support. Manifests follow `spec/schemas/runtime_manifest.json`. See [Managed llama.cpp runtime](docs/architecture/managed_llamacpp_runtime.md) and [Managed runtime supervisor](docs/architecture/managed_runtime_supervisor.md).
- Debug UI: inspect episodes, state snapshots, and traces. See [Troubleshooting guide](docs/guide/troubleshooting.md).
- Recipes + Schemas: installable strategy packs with JSON Schemas. See [Recipes guide](docs/guide/recipes.md) and the [spec schemas directory](https://github.com/t3hw00t/ARW/tree/main/spec/schemas).
- Observability: tracing/logging/metrics and journal. See [Observability architecture](docs/architecture/observability_otel.md). CI enforces interactive performance budgets; see [Interactive bench guide](docs/guide/interactive_bench.md).
- Performance guardrails: dedupe work via the Action Cache + singleflight, serve digest‑addressed blobs with strong validators, stream read‑model deltas, and reuse llama.cpp prompts. See [Roadmap → Performance Guardrails](docs/ROADMAP.md#performance-guardrails), [Architecture → Performance Guardrails](docs/architecture/performance.md), and [Caching layers](docs/architecture/caching_layers.md).

## Try ARW (Quick Path)

> **Heads up:** The unified server and launcher are Rust/Tauri binaries. The first run compiles them, which can take several minutes and requires a full Rust toolchain. Subsequent starts are near-instant.

### 1. Build the binaries

Windows
```powershell
cargo build --release -p arw-server
cargo build --release -p arw-launcher   # optional desktop surfaces
```

Linux / macOS
```bash
cargo build --release -p arw-server
cargo build --release -p arw-launcher   # optional desktop surfaces
```

If you prefer the bundled helper (build + package, docs optional), run:

- Windows: `powershell -ExecutionPolicy Bypass -File scripts/setup.ps1`
- Linux / macOS: `bash scripts/setup.sh`
- Add `-Minimal` / `--minimal` to build only `arw-server`, `arw-cli`, and the launcher without packaging docs on the first run.
- Prefer a portable build with no compilation? Grab the latest release archive from [GitHub Releases](https://github.com/t3hw00t/ARW/releases), then run `bin/arw-server` (service) and optionally `bin/arw-launcher`.

### 2. Set an admin token & start the unified server

Windows (headless)
```powershell
if (-not $env:ARW_ADMIN_TOKEN) { $env:ARW_ADMIN_TOKEN = [System.Guid]::NewGuid().ToString("N") }
powershell -ExecutionPolicy Bypass -File scripts/start.ps1 -ServiceOnly -WaitHealth -AdminToken $env:ARW_ADMIN_TOKEN
```

Linux / macOS (headless)
```bash
export ARW_ADMIN_TOKEN="${ARW_ADMIN_TOKEN:-$(openssl rand -hex 32)}"
bash scripts/start.sh --service-only --wait-health --admin-token "$ARW_ADMIN_TOKEN"
```

- Windows installer packages (when available) ship the launcher with `arw-server` + `arw-cli`. See the [Windows install guide](docs/guide/windows_install.md) for MSI links and tray behavior.
- The start scripts persist the exported token into the launcher preferences when possible so the Control Room unlocks Hub, Chat, and Training automatically. You can still paste or rotate the token manually from **Connection & alerts**.
- Working without a desktop runtime? Keep the service headless and open `http://127.0.0.1:8091/admin/ui/control/` (Control Room) or `http://127.0.0.1:8091/admin/debug` in any modern browser.
- Use the **Active connection** picker in Control Room → Connection & alerts to flip between the local stack and saved remotes without leaving the hero panel.

### 3. Optional: enable screenshots tooling
```bash
cargo run -p arw-server --features tool_screenshots
```

The unified server is API-first. Point your client or integration to (with `arw-server` running locally):

```bash
curl -sS http://127.0.0.1:8091/healthz
curl -sS http://127.0.0.1:8091/about | jq
curl -sS -X POST http://127.0.0.1:8091/actions \
  -H 'content-type: application/json' \
  -d '{"kind":"demo.echo","input":{"msg":"hi"}}'
```

These localhost endpoints assume a healthy `arw-server` bound to port `8091`.

Docker (amd64/arm64) — unified server
```bash
# Generate a strong admin token once and keep it secret
export ARW_ADMIN_TOKEN="$(openssl rand -hex 32)"

docker run --rm -p 8091:8091 \
  -e ARW_BIND=0.0.0.0 \
  -e ARW_PORT=8091 \
  -e ARW_ADMIN_TOKEN="$ARW_ADMIN_TOKEN" \
  ghcr.io/t3hw00t/arw-server:latest
```

Use any equivalent secret generator if `openssl` is unavailable.

Use the same `/healthz` and `/about` checks above to confirm the container is ready.

### Debug & Audit Helpers

Quick wrappers exist for common flows:

```bash
# Linux/macOS — quick dev server
just dev

# Linux/macOS — supply-chain audit (cargo-audit + cargo-deny)
bash scripts/audit.sh --interactive
```

```powershell
# Windows — quick debug run (debug UI served by arw-server)
scripts/debug.ps1 -Interactive

# Windows — supply-chain audit
scripts/audit.ps1 -Interactive
```

## Download

- Windows installer status (x64/ARM64) and manual build steps: [docs/guide/windows_install.md#installer-status](docs/guide/windows_install.md#installer-status)
- macOS install & launcher guide: [docs/guide/macos_install.md](docs/guide/macos_install.md)
- Release archives and checksums: [GitHub Releases](https://github.com/t3hw00t/ARW/releases)

_Note_: MSI bundles (when published) ship the launcher alongside `arw-server`. Run the unified server directly (`scripts/start.ps1 -ServiceOnly` or `scripts/start.sh --service-only`) for headless installs; the debug panels are served by `arw-server` when `ARW_DEBUG=1`.

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

<i>Screenshot:</i> debug UI at `/admin/debug`.

Screenshots → [Screenshots guide](docs/guide/screenshots.md)

## Clients (TypeScript)

- Minimal client for `/actions`, `/events` (SSE), and `/state/*` with Node/browser support.
- NPM: `@arw/client` — see docs for usage and the bundled `arw-events` CLI.
- Docs: [Clients reference](docs/reference/clients.md)

## Operations & Monitoring

- Read stability and crash‑recovery details in [`docs/OPERATIONS.md`](docs/OPERATIONS.md).
- Prometheus alerting examples and dashboard tips in [`docs/ALERTING.md`](docs/ALERTING.md).

## Docker Quickstart

```bash
# Build locally
docker build -f apps/arw-server/Dockerfile -t arw-server:dev .

# Run (dev): override bind address for LAN testing
export ARW_ADMIN_TOKEN="${ARW_ADMIN_TOKEN:-$(openssl rand -hex 32)}"
docker run --rm -p 8091:8091 \
  -e ARW_BIND=0.0.0.0 \
  -e ARW_PORT=8091 \
  -e ARW_ADMIN_TOKEN="$ARW_ADMIN_TOKEN" \
  arw-server:dev

# Verify
curl -sS http://127.0.0.1:8091/healthz
```

Pull from GHCR (on releases): `ghcr.io/t3hw00t/arw-server:latest`. See the Docker guide for compose and hardening.

If you run the container on another host, change the healthcheck URL so it points at that machine instead of `127.0.0.1`.

## Event Topics (Canonical)

> Topics are authored in `crates/arw-topics`; `arw-server` emits the same dot.case events consumed by all surfaces.

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

- Service: user‑mode HTTP with debug UI and SSE events. Interactive performance budgets prioritize first feedback within 50 ms and first partial ≤150 ms; see [Interactive performance guide](docs/guide/interactive_performance.md) and [Interactive bench guide](docs/guide/interactive_bench.md).
- Tools: macro‑driven registration with generated JSON Schemas
- Observability: tracing/logging/metrics and event journal (optional)
- Packaging: portable installs and per‑user state by default
- Clustering (preview): single Home Node with invited Workers under strict policy and egress control; live sharing and pooled compute remain opt-in. See [Cluster federation architecture](docs/architecture/cluster_federation.md).
- Egress Firewall (preview): policy-backed, per-node loopback proxy + DNS guard with project-level network posture (Off/Public/Allowlist/Custom), egress ledger, and pre-offload previews when enabled. See [Egress firewall architecture](docs/architecture/egress_firewall.md).

Three primary perspectives
- Project Hub: the center of real‑world work (files/notes/agents/data/runs)
- Chat: an episode viewer/controller bound to project+agent with a live sidecar
- Training Park (preview): impressionistic dials for instincts/priorities, retrieval diversity, tool success, hallucination risk
- Voice & Vision Studio (roadmap): consent-first capture, transcription, description, narration, and playback will ship with the managed runtime supervisor
 - Agent Card: compact self‑model (confidence, competence, costs, active leases) with reliability mini‑chart

[Logic Units](docs/architecture/logic_units.md) (config‑first strategy packs)
- Installable “strategy packs” that reconfigure agents safely (config‑only preferred).
- Library UI with tabs (Installed/Experimental/Suggested/Archived), diff preview, A/B try, apply/revert/promote.
- Pairs with a Research Watcher that drafts suggested units from frontier work.

Context Working Set (Never‑Out‑Of‑Context)
- Treat context as a just‑in‑time working set built from layered memories with fixed slot budgets, diversity, and on‑demand rehydration.
- Docs: [Context working set](docs/architecture/context_working_set.md); see also Context Recipes and Budgets & Context.

Universal sidecar (always on)
- Episode timeline (obs → belief → intent → action), streaming tokens
- Policy prompts/decisions and runtime/memory meters
- Same sidecar across Hub, Chat, and Training for a coherent mental model

## Next Steps

- [Quickstart guide](docs/guide/quickstart.md)
- [Performance & Reasoning Playbook](docs/guide/performance_reasoning_playbook.md)
- [Design Theme & Tokens](docs/developer/design_theme.md)
- [Open Standards & Practices](docs/developer/standards.md)
- ADRs: [0001 — Design tokens SSOT](docs/adr/0001-design-tokens-ssot.md), [0002 — Events naming](docs/adr/0002-events-naming.md)
- Architecture: [Object graph](docs/architecture/object_graph.md), [Events vocabulary](docs/architecture/events_vocabulary.md)
- [Desktop Launcher](docs/guide/launcher.md)
- [Admin Endpoints](docs/guide/admin_endpoints.md)
- [Models download](docs/guide/models_download.md)
- [Security hardening](docs/guide/security_hardening.md)
- [Network posture](docs/guide/network_posture.md)
- [Roadmap](docs/ROADMAP.md)
- [Clustering blueprint](docs/architecture/cluster_federation.md)

Commons Kit (what we ship on top)
- One‑click “agent recipes”: manifest bundles of prompts + tools + guardrails + minimal UI. Install by dropping a folder into `${ARW_STATE_DIR:-state}/recipes/` (created on first run) and launching. See [Recipes guide](docs/guide/recipes.md) and the [recipe manifest schema](https://github.com/t3hw00t/ARW/blob/main/spec/schemas/recipe_manifest.json).
- Form‑first tools: ARW tool JSON Schemas render parameter forms automatically; validate before dispatch.
- Sensible trust boundaries: default‑deny for file write, shell, and network; per‑recipe ask/allow/never with audit events visible in the sidecar.

## Developers

### Assisted, Iterative Coding

If you use an AI pair‑programmer, start here:
- Working Agreement, Repo Map, and Plan template: [AI pairing index](docs/ai/ai_index.md)
- Open a small “AI Task” issue → follow the PLAN → submit a tight PR.

- Enter Nix dev shell: `nix develop`
- Ensure your Rust toolchain is 1.90+ (`rustup update` keeps you on the latest stable)
- We iterate on the latest stable Rust; expect sharp edges and flag risky features behind toggles when contributing
- Fast loop: `just dev` (runs `arw-server` with `ARW_DEBUG=1`)
- Docs locally: `just docs-serve` → open at `localhost:8000`
- Workspace cleanup: `just clean` (append `--venv` to drop the local virtualenv)
- More: [Developer index](docs/developer/index.md)

90‑day plan (high‑level)
- Weeks 0–2: normalize around Episodes + Projects; ship the universal sidecar; recipe gallery.
- Weeks 2–6: 5 Commons Kit recipes with strict permission prompts; local model backends; speech I/O; “read an Episode log” micro‑guide.
- Weeks 6–10: community pilots (library/school), signed recipe index, iterate to v1.

## Containers

- Run latest image:
  ```bash
  export ARW_ADMIN_TOKEN="${ARW_ADMIN_TOKEN:-$(openssl rand -hex 32)}"
  docker run --rm -p 8091:8091 \
    -e ARW_BIND=0.0.0.0 \
    -e ARW_PORT=8091 \
    -e ARW_ADMIN_TOKEN="$ARW_ADMIN_TOKEN" \
    ghcr.io/t3hw00t/arw-server:latest
  ```
- Compose/Helm examples: see [Docker guide](docs/guide/docker.md)

## Contributing

See [CONTRIBUTING.md](https://github.com/t3hw00t/ARW/blob/main/CONTRIBUTING.md). Please open issues/PRs and discussions on GitHub.

## Conventions

- Language: US English (American).
- Tone: calm, friendly, and action‑oriented.
- Events: `status` is human‑friendly; `code` is a stable machine hint (e.g., `admission-denied`, `hard-budget`, `disk_insufficient`, `canceled-by-user`).
- More: see [Style & Harmony](docs/developer/style.md).

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

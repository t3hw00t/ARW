# Agents Running Wild (ARW)

Local‑first Rust workspace for building and running personal AI agents. ARW bundles a lightweight service, CLI, and optional desktop UI so you can experiment without cloud lock‑in.

General direction: a unified object graph + single event stream. Every surface—Project Hub, Chat, Training Park, Managers (Agents/Models/Hardware/Permissions/Containers/Plugins)—is just a different lens on the same shared objects, driven by the same live events (SSE). This keeps the system coherent, inspectable, and easy to extend.

## Try ARW in 2 Minutes

Windows
```powershell
powershell -ExecutionPolicy Bypass -File scripts/setup.ps1
powershell -ExecutionPolicy Bypass -File scripts/start.ps1 -WaitHealth
```

Linux / macOS
```bash
bash scripts/setup.sh
bash scripts/start.sh --wait-health
```

Open http://127.0.0.1:8090 and visit `/debug` (set `ARW_DEBUG=1` for local dev). The Debug UI includes an Episodes panel (stitched by `corr_id`) and live state snapshots; server read‑models are exposed under `/state/*` (observations, beliefs, intents, actions, episodes).

Docker (amd64/arm64); Native binaries: Windows (x64/ARM64), macOS (x64/ARM64), Linux (x64/ARM64)
```bash
docker run --rm -p 8090:8090 ghcr.io/t3hw00t/arw-svc:latest
```

## What’s Inside

- Service: user‑mode HTTP with debug UI and SSE events
- Tools: macro‑driven registration with generated JSON Schemas
- Observability: tracing/logging/metrics and event journal (optional)
- Packaging: portable installs and per‑user state by default

Three primary perspectives
- Project Hub: the center of real‑world work (files/notes/agents/data/runs)
- Chat: an episode viewer/controller bound to project+agent with a live sidecar
- Training Park: impressionistic dials for instincts/priorities, retrieval diversity, tool success, hallucination risk

Logic Units (config‑first strategy packs)
- Installable “strategy packs” that reconfigure agents safely (config‑only preferred).
- Library UI with tabs (Installed/Experimental/Suggested/Archived), diff preview, A/B try, apply/revert/promote.
- Pairs with a Research Watcher that drafts suggested units from frontier work.

Universal sidecar (always on)
- Episode timeline (obs → belief → intent → action), streaming tokens
- Policy prompts/decisions and runtime/memory meters
- Same sidecar across Hub, Chat, and Training for a coherent mental model

## Next Steps

- Quickstart guide: `docs/guide/quickstart.md`
- Architecture: `docs/architecture/object_graph.md` and `docs/architecture/events_vocabulary.md`
- Desktop Launcher: `docs/guide/launcher.md`
- Admin Endpoints: `docs/guide/admin_endpoints.md`
- Security Hardening: `docs/guide/security_hardening.md`
- Roadmap: `docs/ROADMAP.md`

Commons Kit (what we ship on top)
- One‑click “agent recipes”: manifest bundles of prompts + tools + guardrails + minimal UI. Install by dropping a folder into `recipes/` and launching. See `docs/guide/recipes.md` and schema under `spec/schemas/recipe_manifest.json`.
- Form‑first tools: ARW tool JSON Schemas render parameter forms automatically; validate before dispatch.
- Sensible trust boundaries: default‑deny for file write, shell, and network; per‑recipe ask/allow/never with audit events visible in the sidecar.

## Developers

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

ARW is dual‑licensed under MIT or Apache‑2.0.

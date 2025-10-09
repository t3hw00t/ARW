---
title: Expert Alignment Plan
---

# Expert Alignment Plan

Updated: 2025-10-09
Type: Reference

## Purpose

This plan translates the external architectural review into a concrete execution path that safeguards long-term stability, accessibility, and extensibility. It complements `docs/ROADMAP.md` by prioritising the highest-leverage gaps and sequencing the delivery workstreams that unlock real-world deployments beyond the solo/local focus.

## Guiding Principles
- **Accessibility-first delivery**: every surface ships with keyboard parity, screen reader hints, and consent overlays before we mark a feature complete.
- **Local-first by default** with opt-in federation: remote helpers never widen policy scope or egress without explicit leases and ledger coverage.
- **Bundles over bespoke setup**: ARW owns the lifecycle for supported runtimes so operators get predictable upgrades and rollbacks.
- **Observable-by-design**: every background service emits health, budgets, and actionable failure reasons into `/state/*`, `/metrics`, and Launcher dashboards.
- **Harmonised packs**: optional packs extend the kernel via manifests (logic units, runtimes, tools) so upgrades remain clean and reproducible.

## Priority Themes

1. **Managed Runtime bundles (Text + Audio preview)**  
   Deliver a turnkey runtime experience that covers llama.cpp and Whisper.cpp, including download, activation, health, and restart policies.

2. **Federation MVP & Cluster Matrix**  
   Graduate the remote worker preview into a supported opt-in feature with scheduler integration, auditability, and operator tooling.

3. **Mini-Agent Catalog & Training Park readiness**  
   Seed the orchestrator with curated mini-agents, telemetry, and Launcher workflows so Training Park surfaces deliver value on day one.

4. **Accessibility & Consent Foundations**  
   Codify accessibility gates, consent overlays, and policy hooks for upcoming multi-modal surfaces (voice, vision, pointer).

5. **Operational Excellence & Release Engineering**  
   Harden CI pipelines, packaging, and documentation to support regular bundle releases with deterministic rollbacks.

## Initiatives & Milestones

### 1. Managed Runtime Bundles (Q4 2025)
**Goal:** Zero-config text + audio runtimes with integrated health and policy.

| Phase | Scope | Checkpoints |
| --- | --- | --- |
| A. Bundle Catalog | Signed llama.cpp & Whisper.cpp binaries per platform; manifest schema updates (`spec/schemas/runtime_manifest.json`) to declare bundle channel & hash. | - `configs/runtime/bundles.llama.json` + `bundles.audio.json` checked in with signature metadata.<br>- CLI helper `arw-cli runtime bundles list/install`. |
| B. Supervisor integration | Extend `RuntimeSupervisor` with bundle resolver, preset mapping, and accelerator detection. | - `arw-server` auto-registers bundles found in `state/runtime/bundles`.<br>- `/state/runtimes` exposes bundle + health summary.<br>- Restart budgets enforced per bundle. |
| C. Launcher UX | Voice & runtime tabs with consent prompts, install/activate flows, health toasts. | - Guided setup wizard with hardware probe + recommended profile.<br>- Keyboard-only path validated (NVDA/VoiceOver smoke). |
| D. Accessibility + Docs | Consent overlays, troubleshooting, scripts for offline installs. | - `docs/guide/runtime_manager.md` and `guide/vision_runtime.md` published.<br>- Offline bundle import workflow (`arw-cli runtime bundles import`). |

_Status (2025-10-06): placeholder catalogs (`bundles.llama.json`, `bundles.vision.json`, `bundles.audio.json`) ship in preview with URLs left for the signing pipeline; `/state/runtime/bundles`, `arw-cli runtime bundles list --remote --json`, and `arw-cli runtime bundles reload` expose and refresh the inventory for early testing._
### 2. Federation MVP & Cluster Matrix (Q1 2026)
**Goal:** Reliable remote execution with observable cluster health and policy continuity.

| Phase | Scope | Checkpoints |
| --- | --- | --- |
| A. Remote Worker Shim | gRPC/WebSocket worker receiving leased actions, streaming results. | - Worker binary in `apps/arw-worker` crate.<br>- Lease + capsule propagation validated end-to-end. |
| B. Scheduler Integration | Kernel queue routes background jobs to remote workers under budgets. | - Scheduling policies configurable via `configs/federation/*.toml`.<br>- `/state/cluster` read-model lists nodes, queues, health, budgets. |
| C. Cluster Matrix UI | Launcher + CLI dashboards showing peers, capabilities, and cost. | - Launcher “Cluster” panel with live SSE updates.<br>- CLI command `arw-cli cluster status --watch`. |
| D. Ledger & Settlements | Contribution ledger capturing GPU-seconds/tokens; CSV export. | - `/state/cluster/contributions` endpoint.<br>- Scheduled export via `arw-cli cluster ledger export`. |

### 3. Mini-Agent Catalog & Training Park (Q4 2025)
**Goal:** Out-of-the-box logic units and telemetry so Training Park is actionable.

| Phase | Scope | Checkpoints |
| --- | --- | --- |
| A. Catalog Definition | Define schema for mini-agent packs (`spec/schemas/mini_agent.json`). | - `interfaces/mini_agents.json` with curated entries.<br>- Generator script `scripts/gen_mini_catalog.py`. |
| B. Kernel Support | Populate `/orchestrator/mini_agents` from catalog; surface metadata. | - `apps/arw-server/src/api/orchestrator.rs` returns catalog entries with status.<br>- Health + adoption metrics logged to `/state/training/telemetry`. |
| C. Launcher & CLI | Upgrade Training Park to display catalog, run jobs, surface outcomes. | - Filterable list with keyboard support.<br>- `arw-cli mini-agent run <id>` command. |
| D. Telemetry & Goldens | Golden runs + experiments harness for catalog regression. | - Nightly `just mini-agents-smoke` workflow.<br>- Docs update `guide/experiments_ab.md` with new lanes. |

### 4. Accessibility & Consent Foundations (Continuous, first deliverables Q4 2025)
**Goal:** Bake accessibility checks and consent flows into kernel and UI before multi-modal release.

Actions:
- Accessibility checklist gating CI (`quality/accessibility_checklist.md`), enforced via `just accessibility-ci`.
- Consent overlay component in Launcher, reused across audio/vision/pointer tabs.
- Kernel policy scopes (`audio:*`, `vision:*`, `input:*`) pre-defined with lease expiry defaults.
- Test harness covering keyboard-only, screen reader (NVDA/VoiceOver), and high-contrast themes.

### 5. Operational Excellence (Continuous)
**Goal:** Ensure reproducible releases and clear operator paths.

Actions:
- Bundle signing pipeline (GitHub Actions) delivering `ghcr.io/.../arw-bundles:<version>`.
- Nightly integration tests on Windows/Linux/macOS runners with managed runtimes enabled.
- Release checklist updates (`docs/guide/release.md`) including accessibility, federation, and bundle validation.
- Observability pack for remote deployments: OTEL exemplar dashboards and `docs/ops/cluster_runbook.md`.

## Sequencing
1. **Kick-off (Sprint 1-2)**: Bundle catalog groundwork, accessibility checklist, mini-agent schema.
2. **Pilot Delivery (Sprint 3-6)**: Ship llama bundle + Launcher wizard; seed mini-agent catalog; release CLI tooling.
3. **Federation MVP (Sprint 7-12)**: Remote worker shim, scheduler integration, cluster matrix UI.
4. **Multi-modal Preview (Sprint 13+)**: Whisper bundle activation with consent flows; gradually add vision/pointer adapters following accessibility gates.

## Success Metrics
- **Runtime adoption**: ≥80% of installs enable managed llama bundle within first week; <1% restart-loop incidents per release.
- **Training Park usage**: Mini-agent runs per active install increases 3×; <5% failure rate across catalog.
- **Federation stability**: Remote worker job completion success ≥99% with restart recovery <60s; ledger exports reconcile with execution history.
- **Accessibility compliance**: All new UI surfaces pass automated axe-core scans and manual screen reader smoke tests.
- **Operator satisfaction**: Trial cohort feedback (docs/ops/trials) shows “frictionless setup” rating ≥4/5 post-launch.

## Next Steps
1. Form dedicated bundle squad (runtime + release engineering) and spin up pilot hardware lab.
2. Create RFCs for bundle signing pipeline and remote worker protocol; route through architecture review.
3. Allocate accessibility champion within Launcher to own checklist adoption.
4. Begin drafting CLI/Launcher UX copy with documentation in parallel to engineering delivery.

This plan will evolve alongside quarterly roadmap updates; keep `docs/ROADMAP.md` and `docs/BACKLOG.md` in sync as phases land.

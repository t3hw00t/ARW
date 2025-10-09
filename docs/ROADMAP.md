---
title: Roadmap
---

# Roadmap

Updated: 2025-10-07
Type: Reference

See also: [Backlog](BACKLOG.md), [Interface Roadmap](INTERFACE_ROADMAP.md), and the [Expert Alignment Plan](architecture/expert_alignment_plan.md) for phased delivery details and success metrics.
Roadmap highlights themes and timelines; Backlog tracks actionable items.

## Priority One – Managed Runtime Supervisor

Promote the managed runtime supervisor from blueprint to the primary kernel initiative. This work elevates local model orchestration to the same Class 1 footing as Model Steward so every install gets a stable, policy-aligned runtime stack.

- **Phase 1: Stabilize the Runtime Matrix** — expand the existing `/state/runtime_matrix` service and `runtime.state.changed` events with health causes, restart budgets, and accessibility-focused status hints (expected completion: 2 sprints). Ship smoke tests that launch the current llama.cpp integration under CPU-only and GPU hosts to keep regressions visible.
- **Phase 2: Supervisor Core** — land the `RuntimeRegistry` service, adapter contract, and guarded start/stop APIs for text runtimes. Ensure Cedar policy and Guardrail Gateway leases gate accelerator access. Integration tests must exercise failure recovery, lease enforcement, and Snappy budgets.
- **Phase 3: Multimodal Expansion** — lead with vision adapters (llava.cpp/Moondream) and consent-first camera UX, then follow with audio (Whisper.cpp/Piper) once the shared overlays and restart budgets are in place. Ship prompt-cache warmers, provenance journaling, and accessibility defaults (captions, high-contrast focus states, keyboard parity).
- **Phase 4: Federation Hooks** — expose runtime claims through orchestrator lanes and worker manifests so remote nodes advertise accelerator profiles safely. Include ledger coverage to track shared GPU usage.
- **Documentation & UX** — replace the llama.cpp blueprint with the finalized supervisor design, add operator runbooks, and surface profiles plus consent cues in Launcher. Keep docs, feature matrix, and roadmap checkpoints in sync as phases land.

<a id="priority-two--modular-cognitive-stack--memory-orchestration"></a>
## Priority Two – Modular Cognitive Stack & Memory Orchestration

Establish the multi-agent cognitive scaffold that keeps user-facing chat, memory, validation, and tooling specialists aligned under a single orchestrator.

- **Phase 0: Foundations** — [Kernel][Pack: Research] finalize the modular cognitive stack architecture, typed message schemas, and gating policy updates (expected completion: 2 sprints). Deliver contracts for agent registration, memory lane access, and tool brokerage with docs in place.
- **Phase 1: Memory Fabric Integration** — [Kernel][Pack: Research] connect recall/compression agents to the Memory Abstraction Layer with short-term buffers, episodic logs, and compression tiers instrumented for loss metrics. Ship hygiene jobs, retention policies, and provenance metadata surfaced in `/state/memory`.
- **Phase 2: Validation & Governance** — [Kernel][Pack: Collaboration] wire the Validation Agent into the orchestrator loop, enforce privacy/PII scrubbing before durable storage, and expose provenance trails in UI and `/state/episodes`. Expand guardrail policies to cover tool outputs and prompt-injection detection.
- **Phase 3: Tooling & Planner** — [Kernel][Pack: Research] broker tool execution with sandbox requirements, retries, and audit trails; introduce an optional planner that can fan out to specialists while respecting lease scopes. Integrate telemetry with the Evaluation Harness for AB testing.
- **Documentation & Telemetry** — Publish operator guides, UI affordances, and dashboard widgets for memory hit rate, validation catch rate, and compression health. Keep [Backlog](BACKLOG.md) and [Developer Tasks](developer/tasks.md) aligned as phases land.

## Scope Badges

The roadmap and its planning mirrors use badges to flag which slice of the stack an initiative touches and how that work supports the Complexity Collapse program of record:

- `[Kernel]` — Hardens the runtime, policy, and journal so the “Collapse the Kernel” thrust stays minimal, dependable, and auditable.
- `[Pack: Collaboration]` — Optional collaboration, UI, and workflow packs that give calm surfaces and governance without bloating the kernel.
- `[Pack: Research]` — Optional research, experimentation, and memory packs that extend retrieval, clustering, and replay while staying pluggable.
- `[Pack: Federation]` — Optional federation packs that let multiple installs cooperate under shared policy, budgets, and accountability.
- `[Future]` — Bets incubating beyond the active quarter; they stay visible but outside the current Complexity Collapse execution window.

Badges can be combined (for example, `[Pack: Collaboration][Future]`) to show both the optional pack and that the work sits beyond the active delivery window.

## Execution Streams (Alignment)

To keep the larger intent visible while we iterate, we track four active execution streams. Each stream carries a short list of near-term moves, explicit checks, and documentation hooks so we stay synchronized as work lands.

### Collapse the Kernel
- Immediate moves: Snappy Governor verification and Event Spine patch replay (JSON Patch SSE test) both landed; the restructure board is closed, so shift focus to backlog items that extend instrumentation and UI patch consumers.
- Checks & optimizations: continue verifying latency budgets through `/metrics` and `route_stats`; confirm CAS + SQLite dual-write paths capture Snappy events without regressions.
- Documentation: restructure timeline is final; keep backlog → Complexity Collapse entries in sync as new work spins up outside the restructure gate.

### Never-Out-Of-Context
- Immediate moves: land slot budgets and stable IDs in the Context API, ship the MMR selector pass, and draft the compression cascade executor so the Training Park metrics have a feed.
- Immediate moves: kick off the Modular Cognitive Stack MVP (agent contracts, recall/compression wiring, validation loop) and stage tool brokerage schemas for review.
- Checks & optimizations: add telemetry assertions for `context.recall.risk` and `context.coverage`; enforce hygiene caps/TTL janitor runs in integration tests.
- Documentation: expand guide coverage for the new API surfaces and annotate Backlog entries as instrumentation ships.

### Collaboration & Human-in-Loop Surfaces
- Immediate moves: finalize the recipes runner/gallery, wire the Heuristic Feedback Engine shadow lane, surface the pending action queue in the sidecar, and plug Hub/Project surfaces into the Event Spine patch stream.
- Accessibility focus: bring Launcher runtime panels to parity (keyboard flows, ARIA labeling, high-contrast audits) and fold consent copy from the onboarding kit into shared help surfaces.
- Checks & optimizations: ensure SSE reconnection/backoff is captured in the universal right-sidecar and validate feedback deltas via snapshot tests.
- Documentation: update showcase/install paths and add the approval queue walkthrough once the sidecar panel is interactive.

### Security Hardening & Observability
- Immediate moves: stage the egress firewall scope manifest, wire capsule telemetry, expose the event journal reader and metrics registry endpoints, and land the GTK/GLib ≥0.20 supply-chain upgrade so we can drop the RUSTSEC ignore.
- Checks & optimizations: exercise DNS guard and proxy posture in CI; audit new endpoints with Spectral rules and Prometheus scrapes.
- Documentation: fold the guardrail posture presets into the security hardening guide, close the loop on supply-chain rollback notes, and keep Backlog observability rows in sync as we ship readers.

## User Outcomes & Storytelling

We keep the high-level promise (“your private AI control room”) tangible by committing to a handful of experience-first checkpoints alongside the technical work:

- **Trusted Onboarding Journey (Q4)** — deliver a narrated first-run flow and companion walkthrough deck that explains leases, consent, and local-first defaults in plain language; circulate with the beta cohort and fold feedback into Launcher help cards.
- **Consent UX Validation Sprint (Q4)** — run moderated sessions with three partner teams to confirm audio/vision consent dialogs and accessibility cues are comprehensible; publish a one-page findings brief and close the loop in Launcher/Docs copies.
- **Complexity Collapse Mission Brief (monthly)** — ship a short stakeholder update summarizing wins, risks, and upcoming user-facing moments; archive briefs under `docs/release_notes.md` so non-technical sponsors can track progress without scanning engineering backlogs.

These artifacts make sure the roadmap keeps aiming at confidence, clarity, and control for people—not just kernel correctness.

## Near-Term (Weeks)

#### [Kernel] Collapse the Kernel
- [Kernel] Kernel & Triad (complete): unified SQLite journal + CAS with triad API (`/actions`, `/events`, `/state`) is live; remain in stabilization mode while iterating.
- [Kernel] Stabilization window: limit to bug fixes, docs, tests, and internal cleanups; additive API changes only.
- [Kernel] Observability & Eventing: event journal tail/readers and metrics/docgen polish — see Backlog → Now.
- [Kernel] Security & Remote Access: hashed tokens, per-route gating, TLS profiles, proxy templates — see Backlog → Now.
- [Kernel] Egress Firewall (plan): add policy network scopes + TTL leases; per-node loopback proxy + DNS guard; route containerized scrapers first; egress ledger and pre-offload preview; default posture “Public only.”
- [Kernel] Lightweight mitigations (plan): memory quarantine; project isolation; belief-diff review queue; hardened headless browsing (disable SW/H3; same-origin); safe archive jail; DNS guard with anomaly alerts; secrets redaction; security posture presets.
- [Kernel] Asimov Capsule Guard: lease-based capsules with runtime refresh hooks and telemetry are live; future tuning flows through Security Hardening backlog items.
- [Kernel] State & Episodes: observations/beliefs/intents/actions stores; episodes with reactive UI — see Backlog → Now.
- [Kernel] Services & Orchestration: hierarchy/governor services; queue leases and nack behavior — see Backlog → Now.
- [Kernel] Specs & Interop: AsyncAPI + MCP artifacts and /spec/* serving — see Backlog → Now.
- [Kernel] Legacy feature migration (Phases A–E): completed; see `docs/RESTRUCTURE.md` for the final summary and hand-off guidance.

#### [Pack: Collaboration] Calm collaboration surfaces
- [Pack: Collaboration] UI coherence & routing: canonical admin debug/UI endpoints; launcher open path alignment; SSE reconnection/backoff and status; universal right-sidecar across Hub/Chat/Training; command palette.
- [Pack: Collaboration] Docs & Showcase: gating keys docgen; packaging and installer polish — see Backlog → Next.
- [Pack: Collaboration] Visual capture: screenshot tool (OS-level) with optional window crop; OCR (optional); SSE events + thumbnails; sidecar Activity integration.
- [Pack: Collaboration] Recipes MVP: schema + gallery + runner (local-first, default-deny permissions).
- [Pack: Collaboration] Heuristic Feedback Engine: lightweight, near-live suggestions with guardrails — see Backlog → Now → Feedback Engine for concrete work items.
- [Pack: Collaboration] Human-in-the-loop staging: phase one shipped (queue + approvals). Next: evidence previews, posture-driven review modes, and SLA tooling.

#### [Pack: Research] Research & memory packs
- [Pack: Research] Logic Units (config-first): manifest/schema, Library UI with diff preview, apply/revert/promote, initial sample units.
- [Pack: Research] Research Watcher (read-only): ingestion + read-models live. Next: scoring heuristics, richer payload renderers, and cross-install subscriptions.
- [Pack: Research] Training Park telemetry: baseline `/state/training/telemetry` is live; next deliver richer metrics and upgrade the launcher view from stub to live controls.

## Mid-Term (1–2 Months)

#### [Kernel] Collapse the Kernel
- [Kernel] WASI plugin sandbox: capability-based tools with explicit permissions.
- [Kernel] Policy engine integration: Cedar bindings; per-tool permission manifests.
- [Kernel] Model orchestration: managed runtime supervisor (llama.cpp, ONNX Runtime, vLLM) with pooling, profiles, accelerator bundles (CUDA/ROCm/Metal/DirectML/CoreML/Vulkan), and GPU/CPU KV memory policy hints for long-context batching and prefix sharing.
- [Kernel] Tests: feature-gated HTTP oneshot tests; policy and capability contract tests.
- [Kernel] AsyncAPI + MCP artifacts: generate `/spec/asyncapi.yaml` and `/spec/mcp-tools.json` in CI; serve `/spec/*` endpoints.
- [Kernel] Policy hooks for feedback auto-apply decisions (shadow mode → guarded auto).
- [Kernel] JetStream durable queue backend with acks, delay/nack, and subject mapping (keep core NATS for fast lane); add peer/edge CAS with gated `by-digest` endpoints for tool artifacts and optional gossip in multi-host dev.
- [Kernel] Budgets/Quotas: optional allow-with-budgets with per-window counters persisted to state; deny precedence.

#### [Pack: Collaboration] Calm collaboration surfaces at scale
- [Pack: Collaboration] UI app to manage various project types.
- [Pack: Collaboration] Voice & Vision Studio: consent-first audio/video capture, managed Whisper.cpp & llava.cpp adapters, live transcription/description panels.
- [Pack: Collaboration] Regulatory Provenance Unit (RPU): trust store, signature verification, Cedar ABAC for capsule adoption, hop TTL/propagation, adoption ledger (ephemeral by default).

#### [Pack: Research] Research & memory packs
- [Pack: Research] Capsules: record inputs/outputs/events/hints; export/import; deterministic replay.
- [Pack: Research] Runtime claims for multimodal autonomy: `runtime.claim.*` events, accelerator-aware fallbacks, budget-driven prompt cache warm-up.
- [Pack: Research] Dataset & memory lab: local pipelines, tags, audits, and reproducible reports.
- [Pack: Research] Commons Kit: ship 5 public-goods recipes with signed index and exportable memories.
- [Pack: Research] Logic Units v2: scripted transforms (sandboxed) and plugin units (with contract tests); policy-gated installation; compatibility matrix.
- [Pack: Research] Cluster trust (plan): node manifest pinning; mTLS; event sequencing and dedupe keys; scheduler targets only trusted manifests.
- [Pack: Research] Remote core connections (secure multi-node): mTLS between nodes/connectors and a remote coordinator with certificate rotation, NATS TLS profiles and client auth options for WAN clusters (local default remains plaintext loopback), policy-gated remote admin surface with proxy headers validation, and optional IP allowlists.

#### [Pack: Federation] Federated clustering MVP
- [Pack: Federation] Remote runner (one extra box): register Worker, accept jobs, stream results; enforce policies/budgets at Home.
- [Pack: Federation] Cluster Matrix + scheduler: show nodes; route simple offloads (long-context inference, heavy tools); per-node queues.
- [Pack: Federation] Live session sharing: invite guest with roles (view/suggest/drive); staging approvals remain on Home.
- [Pack: Federation] Egress ledger + previews: show payload summary/cost before offload; record to ledger.
- [Pack: Federation] Content-addressed models: Workers announce hashes; Home instructs fetch from allowed peers or registry; verify on load.
- [Pack: Federation] World diffs: export “public beliefs” with provenance; review conflicts on import.
- [Pack: Federation] Contribution meter + revenue ledger: track contributions per node; settlement report (CSV) with clear math.
- [Pack: Federation] Minimal broker (optional): tiny relay/directory for NAT-tricky cases; stateless/replaceable.

## Kernel Hardening

### Guiding Initiatives

#### Performance Guardrails
The stack scales by refusing to recompute or resend the same work twice and by bounding how much memory, CPU, or bandwidth each layer may consume. See [Architecture → Performance Guardrails](architecture/performance.md) for implementation details and operating guidance.

- **Prompt reuse for inference** keeps llama.cpp KV blocks on disk and plans vLLM prefix/KV sharing so repeated scaffolds skip token recompute, bounding GPU minutes per task.
- **Action Cache (Bazel-style)** deduplicates deterministic tool calls via hashed inputs and a CAS-backed store; fronted by a W-TinyLFU cache with TTL and capacity caps so CPU time and disk grow only within declared budgets.
- **Digest-addressed HTTP caching** serves model blobs and other immutable artifacts by sha256 with strong validators, keeping bandwidth predictable and capping repeated egress.
- **Request coalescing** applies singleflight around identical tool invocations and heavy reads, collapsing surges so concurrency stays within worker limits instead of stampeding.
- **Read-model SSE deltas** stream RFC-6902 patches with burst coalescing and Last-Event-ID resume so dashboards stay live while network and client JSON patching stay bounded.
- **Semantic and negative caches (planned)** keep per-project Q→A matches plus "no hit" markers with verifier gates, reducing redundant inference while privacy scopes and eviction policies pin their memory footprint.
- **Tiered storage & compression** layers in-memory caches with RocksDB and optional flash tiers, pairing Zstd dictionaries for small JSON blobs so hot data stays fast without unbounded disk churn.
- **Instrumentation & policy manifests** publish hit ratios, latency savings, and suppression counters in `/state/*` and `/metrics`, then feed declarative cache policy files that enforce privacy scopes and fallbacks before limits are exceeded.

### Recently Shipped (Sep 2025)
- [Kernel] Stability baseline (v0.1.0-beta): consolidation freeze, clippy-clean core, docs freeze checklist, CHANGELOG + release script.
- [Kernel] Optional gRPC server for the unified stack (tracked under Services & Orchestration).
- [Kernel] CI hardening: cargo-audit, cargo-deny, CodeQL, Nix build/test, docs link-check (lychee), Windows Pester tests; concurrency cancellation.
- [Kernel] Containers & Ops: multi-stage Dockerfile (non-root), docker-compose, Helm chart (readiness/liveness, securityContext, optional PVC), Justfile helpers.
- [Kernel] Dev environment: Nix devshell, VS Code devcontainer.
- [Kernel] Repo hygiene: Dependabot for Cargo and Actions; .gitattributes for line endings.
- [Kernel] Persistence hardening: atomic JSON/bytes writes with per-path async locks; best-effort cross-process advisory locks; audit log rotation.
- [Kernel] Event bus upgrades: counters (published/delivered/lagged/no_receivers), configurable capacity/replay, lag surfaced as `bus.gap`, subscribe-filtered API, SSE replay and prefix filters, kernel-backed journal tail (`/admin/events/journal`), Prometheus `/metrics`.
- [Kernel] Episodes & State: live read-models under `/state/*` (observations, beliefs, world, intents, actions, episodes) with corr_id stitching, duration and error rollups; Episodes panel with filters and details in Debug UI. The `world` view is a scoped belief graph (Project Map) built from the event stream with a selector endpoint for top-K beliefs.
- [Kernel] Resources pattern: unified AppState with typed `Resources`; moved Governor/Hierarchy/Memory/Models logic behind services; endpoints prefer services while preserving behavior.
- [Kernel] Tests + Lint: fixed flaky gating contract tests (serialized shared state); workspace clippy clean with `-D warnings`.

### Caching & Performance
- [Kernel] Inference-level: enable llama.cpp prompt cache; plan vLLM prefix/KV reuse when we add that backend.
- [Kernel] Exact CAS HTTP caching: strong validators and long-lived `Cache-Control` for immutable model blobs served by sha256.
- [Kernel] Action Cache (Bazel-style): deterministic keys (tool id, version, canonical input, env signature) → CAS’d outputs; in-memory front (W-TinyLFU), disk CAS backing.
- [Kernel] Request coalescing: singleflight on identical tool calls and expensive reads to prevent stampedes.
- [Kernel] Read-models over SSE: stream JSON Patch deltas with Last-Event-ID resume; avoid snapshot retransmits.
- [Kernel] Semantic caches (design): per-user/project Q→A cache with verifier; negative cache for retrieval misses; SimHash prefilter.
- [Kernel] Storage: RocksDB tiers for persistent caches; optional flash secondary cache; Zstd dictionaries for small JSON blobs.
- [Kernel] Measurement: layer hit ratios, latency/bytes saved, stampede suppression, semantic false-hit rate; expose in `/state/*`.
- [Kernel] Cache Policy: author a declarative cache policy manifest and loader; map to current knobs incrementally; document fallbacks and scope privacy defaults.

### Complexity Collapse Initiative
- [Kernel] Collapse surfaces to one API (`/state`, `/events`, `/actions`), one SQLite journal + content-addressed blobs, one job scheduler, one plugin ABI, and a shared UI sidecar/form builder.
- [Kernel] Schema-first patches for Project/AgentProfile/Policy/etc. with a documented event taxonomy; flows and errors modeled as data.
- [Kernel] Unified engines: retrieval pipeline, memory abstraction, runtime/cluster matrix, and capability/lease security.
- [Kernel] Goal: keep the kernel tiny; push features into declarative packs and reusable executors.

### Guiding Principles
- [Kernel] Local-first, open, privacy-respecting, and comprehensible.
- [Kernel] Calm defaults; explicit opt-in for power features.
- [Kernel] One truth for schemas & keys (central registry); reproducibility over hype.

### MVP Acceptance Checks
- [Kernel] Logic Units: install/apply/revert with diff preview; events visible; snapshot records active units.
- [Kernel] Read-models: `/state/logic_units`, `/state/experiments`, `/state/runtime_matrix`, `/state/episode/{id}/snapshot` respond.
- [Kernel] Evaluation: A/B dry-run flow produces metrics and renders in UI.
- [Kernel] Policy: permission prompts surface as leases; visible in sidecar and `/state/policy`.

## Optional Packs

### Pack: Collaboration

#### Recently Shipped
- [Pack: Collaboration] Debug/Launcher UIs: metrics quick-link, SSE presets (Replay 50, Models-only), insights wired to route stats, download progress.
- [Pack: Collaboration] Workflow Views: universal right-sidecar across Hub/Chat/Training with Timeline/Context/Policy/Metrics/Models; command palette; Compare panels (Text/JSON, Image, CSV) in Hub; Chat A/B pin-to-compare; Events window presets + filters; Logs focus mode and route filter; P95 sparklines.
- [Pack: Collaboration] TypeScript client v0.2.0: Node-friendly SSE (Last-Event-ID + replay) and `arw-events` CLI for tailing events with resume.

#### Long-Term (3–6 Months)
- [Pack: Collaboration][Future] Ledger-driven settlement tooling: contribution meter and revenue ledger mature into auditable exports for collaborators, and opt-in policy templates help teams review disputes locally without a separate governance simulator.

### Pack: Autonomous Operations

#### Planning
- [Pack: Collaboration][Future] Autonomy Lane Charter: document sandbox rules, budgets, and operator controls for autonomous helpers (see docs/spec/autonomy_lane.md).
- [Pack: Collaboration][Future] Autonomy governor: scheduler kill switch, pause/rollback UI, and telemetry tile ahead of Gate G4 trials.
- [Pack: Collaboration][Future] Autonomy rehearsal: synthetic workload drills, two-person sign-off, and Trial Control Center overlays before inviting real workloads.

### Pack: Research

#### Recently Shipped
- [Pack: Research] Docs: training research + wiki structure pages; gRPC guide; stability checklist; docgen updates; OpenAPI regeneration in CI.

#### Long-Term (3–6 Months)
- [Pack: Research][Future] Research-grade local stack: on-device accel (CPU/GPU/NPU), quantization, LoRA fine-tuning, model manifests, artifact signing/verification, SBOMs, dependency audits, and signed policy capsules with Sigstore that rely on the RPU trust store plus local timestamping (renegotiation on restart remains default).

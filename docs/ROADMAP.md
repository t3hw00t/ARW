---
title: Roadmap
---

# Roadmap

Updated: 2025-09-12

See also: [Backlog](BACKLOG.md) and [Interface Roadmap](INTERFACE_ROADMAP.md).
Roadmap highlights themes and timelines; Backlog tracks actionable items.

## Recently Shipped (Sep 2025)
- Stability baseline (v0.1.0-beta): consolidation freeze, clippy-clean core, docs freeze checklist, CHANGELOG + release script.
- Optional gRPC server for arw-svc (feature-flagged; ARW_GRPC=1).
- CI hardening: cargo-audit, cargo-deny, CodeQL, Nix build/test, docs link-check (lychee), Windows Pester tests; concurrency cancellation.
- Containers & Ops: multi-stage Dockerfile (non-root), docker-compose, Helm chart (readiness/liveness, securityContext, optional PVC), Justfile helpers.
- Dev environment: Nix devshell, VS Code devcontainer.
- Docs: training research + wiki structure pages; gRPC guide; stability checklist; docgen updates; OpenAPI regeneration in CI.
- Repo hygiene: Dependabot for Cargo and Actions; .gitattributes for line endings.
- Persistence hardening: atomic JSON/bytes writes with per‑path async locks; best‑effort cross‑process advisory locks; audit log rotation.
- Event bus upgrades: counters (published/delivered/lagged/no_receivers), configurable capacity/replay, lag surfaced as `Bus.Gap`, subscribe‑filtered API, SSE replay and prefix filters, optional persistent JSONL journal with rotation, Prometheus `/metrics`.
- Debug UI: metrics quick‑link, SSE presets (Replay 50, Models‑only), insights wired to route stats, download progress.
- Episodes & State: live read‑models under `/state/*` (observations, beliefs, world, intents, actions, episodes) with corr_id stitching, duration and error rollups; Episodes panel with filters and details in Debug UI. The `world` view is a scoped belief graph (Project Map) built from the event stream with a selector endpoint for top‑K beliefs.
- Resources pattern: unified AppState with typed `Resources`; moved Governor/Hierarchy/Memory/Models logic behind services; endpoints prefer services while preserving behavior.
- Tests + Lint: fixed flaky gating contract tests (serialized shared state); workspace clippy clean with `-D warnings`.

## Near‑Term (Weeks)
- Stabilization window: limit to bug fixes, docs, tests, and internal cleanups; additive API changes only.
- Observability & Eventing: event journal tail/readers and metrics/docgen polish — see Backlog → Now.
- Security & Remote Access: hashed tokens, per‑route gating, TLS profiles, proxy templates — see Backlog → Now.
- Egress Firewall (plan): add policy network scopes + TTL leases; per‑node loopback proxy + DNS guard; route containerized scrapers first; egress ledger and pre‑offload preview; default posture “Public only.”
- Lightweight mitigations (plan): memory quarantine; project isolation; belief‑diff review queue; hardened headless browsing (disable SW/H3; same‑origin); safe archive jail; DNS guard with anomaly alerts; secrets redaction; security posture presets.
- State & Episodes: observations/beliefs/intents/actions stores; episodes with reactive UI — see Backlog → Now.
- Services & Orchestration: hierarchy/governor services; queue leases and nack behavior — see Backlog → Now.
- Specs & Interop: AsyncAPI + MCP artifacts and /spec/* serving — see Backlog → Now.
- Docs & Showcase: gating keys docgen; packaging and installer polish — see Backlog → Next.
 - UI coherence: universal right‑sidecar across Hub/Chat/Training; command palette
 - Recipes MVP: schema + gallery + runner (local‑first, default‑deny permissions)
 - Logic Units (config‑first): manifest/schema, Library UI with diff preview, apply/revert/promote, initial sample units
 - Research Watcher (read‑only): draft Suggested units from curated feeds; human review flow

## Heuristic Feedback Engine
Scope: Lightweight, near‑live suggestions with guardrails.
See Backlog → Now → Feedback Engine for concrete work items.

## Complexity Collapse Initiative
- Collapse surfaces to one API (`/state`, `/events`, `/actions`), one SQLite journal + content-addressed blobs, one job scheduler, one plugin ABI, and a shared UI sidecar/form builder.
- Schema-first patches for Project/AgentProfile/Policy/etc. with a documented event taxonomy; flows and errors modeled as data.
- Unified engines: retrieval pipeline, memory abstraction, runtime/cluster matrix, and capability/lease security.
- Goal: keep the kernel tiny; push features into declarative packs and reusable executors.

## Mid‑Term (1–2 Months)
- UI app to manage various project types.
- WASI plugin sandbox: capability‑based tools with explicit permissions.
- Policy engine integration: Cedar bindings; per‑tool permission manifests.
- Model orchestration: adapters (llama.cpp, ONNX Runtime) with pooling and profiles.
- Capsules: record inputs/outputs/events/hints; export/import; deterministic replay.
- Dataset & memory lab: local pipelines, tags, audits, and reproducible reports.
 - Commons Kit: ship 5 public‑goods recipes with signed index and exportable memories.
 - Logic Units v2: scripted transforms (sandboxed) and plugin units (with contract tests); policy‑gated installation; compatibility matrix
- Tests: feature‑gated HTTP oneshot tests; policy and capability contract tests.
- AsyncAPI + MCP artifacts: generate `/spec/asyncapi.yaml` and `/spec/mcp-tools.json` in CI; serve `/spec/*` endpoints.
- Policy hooks for feedback auto‑apply decisions (shadow mode → guarded auto).
- Cluster trust (plan): node manifest pinning; mTLS; event sequencing and dedupe keys; scheduler targets only trusted manifests.
- Regulatory Provenance Unit (RPU): trust store, signature verification, Cedar ABAC for capsule adoption, hop TTL/propagation, adoption ledger (ephemeral by default).
- JetStream durable queue backend with acks, delay/nack, and subject mapping (keep core NATS for fast lane).
- Remote core connections (secure multi‑node):
  - mTLS between nodes/connectors and a remote coordinator; certificate rotation strategy.
  - NATS TLS profiles and client auth options for WAN clusters; local default remains plaintext loopback.
  - Policy‑gated remote admin surface; proxy headers validation; optional IP allowlists.
- Budgets/Quotas: optional allow-with-budgets with per-window counters persisted to state; deny precedence.

### Federated Clustering (MVP Path)
- Remote runner (one extra box): register Worker, accept jobs, stream results; enforce policies/budgets at Home.
- Cluster Matrix + scheduler: show nodes; route simple offloads (long‑context inference, heavy tools); per‑node queues.
- Live session sharing: invite guest with roles (view/suggest/drive); staging approvals remain on Home.
- Egress ledger + previews: show payload summary/cost before offload; record to ledger.
- Content‑addressed models: Workers announce hashes; Home instructs fetch from allowed peers or registry; verify on load.
- World diffs: export “public beliefs” with provenance; review conflicts on import.
- Contribution meter + revenue ledger: track contributions per node; settlement report (CSV) with clear math.
- Minimal broker (optional): tiny relay/directory for NAT‑tricky cases; stateless/replaceable.

## Long‑Term (3–6 Months)
- Community training interface/simulation:
  - Online opt‑in interface; privacy‑preserving local preprocessing.
  - Metrics for “interaction quality” (clarity, helpfulness, faithfulness, novelty).
  - Value alignment via debate/consensus rounds; transparent rationale graphs.
  - Weighted participation (democratic/liquid/interest‑group based).
- Governance & decision systems:
  - Composable priorities; dynamic hierarchies; fairness and safety constraints.
  - Argument mapping, counterfactual sandboxing, and policy proofs.
- Research‑grade local stack:
  - On‑device accel (CPU/GPU/NPU), quantization, LoRA fine‑tuning, model manifests.
  - Artifact signing/verification, SBOMs, and dependency audits.
  - Signed policy capsules with Sigstore; optional Bitcoin anchoring for timestamping (opt‑in; renegotiation on restart remains default).

## Guiding Principles
- Local‑first, open, privacy‑respecting, and comprehensible.
- Calm defaults; explicit opt‑in for power features.
- One truth for schemas & keys (central registry); reproducibility over hype.

## MVP Acceptance Checks
- Logic Units: install/apply/revert with diff preview; events visible; snapshot records active units.
- Read‑models: `/state/logic_units`, `/state/experiments`, `/state/runtime_matrix`, `/state/episode/{id}/snapshot` respond.
- Evaluation: A/B dry‑run flow produces metrics and renders in UI.
- Policy: permission prompts surface as leases; visible in sidecar and `/state/policy`.

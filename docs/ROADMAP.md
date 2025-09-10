Agents Running Wild — Roadmap
Updated: 2025-09-10
See [Interface Roadmap](INTERFACE_ROADMAP.md) for user-facing UI and tooling plans.

Near‑term (Weeks)
- Self‑learning UI polish: apply buttons per suggestion with rationale + confidence.
- Persist hints/profile/suggestions to state; reload at startup; simple rollback.
- Metrics polish: add p95 per route (light sliding window); highlight outliers in Insights.
- Models panel: download stub with progress; checksum verification; safe cancel.
- Security defaults: document token gating; add minimal rate‑limit for admin endpoints.
- Cluster MVP (done): pluggable Queue/Bus with local default; NATS queue groups; inbound NATS→local bus aggregator.
- Hierarchy foundation (done): local role/state + HTTP scaffolding for hello/offer/accept.
- Gating Orchestrator (done): central keys, deny contracts (role/node/tags, time windows, auto-renew), ingress/egress guards.
- Policy Capsules (done): wire format + header-based adoption (ephemeral, renegotiated on restart).
- Docgen: gating keys listing + config schema and examples.
- Docgen: surface route metrics/events in docs and status page.
- Showcase readiness: polish docs, packaging, and installer paths.

Heuristic Feedback Engine (Lightweight, Near‑Live)
- Engine crate: `arw-feedback` (actor + O(1) stats + deltas via bus).
- Signals: EWMA latency, decayed error rate, tiny P² p95 per route; memory ring pressure; download stalls.
- Evaluation: 250–500 ms ticks with cooldowns and bounds; suggestions only (manual apply default).
- State: snapshot published atomically; debounce persistence into `orchestration.json`; audit events.
- APIs: reuse existing `/feedback/*`; optional `/feedback/updates?since=` delta feed; expose evaluate/apply as tools.
- Safety: bounded queues/maps; drop/sample on overload; rate‑limit auto‑apply (opt‑in later, policy‑gated).

Mid‑term (1–2 Months)
- UI app to manage various project types.
- WASI plugin sandbox: capability‑based tools with explicit permissions.
- Policy engine integration: Cedar bindings; per‑tool permission manifests.
- Model orchestration: adapters (llama.cpp, ONNX Runtime) with pooling and profiles.
- Capsules: record inputs/outputs/events/hints; export/import; deterministic replay.
- Dataset & memory lab: local pipelines, tags, audits, and reproducible reports.
- Tests: feature‑gated HTTP oneshot tests; policy and capability contract tests.
- AsyncAPI + MCP artifacts: generate `/spec/asyncapi.yaml` and `/spec/mcp-tools.json` in CI; serve `/spec/*` endpoints.
- Policy hooks for feedback auto‑apply decisions (shadow mode → guarded auto).
- Regulatory Provenance Unit (RPU): trust store, signature verification, Cedar ABAC for capsule adoption, hop TTL/propagation, adoption ledger (ephemeral by default).
- JetStream durable queue backend with acks, delay/nack, and subject mapping (keep core NATS for fast lane).
- Budgets/Quotas: optional allow-with-budgets with per-window counters persisted to state; deny precedence.

Long‑term (3–6 Months)
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

Guiding Principles
- Local‑first, open, privacy‑respecting, and comprehensible.
- Calm defaults; explicit opt‑in for power features.
- One truth for schemas & keys (central registry); reproducibility over hype.

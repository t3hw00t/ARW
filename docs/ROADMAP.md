Agents Running Wild — Roadmap
Updated: 2025-09-07

Near‑term (Weeks)
- Self‑learning UI polish: apply buttons per suggestion with rationale + confidence.
- Persist hints/profile/suggestions to state; reload at startup; simple rollback.
- Metrics polish: add p95 per route (light sliding window); highlight outliers in Insights.
- Models panel: download stub with progress; checksum verification; safe cancel.
- Security defaults: document token gating; add minimal rate‑limit for admin endpoints.
- Docgen: surface route metrics/events in docs and status page.
- Showcase readiness: polish docs, packaging, and installer paths.

Mid‑term (1–2 Months)
- UI app to manage various project types.
- WASI plugin sandbox: capability‑based tools with explicit permissions.
- Policy engine integration: OPA/Cedar bindings; per‑tool permission manifests.
- Model orchestration: adapters (llama.cpp, ONNX Runtime) with pooling and profiles.
- Capsules: record inputs/outputs/events/hints; export/import; deterministic replay.
- Dataset & memory lab: local pipelines, tags, audits, and reproducible reports.
- Tests: feature‑gated HTTP oneshot tests; policy and capability contract tests.

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

Guiding Principles
- Local‑first, open, privacy‑respecting, and comprehensible.
- Calm defaults; explicit opt‑in for power features.
- One truth for schemas; reproducibility over hype.


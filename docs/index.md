---
title: Agent Hub (ARW)
---

# Agent Hub (ARW)

Your private AI control room that can scale and share when you choose.

In plain terms: Agent Hub (ARW) lets you run your own team of AI "helpers" on your computer to research, plan, write, and build — while laying the groundwork for upcoming voice and vision helpers — and stay in charge. It is local-first and privacy-first by default, with the option to securely pool computing power with trusted peers when a project needs more muscle.

ARW stands on four pillars: empathetic personas with transparent self-models, practical infinite-context memory overlays on any model, a fair autonomous economy for earning with policy-aligned agents, and universal access via eco defaults, offline bundles, and sustainability safeguards that keep participation free on consumer hardware.

Updated: 2025-10-30
Type: Explanation

Assistant quickstart: [Agent Onboarding](ai/AGENT_ONBOARDING.md)

## At a Glance

| Surface / Pack | Status | Notes |
| --- | --- | --- |
| Project Hub | Shipping | Primary workspace for projects, files, notes, and runs. |
| Chat | Shipping | Episode-first chat with the shared sidecar and evidence replay. |
| Training Park | Preview | Launcher telemetry and controls live; richer charts and adjustments landing next. |
| Remote collaborator packs | Preview (opt-in) | Federation, pooled compute, Guardrail Gateway; off until you enable them. |
| Future packs | Roadmap | Voice & vision studio, runtime supervisor automation, capsule guard extensions. |

## Why It’s Different
- You decide access: files, web, mic, and camera are off until you grant time‑limited permission.
- You can see and replay everything: sources, steps, tools used, and cost; snapshot any run and compare later.
- It grows with you: start on one laptop; federation preview lets invited machines help or co-drive under your supervision.
- Configurable, not brittle: “[Logic Units](architecture/logic_units.md)” are safe strategy packs you can A/B test, apply, and roll back.
- Modular by design: a cognitive stack of specialist agents keeps dialogue, memory, validation, and tooling aligned with full provenance.
- Persona-first interactions (preview): opt into an empathetic persona builder, review self-model updates, and tune vibe feedback so the agent grows with you transparently once you enable the persona preview flag.

## What You Can Do
- Turn messy folders, PDFs, and links into clean briefs, reports, or knowledge bases.
- Run focused research sprints with citations and comparisons.
- Watch sites or docs for changes and get actionable updates.
- Turn vague goals into concrete plans and next steps.
- Chat to explore data and export both answers and evidence.
- Point ARW at external llama.cpp hosts today; managed downloads and adapters for ONNX Runtime and vLLM remain on the roadmap.
- Prepare for voice, vision, and pointer tooling: consent-first audio/video capture, local narration, and high-trust automation are under active development. Follow the [Multi-Modal Runtime Plan](architecture/multimodal_runtime_plan.md) for status.
- Cultivate revenue-ready automations: combine federation previews, contribution ledgers, and recipe packs to earn with autonomous agents under your control.

## Scaling & Sharing (Opt‑In)
- Stay local-first by default; remote workers and co-drive remain off until you flip them on.
- **Preview** Pool compute to your GPU box or a trusted collaborator’s machine; offload heavy jobs under your rules and budget.
- **Preview** Live co-drive sessions: others can watch, suggest, or take the wheel with your approval; risky actions wait for your sign-off.
- **Preview** Clear boundaries: enable the [Guardrail Gateway](architecture/egress_firewall.md) proxy + ledger to preview egress and capture an audit trail.
- **Preview** Managed runtimes share accelerator capacity across collaborators with signed bundles and automatic fallbacks.
- **Future** Fair splits: meter GPU time, tokens, and tasks for transparent revenue sharing later.

> **Enable preview features** Add `[cluster]` with `enabled = true` to a config file the server already loads (for example `configs/default.toml`). If you store overrides elsewhere, export `ARW_CONFIG` or `ARW_CONFIG_DIR` so that file is discovered. Then set `ARW_EGRESS_PROXY_ENABLE=1` and `ARW_EGRESS_LEDGER_ENABLE=1` to record previews.

## Safety & Control
- Permission leases with timers and scopes; no silent escalation.
- A project [world view](architecture/object_graph.md) tracks key facts, open questions, and constraints so agents act on evidence, not guesses.
- Budgets for time, tokens, and spend; stay within plan with a visible meter.
- Signed plugins and sandboxed tools by default.
- Managed runtime supervisor (in progress) respects guardrails: accelerator access is lease-gated, telemetry is recorded, and fallbacks never break privacy promises as the supervisor matures toward full automation.

## Get Started
- [Quick Smoke](guide/quick_smoke.md): confirm the Rust toolchain with `verify --fast` before installing optional stacks.
- [Runtime Quickstart (Non-Technical)](guide/runtime_quickstart.md): step-by-step operator checklist (includes zero-auth mirrors and checksum guidance).
- [Persona Preview Quickstart](guide/persona_quickstart.md): enable the optional persona stack, seed an initial persona, and review consent notes before collecting vibe telemetry.
- [Quickstart](guide/quickstart.md): run ARW locally in minutes.
- [Feature Matrix](reference/feature_matrix.md): review what ships today and what’s planned.
- [Architecture overview](architecture/object_graph.md): follow the unified object graph and SSE stream.
- [Verify endpoints](guide/quickstart.md#verify-the-server): do a quick `/healthz` and `/about` check.

Tip: ARW treats context as a just‑in‑time working set assembled from layered memories. See [Architecture → Context Working Set](architecture/context_working_set.md) for how we keep prompts small, relevant, and explainable.

## Who It’s For
- People who want real help on real work without giving away their data.
- Independent builders who prefer practical, local tools that can scale when needed.
- Teams who want transparent collaboration, clear costs, and reproducible results.

## Non‑Goals
- Not a hosted cloud platform; no hidden network egress by default.
- Not a monolithic “one‑true‑agent” — compose via recipes and tools.

## Choose Your Path
- [Quick Smoke](guide/quick_smoke.md)
- [Run Locally](guide/quickstart.md)
- [Operate & Secure](guide/security_hardening.md) · [Deployment](guide/deployment.md) · [Admin Endpoints](guide/admin_endpoints.md)
- [Vision Runtime Preview](guide/vision_runtime.md)
- [Contribute](developer/index.md)

Related:
- [Experiments (A/B) & Goldens](guide/experiments_ab.md)
- [Debug UI Overview](guide/debug_ui.md)
- [Orchestrator CLI](guide/orchestrator_cli.md)
- [CLI Guide](guide/cli.md)
- [Spec Endpoints](reference/specs.md)
- [Automation Ops Handbook](ops/automation_ops.md)
- [Persona Telemetry](guide/persona_telemetry.md)

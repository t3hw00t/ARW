---
title: Managed Runtime Supervisor
---

# Managed Runtime Supervisor

Updated: 2025-10-09
Type: Blueprint
Status: Priority One

## Intent

Promote the runtime manager from proposal to a first-class kernel capability. The supervisor will discover, launch, and monitor local inference engines (text, audio, vision) while respecting ARW’s privacy posture, accessibility targets, and unified object graph. This document coexists with the focused [Managed llama.cpp Runtime](managed_llamacpp_runtime.md) blueprint; it adds cross-runtime orchestration requirements, maturity gates, and verification checkpoints.

## Current Implementation Snapshot

- View manifest schema: `spec/schemas/runtime_manifest.json`.

- `RuntimeSupervisor` (server) now ships with a built-in `process` adapter that can launch and monitor local binaries described in `configs/runtime/runtimes.toml`. See `spec/schemas/runtime_manifest.json` for the manifest schema.
- Restore attempts flow through the supervisor: restart budgets apply, health loops publish status back into `/state/runtime_matrix`, and the public API surfaces concrete errors when launches fail.
- Manifests can mark runtimes with `auto_start = true`; the server now schedules those restores automatically on boot without waiting for manual intervention. Removing the flag (or setting it to `false`) stops the runtime gracefully on the next reload.
- Manifests are optional; when none are present the registry still behaves as before, letting operators opt in incrementally while we continue wiring additional adapters (ONNX Runtime, vLLM, multimodal).
- `/state/runtime/bundles` now surfaces discovered bundle catalogs (including the preview `bundles.llama.json`, `bundles.vision.json`, and `bundles.audio.json` entries); operators can also inspect the same data via `arw-cli runtime bundles list` or force a rescan with `arw-cli runtime bundles reload`.
- Preview bundle catalogs live in `configs/runtime/bundles.llama.json`, `configs/runtime/bundles.vision.json`, and `configs/runtime/bundles.audio.json`; inspect available entries with `arw-cli runtime bundles list` (add `--remote` to query a running server, `--json/--pretty` for scripting). Artifact URLs remain placeholders until the signing pipeline lands.

## Guiding Principles

- **Stability first** – treat the supervisor as kernel infrastructure. Every adapter must warm-start predictably, surface health telemetry, and degrade gracefully without blocking `/actions`.
- **Accessibility matters** – Launcher controls, logs, and status toasts must expose non-visual cues (aria labels, transcripts, keyboard flow). Runtime decisions need human-readable explanations alongside icons.
- **Harmonized state** – runtime metadata lives in the unified object graph and reuse existing read-model patterns (`runtime_matrix`, `models`, `projects`). No bespoke side channels.
- **Policy-aligned** – guardrail settings, leases, and capsules follow runtime processes. Accelerator access, egress, and peripheral usage stay opt-in and auditable.

## Phased Delivery Plan

| Phase | Focus | Key Deliverables | Exit Criteria |
| --- | --- | --- | --- |
| 1. Runtime Matrix Stabilization | Harden the existing runtime heartbeat feed. | `runtime_matrix` read-model exposes health reasons, restart quotas, accessible status strings; Snappy budgets for runtime endpoints; CPU-only llama smoke tests | `/state/runtime_matrix` returns detailed health; CI job launches llama runtime twice (Linux/Windows) and asserts ready state; Launcher surfaces accessible status pill. |
| 2. Supervisor Core | Create the supervisor process, registry, and API. | `RuntimeRegistry` service, adapter trait, start/stop orchestrations, guardrail policy hooks, minimal `/runtimes/*` endpoints, structured logs | Integration tests cover crash restart, lease denial, and prompt-cache warm start; policy simulator reflects runtime capabilities; docs include operator checklist. |
| 3. Multimodal Expansion | Add vision-first adapters and consent workflows, then layer audio. | llava/Moondream adapters with Memory Fabric provenance, shared prompt cache manager, Launcher consent dialog with transcripts, pointer/keyboard parity actions, Whisper/Piper adapters follow once overlays/stability land | Accessibility review signed off (screen reader walkthrough + caption audit); instrumentation publishes modality-specific metrics; failure fallbacks degrade to text runtime without hanging jobs. |
| 4. Federation Hooks | Allow remote nodes to advertise and claim runtimes. | `runtime.claim.*` protocol, worker manifest extensions, ledger entries for GPU consumption, orchestrator scheduling hints | Remote worker demo passes health, refusal, and reclamation tests; ledger shows aggregated runtime usage by collaborator; policy asserts remote accelerator leases before jobs run. |

## Architecture Outline

1. **Runtime Registry (server)** – stores desired/runtime state in kernel CAS, emits patches over SSE, and enforces policy via Cedar. Builds on `apps/arw-server/src/runtime_matrix.rs` with new supervisor modules (`runtime/registry.rs`, `runtime/adapters/*`).
2. **Supervisor Daemon (launcher)** – cross-platform controller that installs binaries, supervises processes, and streams telemetry back to the server over gRPC/WebSocket. Respects Guardrail Gateway network posture.
3. **Adapter Contract** – shared trait for llama.cpp, ONNX Runtime, vLLM, Whisper, llava, etc. Each adapter declares capabilities (modalities, accelerator requirements, context limits) and boot steps. Reuse contract sketch from the llama.cpp blueprint.
4. **Profiles & Scheduling** – declarative profiles stored in `configs/runtime/profiles/*.toml`. Profiles map to accelerator strategies and prompt-cache options. Orchestrator jobs request profiles; supervisor resolves to available runtimes or queues fallbacks.
5. **Observability** – extend Snappy metrics (`runtime.*` family), add `/metrics` counters for restarts, warmups, and accelerator usage, and stream structured supervisor logs into the unified journal (`runtime.supervisor.log`). RuntimeRegistry now emits `arw::runtime` structured logs whenever payloads change or restore jobs run, including restart budget hints for operators while deduplicating identical heartbeats. SSE/read-model payloads expose both canonical slugs (`state`, `severity`) and their human labels (`state_label`, `severity_label`) so launchers and CLIs do not need to re-map enums locally.

## Stability & Test Strategy

- **Unit tests** – adapter contract conformance (mock process handles), config parsing, and policy gate evaluation.
- **Integration tests** – CI job subset that boots supervisor with llama.cpp CPU build, checks `/state/runtime_matrix`, and simulates crash recovery.
- **Hardware matrix smoke tests** – optional nightly job that runs GPU-enabled builds on hosted runners (CUDA, Metal) and collects throughput metrics.
- **Failure drills** – scripted tests that revoke leases mid-run, exhaust restart budgets, and validate guardrail-ledger entries.
- **Roll-forward plan** – supervisor ships disabled-by-default behind `ARW_RUNTIME_SUPERVISOR=1` until Phase 2 exit criteria pass; maintain migration guide for existing manual setups.

## Accessibility & UX Notes

- Status chips must include text labels (e.g., "Ready – GPU" / "Degraded – CPU fallback") and ARIA live regions for changes.
- Launcher command palette should expose keyboard shortcuts for start/stop, profile switch, and log view.
- Consent flows (audio/vision) require transcripts or textual summaries before enabling streaming.
- Document color contrast guidelines for new runtime panels; reuse design tokens defined in `assets/design/generated/tokens.theme.css`.

## Harmonization Checklist

- Update `interfaces/features.json` and regenerate Feature Matrix when phases complete.
- Keep `docs/ROADMAP.md`, `docs/FEATURES.md`, and Launcher help cards aligned with supervisor maturity tags.
- Publish API schemas (`/spec/runtime_supervisor.yaml`) once endpoints stabilize, mirroring the existing OpenAPI export flow.
- Ensure `spec/schemas/runtime_profile.json` covers new modalities and references shared enums to avoid drift.

## Open Questions

1. How do we balance auto-updates for bundled binaries against air-gapped environments? Potential answer: add policy flag `runtime.auto_update` with default `deny` unless signed capsules permit.
2. What is the user experience for attaching external runtimes (e.g., existing vLLM deployment)? Provide read-only “external adapter” mode that surfaces health but skips management.
3. Should we persist telemetry for historical analysis (e.g., throughput, errors)? If yes, design retention policy and privacy scope.
4. How do we expose cost estimates when federation shares GPU minutes? Needs integration with contribution ledger and budget alerts.

## Next Actions

1. Land Runtime Matrix enhancements (Phase 1) with accessible Launcher indicators — restart budgets now block auto-restores once the window is exhausted; stubbed llama smoke (`just runtime-smoke`) and the vision supervisor smoke (`just runtime-smoke-vision`) keep the pipeline honest while we wire real CPU/GPU builds.
2. Draft supervisor module scaffolding (`crates/arw-runtime`) and adapter trait tests.
3. Coordinate with Policy team to define accelerator lease keys and guardrail presets.
4. Produce operator runbook (checklist + rollback steps) before default-on rollout.

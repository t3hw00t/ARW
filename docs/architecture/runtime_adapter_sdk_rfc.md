---
title: Runtime Adapter SDK RFC (Draft)
---

# Runtime Adapter SDK RFC (Draft)
Updated: 2025-10-24
Status: Draft
Type: Proposal

## Summary
Define a lightweight adapter SDK that lets community runtimes (local binaries or remote services) register with the ARW RuntimeSupervisor without forking the core workspace. The SDK introduces a stable adapter trait, signed manifest schema, lint/smoke tooling, and documentation so operators can safely plug in llama.cpp forks, vLLM hosts, or REST gateways while preserving policy, telemetry, and consent guarantees.

## Goals
- Expose a minimal, well-documented Rust trait that adapters implement to add new runtime backends (text, vision, audio).
- Describe adapters declaratively via signed manifests so operators can review capabilities, resources, and consent annotations before activation.
- Provide scaffolding (CLI + template) that generates Ready-to-run adapters with proper logging, health reporting, and lease handling.
- Offer lint and smoke-test helpers so adapters can be validated offline before being loaded into `/state/runtime/*`.
- Keep adapters policy-aligned: capabilities, egress lanes, consent scopes, and restart budgets must honour Cedar policy decisions and Guardrail Gateway.

## Non-Goals
- Shipping curated binaries for every third-party runtime (the SDK focuses on integration, not distribution).
- Managing commercial licensing or paid model access (adapters can surface licensing metadata but enforcement remains external).
- Removing the existing curated bundles; those remain the fast path for default installs.

## Architecture Overview

```
              +-------------------------------+
              | Runtime Adapter Manifests    |
              |  (signed TOML/JSON)          |
              +---------------+--------------+
                              |
                              v
                 +---------------------------+
                 | Adapter Loader (SDK)      |
                 |  - manifest parser        |
                 |  - signature verification |
                 |  - lease inheritance      |
                 +---------------+-----------+
                                 |
        +------------------------+-------------------------+
        | Runtime Supervisor (existing)                    |
        |  - RuntimeRegistry                               |
        |  - Policy & consent checks                       |
        |  - Restart budgets / health matrix               |
        +------------------------+-------------------------+
                                 |
       +-------------------------+-------------------------+
       | Adapter Implementation (community crate)          |
       |  - Implements `RuntimeAdapter` trait              |
       |  - Uses SDK helpers for logging, telemetry        |
       |  - Calls backend binary / service                 |
       +--------------------------------------------------+
```

Adapters are regular Rust crates (workspace optional) that depend on the `arw-runtime-adapter` SDK. They advertise capabilities through a manifest and register themselves at runtime via `RuntimeRegistry::register_adapter`.

## Adapter Contract
- `RuntimeAdapter` trait covers lifecycle methods (`prepare`, `start`, `stop`, `health`, `capabilities`).
- Capabilities describe supported modalities, acceleration hints, context limits, and consent scopes.
- The SDK ships helper structs for token budgeting, guardrail headers, runtime events, and telemetry sinks.
- Adapters emit structured logs + metrics using the shared `tracing` targets (`runtime.adapter.<id>`).
- Error handling standardised via `AdapterError` enums (classified for supervisor retries vs terminal failures).

### Async Patterns
- All stateful adapters must be `Send + Sync`.
- `start` returns a stream handle implementing `AdapterSession` so supervisors can push prompts and receive events (tokens, tool calls, logs).
- The SDK provides a `spawn_instrumented` helper that wraps tokio tasks with automatic panic reporting and restart hints.

## Manifest Schema
- Stored in `spec/schemas/runtime_adapter_manifest.json`.
- Key fields: `id`, `version`, `modality`, `entrypoint` (crate + initializer fn), `binary_requirements`, `consent`, `capabilities`, `metrics`, `health`.
- Optional `resources` block lists GPU/CPU requirements, memory footprints, and external network needs.
- Signatures reuse the bundle signer registry (`configs/runtime/bundle_signers.json`). CLI helper `arw-cli runtime adapters sign|verify` mirrors bundle tooling.

## Tooling & CLI
- `arw-cli runtime adapters init <name>` scaffolds a new adapter crate with tests, manifest, and sample smoke.
- `arw-cli runtime adapters lint` validates manifests, signatures, and required fields.
- `arw-cli runtime adapters smoke` launches the adapter in-process with mock runtimes to verify lifecycle + metrics.
- Bundled examples cover:
  - `llamacpp` (thin wrapper around existing supervisor integration).
  - `vllm` (HTTP/OpenAI-compatible server).
  - `rest-proxy` (generic JSON API adapter with declarative mapping).

## Policy & Consent
- Adapters inherit the caller's lease scopes. SDK enforces `LeaseGuard` to ensure outgoing calls include egress capsules.
- Consent metadata is surfaced to Launcher (UI badges) and `/state/runtime_matrix` via the supervisor.
- Restart budgets + health status integrate into existing runtime matrix so operators see adapter failures alongside bundled runtimes.
- Ledger integration: adapters can emit `runtime.claim` events so shared GPU usage is tracked even for community runtimes.

## Migration Plan
1. Publish SDK crate (`crates/arw-runtime-adapter`) and manifest schema.
2. Port existing llama.cpp integration to the SDK to prove parity.
3. Ship CLI scaffolding + lint/smoke commands.
4. Document adapter authoring guide (How-to).
5. Invite pilot contributors to port vLLM + REST adapters; gather feedback.

## Open Questions
- Should adapters support dynamic capability updates (e.g., hot-swapping model lists) or require manifest re-registration?
- How strict should signature enforcement be for local-only adapters (opt-in relaxed mode?)?
- Where should long-running adapter-specific metrics live (per-adapter Prometheus registry or supervisor aggregated)?

## References
- [Managed Runtime Supervisor](managed_runtime_supervisor.md)
- [Managed llama.cpp Runtime](managed_llamacpp_runtime.md)
- [Multimodal Runtime Plan](multimodal_runtime_plan.md)

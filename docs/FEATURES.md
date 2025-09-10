Agents running wild — Features and Tracks
Updated: 2025-09-06.
Revision: 2025-09-10 (cluster/gating/hierarchy updates)

GOALS

Free & open universal agent interface/runtime with robust standards.

Rust-first core; opt-in plugins; thin UIs & integrations.

CORE

Interop: MCP (client/server), HTTP/WS (OpenAPI 3.1 + AsyncAPI).

Observability: OpenTelemetry traces/logs/metrics; event bus (local + NATS inbound aggregator); notification routing.

Gen‑AI semantic attributes (OTel) are feature‑flagged while the convention evolves.

Security & policy: Gating Orchestrator (central keys + deny contracts with role/node/tags/time/auto-renew), ingress/egress guards, Policy Capsules (propagatable); Regulatory Provenance Unit (planned) for signatures and ABAC adoption; OPA/Cedar; permission manifests; Sigstore releases; SBOM.

Extensibility: Rust static plugins + WASI/WASM dynamic plugins.

Agent runtime: orchestrator; pluggable Queue/Bus (local default; NATS queue groups; JetStream planned); tool protocol/adapters; selector UX/APIs for easy model/tool/profile switching; Run Capsules.

Memory: layered design; Memory Lab for dataset complexity/logic/abstraction experiments; Live Probe; conditional training.

Execution safety & config: sandboxed file/network allowlists; profiles (performance/balanced/power-saver/custom); secrets.

Browser module with citations; file loader/saver; logging/metrics.

Hardware & performance: robust capability discovery; governor w/ presets; CAS + model daemon for multi-actor concurrency.

Connections: central connection registry with link policies, health checks, rate limits, QoS hints, secret binding, tracing, and audit.
Hierarchy Orchestrator: roles (root/regional/edge/connector/observer), HTTP scaffolding for hello/offer/accept; topology events.

PLUGINS (first‑party, optional)

Search, Vision, Translation, Model Routing, Scheduler, Speech I/O.

NPU/accelerator bridges via ONNX Runtime EPs.

Image gen (Stable Diffusion, AUTOMATIC1111 API).

GitHub integration, aider bridge.

Desktop/WebPush notifications.

Windows Command Palette integration (Win-only).

Vector/IR adapters (FAISS, Parquet/Arrow).

Vector backends beyond SQLite/Parquet are Phase 2 items.

ADAPTERS

llama.cpp (GGUF; CPU/GPU/NPU offload).

ONNX Runtime (DirectML/CUDA/ROCm/OpenVINO/CoreML when present).

OpenAI-compatible HTTP; optional vendor shims if installed.

Notes: process‑mode first (e.g., llama.cpp server). Library‑mode later. Mandatory CPU fallback if accelerators are missing/unhealthy.

INTERFACES & INTEGRATIONS

VS Code extension, Command Palette, aider CLI, GitHub, MCP bridges.

COMPANION APPS (Rust)

Launcher (Tauri): settings, templates, scheduler, tray, notifications.

Debug UI (Tauri): event stream, probe overlays, training console, logs/metrics, graph, capsule replay.

Model Manager (Tauri): browse/download/manage/convert/quantize models; profiles & compatibility checks.

Connection Manager (Tauri): discover, create, tune, and control connections/links between services (HTTP, WS, MCP, local tools). Features: enable/disable, profiles, rate limits and concurrency, retry/backoff, QoS hints, health checks, tracing, policy checks, dry-run tests, emergency cut-off.

PROJECTS / WORKSTREAMS

Core framework, Launcher, Debug UI, Model Manager, Connection Manager.

Shopware 6 migration; Crypto/NEAR research; Command Palette feasibility; Local browsing with citations.

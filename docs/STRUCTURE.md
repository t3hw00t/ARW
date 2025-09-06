Agents running wild — Repository & Workspace Structure
Updated: 2025-09-06.

PRINCIPLES

Open protocols (MCP, OpenAPI, AsyncAPI), open observability (OTel), open policy (OPA/Cedar),
open signing (Sigstore), portable plugins (WASI), hardware-first performance, safe concurrency.

TOP-LEVEL
/ repo root
/Cargo.toml Cargo workspace
/README.md
/docs/ PROJECT_INSTRUCTIONS.md, FEATURES.md, STRUCTURE.md, API_AND_SCHEMA.md, MEMORY_AND_TRAINING.md, HARDWARE_AND_MODELS.md
/spec/ generated API catalogs + JSON Schemas; spec/README.md
/policy/ example policies (Rego/Cedar) + tests
/crates/ all Rust crates (core + plugins + adapters)
/apps/ CLI + Tauri apps (Launcher, Debug UI, Model Manager, Connection Manager)
/integrations/ VS Code extension, GitHub webhook sample, MCP examples
/examples/ end-to-end samples
/templates/ prompts & task templates
/configs/ default.toml, policy bindings, presets/*.toml
/scripts/ build, release, signing, SBOM
/tests/ integration tests
/.github/workflows/ CI (lint, build, test, docgen, spec publish, SBOM, cosign)

CORE CRATES
/crates/arw-spec/ JSON Schema types + validators (mirrors /spec)
/crates/arw-protocol/ request/response/capability types; ProblemDetails; paging; op ids
/crates/arw-events/ event bus, event types, routing (OTel correlation)
/crates/arw-core/ orchestrator, tool runtime, selectors, Run Capsules, validation
/crates/arw-browser/ deterministic fetch + citations
/crates/arw-memory/ memory API + default stores (file/SQLite)
/crates/arw-memory-lab/ experiment harnesses for memory datasets & abstractions
/crates/arw-probe/ live memory/data-model probes & overlays
/crates/arw-training/ conditional training flows (policy/consent gated)
/crates/arw-policy/ policy bindings (OPA/Cedar), permission manifests
/crates/arw-interop-mcp/ MCP client + server bridge
/crates/arw-otel/ OpenTelemetry wiring (OTLP exporters, resource attrs)
/crates/arw-wasi/ WASI/WASM plugin host (capability-based)
/crates/arw-tool-registry/ compile-time registry + macros
/crates/arw-tools-macros/ #[arw_tool] proc-macro for schema/runtime/doc generation
/crates/arw-docgen/ emits OpenAPI/AsyncAPI/MCP catalogs from registries
/crates/arw-introspect/ HTTP/WS endpoints for tool/event/schema catalogs
/crates/arw-hw/ hardware capability discovery (CPU/GPU/NPU, drivers, features)
/crates/arw-governor/ performance/power presets, dynamic tuning, reconfig
/crates/arw-cas/ content-addressable store (mmapped read-only sharing)
/crates/arw-modeld/ local model daemon (pooling, batching, leases)
/crates/arw-autotune/ benchmarking & config search (threads/offload/kv-cache/batch)
/crates/arw-model-manifest/ model metadata schemas + compatibility solver
/crates/arw-model-hub/ download/verify/cache models; hub integrations
/crates/arw-conn/ connection registry, connectors, link policies, diagnostics

ADAPTERS
/crates/adapters/arw-adapter-llama/ llama.cpp adapter (GGUF)
/crates/adapters/arw-adapter-onnxrt/ ONNX Runtime (DirectML/CUDA/ROCm/OpenVINO/CoreML)
/crates/adapters/arw-adapter-openai/ OpenAI-compatible HTTP adapter
Notes: process‑mode backends first (llama.cpp server). Library bindings later. ONNX Runtime is the preferred cross‑vendor accelerator path with CPU fallback.

PLUGINS
/crates/plugins/arw-plugin-search/ SearXNG/direct search
/crates/plugins/arw-plugin-vision/ image understanding
/crates/plugins/arw-plugin-translation/ translation
/crates/plugins/arw-plugin-scheduler/ repeatable jobs
/crates/plugins/arw-plugin-speech-io/ STT/TTS
/crates/plugins/arw-plugin-model-routing/ model selection policy
/crates/plugins/arw-plugin-sd-a1111/ Stable Diffusion (AUTOMATIC1111)
/crates/plugins/arw-plugin-github/ GitHub ops + webhook verifier
/crates/plugins/arw-plugin-aider/ aider CLI bridge
/crates/plugins/arw-plugin-notify-desktop/ desktop notifications
/crates/plugins/arw-plugin-notify-webpush/ WebPush/webhooks
/crates/plugins/arw-plugin-win-palette/ Windows Command Palette integration
/crates/plugins/arw-plugin-faiss/ FAISS vector adapter (optional)
/crates/plugins/arw-plugin-parquet/ Arrow/Parquet indexing (optional)

APPS
/apps/arw-cli/ CLI (selectors, run, test, capsule)
/apps/arw-launcher/src-tauri/ Tauri launcher (settings, scheduler, notifications)
/apps/arw-launcher/ui/ static or Rust→WASM UI
/apps/arw-debug-ui/src-tauri/ Tauri debug UI (probe overlays, training console)
/apps/arw-debug-ui/ui/
/apps/arw-model-manager/src-tauri/ Tauri Model Manager (browse, download, convert, quantize, profiles)
/apps/arw-model-manager/ui/
/apps/arw-connection-manager/src-tauri/ Tauri Connection Manager (links, policies, diagnostics)
/apps/arw-connection-manager/ui/

WORKSPACE (excerpt)
/Cargo.toml

[workspace]
members = [
  "crates/arw-spec",
  "crates/arw-protocol",
  "crates/arw-events",
  "crates/arw-core",
  "crates/arw-browser",
  "crates/arw-memory",
  "crates/arw-memory-lab",
  "crates/arw-probe",
  "crates/arw-training",
  "crates/arw-policy",
  "crates/arw-interop-mcp",
  "crates/arw-otel",
  "crates/arw-wasi",
  "crates/arw-tool-registry",
  "crates/arw-tools-macros",
  "crates/arw-docgen",
  "crates/arw-introspect",
  "crates/arw-hw",
  "crates/arw-governor",
  "crates/arw-cas",
  "crates/arw-modeld",
  "crates/arw-autotune",
  "crates/arw-model-manifest",
  "crates/arw-model-hub",
  "crates/adapters/arw-adapter-llama",
  "crates/adapters/arw-adapter-onnxrt",
  "crates/adapters/arw-adapter-openai",
  "crates/plugins/arw-plugin-search",
  "crates/plugins/arw-plugin-vision",
  "crates/plugins/arw-plugin-translation",
  "crates/plugins/arw-plugin-scheduler",
  "crates/plugins/arw-plugin-speech-io",
  "crates/plugins/arw-plugin-model-routing",
  "crates/plugins/arw-plugin-sd-a1111",
  "crates/plugins/arw-plugin-github",
  "crates/plugins/arw-plugin-aider",
  "crates/plugins/arw-plugin-notify-desktop",
  "crates/plugins/arw-plugin-notify-webpush",
  "crates/plugins/arw-plugin-win-palette",
  "crates/plugins/arw-plugin-faiss",
  "crates/plugins/arw-plugin-parquet",
  "apps/arw-cli",
  "apps/arw-launcher/src-tauri",
  "apps/arw-debug-ui/src-tauri",
  "apps/arw-model-manager/src-tauri",
  "crates/arw-conn",
  "apps/arw-connection-manager/src-tauri"
]
resolver = "2"
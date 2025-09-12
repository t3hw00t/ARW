---
title: Project Instructions
---

# Project Instructions

Updated: 2025-09-06.

## Mission

Build a free & open, universal interface/runtime for intelligent agents.

Favor robust, open, widely available standards over bespoke mechanisms.

Keep low-level implementations and high-level features in harmony via single-source schemas.

## Principles

Openness: MCP (Model Context Protocol), OpenAPI 3.1, AsyncAPI, OpenTelemetry, OPA/Cedar policy, Sigstore, WASI.

Performance with safety: Rust-first core, policy-gated capabilities, reproducible runs.

Extensibility: static Rust plugins + dynamic WASI plugins; clear permission manifests.

Reliability: schema validation, structured errors (RFC 7807), CI contract tests, signed releases.

## Architecture Overview

Core: orchestrator, selectors, memory system, event bus, schemas/specs, policy, OTel, governor, hardware probes, connection registry, lightweight feedback engine.

Plugins: optional tools (search, vision, translation, routing, sched, speech, SD/A1111, GitHub, aider, notify, win-palette).

Adapters: llama.cpp, ONNX Runtime (DirectML/CUDA/ROCm/OpenVINO/CoreML), OpenAI-compatible HTTP; vendor shims optional.

Apps: CLI, Launcher (Tauri), Debug UI (Tauri), Model Manager (Tauri), Connection Manager (Tauri).

Integrations: VS Code extension, GitHub webhooks, MCP client/server examples.

Projects/Workstreams: live under /projects; do not couple to core.

## Interfaces & Docs

One source of truth: tool functions annotated with macros generate schemas + runtime + docs.

Generated artifacts: /spec/openapi.yaml, /spec/asyncapi.yaml (event streams including Feedback.*), /spec/mcp-tools.json, /spec/schemas/*.json.

Introspection endpoints expose tool catalogs and schemas at runtime.

## Security & Policy

Policy-first (OPA/Rego or Cedar): governs tools, data, network, training, governor profiles, and connection/link permissions.

Permission manifests per tool. Signed releases (cosign). SBOM (SPDX/CycloneDX).

## Memory & Training

Layered memory (ephemeral/episodic/semantic/procedural), pluggable stores.

Live Memory Probe (visibility), conditional training with approvals, reproducible “Run Capsules”.

## Hardware & Performance

arw-hw probes CPU/GPU/NPU + drivers/features; arw-governor applies performance/balanced/power-saver presets.

arw-modeld provides pooled, concurrent model serving; arw-cas provides mmapped, content-addressable artifacts.

arw-autotune benchmarks and writes tuned profiles per device/model.

## Developer Workflow

Define tools with #[arw_tool] (schema, runtime, docs from one function). Expose feedback evaluate/apply as tools for MCP/HTTP parity.

Validate inputs → check policy → invoke → emit events → return structured results.

Keep semver discipline on operation schemas; docs and clients are generated in CI.

## Connection Manager (New Companion App)

Purpose: discover, create, tune, and control connections/links between services (HTTP, WebSocket, MCP, local tools).

Controls: on/off toggles, profiles (strict/normal/lenient), rate limits and concurrency, retry/backoff, QoS hints.

Security: bind auth/secrets, apply per-connection policy, audit changes.

Diagnostics: health checks, latency/error charts, tracing links to OTel spans.

Actions: quick test, dry-run policy check, emergency cut-off.

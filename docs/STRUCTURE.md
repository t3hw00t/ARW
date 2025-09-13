---
title: Repository & Workspace Structure
---

# Repository & Workspace Structure
{ .topic-trio style="--exp:.8; --complex:.4; --complicated:.3" data-exp=".8" data-complex=".4" data-complicated=".3" }

Updated: 2025-09-07

See also: [Glossary](GLOSSARY.md)

## Principles

Open protocols (MCP, OpenAPI, AsyncAPI), open observability (OTel), open policy (OPA/Cedar),
open signing (Sigstore), portable plugins (WASI), hardware-first performance, safe concurrency.

## Component Categories

- **System / Host**: OS interfaces, hardware discovery, log and state directories.
- **Core Project**: workspace crates providing protocols, orchestration, telemetry, and CLI/service binaries.
- **External Dependencies**: third-party libraries (Tokio, Axum, Serde, tracing, etc.).
- **Core Plugins**: none shipped yet; reserved for built-in capabilities.
- **Plugin Extensions**: adapters or community plugins that may live under `crates/` in the future.

## Top-Level Layout

```
/              repo root
├─ Cargo.toml  workspace manifest
├─ README.md   project overview
├─ docs/       guides, roadmap, and reference material
├─ crates/     core Rust crates
├─ apps/       binaries and services
├─ scripts/    build and packaging helpers
└─ configs/    default configuration files
```

## Core Crates

- `crates/arw-protocol` – request/response types and capability schemas.
- `crates/arw-events` – event bus and tracing hooks.
- `crates/arw-core` – orchestrator and runtime utilities.
- `crates/arw-otel` – OpenTelemetry wiring.
- `crates/arw-macros` – compile-time helpers and procedural macros.

## Apps

- `apps/arw-cli` – command‑line interface for running and testing tools.
- `apps/arw-svc` – user‑mode HTTP service with a minimal debug UI.
- `apps/arw-launcher/src-tauri` – Tauri-based launcher (tray + windows: Events, Logs, Debug). Preferred cross‑platform companion app.

## Integration Crates

- `crates/arw-tauri` – shared glue for Tauri apps (service control, prefs, window openers).

---
This structure keeps the project portable and highlights where future plugins or
extensions can hook in.

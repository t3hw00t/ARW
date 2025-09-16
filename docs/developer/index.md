---
title: Developer Guide
---

# Developer Guide

This workspace is designed to be clear and modular. Start with Structure for an overview of crates and apps. CI & Releases explains how we validate changes and package artifacts.

Updated: 2025-09-16
Type: Reference

## Key Ideas
- Single workspace with focused crates and clean boundaries.
- Inventory-based tool registration via `#[arw_tool]`.
- Observability ready: tracing everywhere, OTEL wiring optional.
- Open standards and coherence: Design Tokens SSoT, dot.case events, CloudEvents, and documented ADRs.

## Useful Commands
```bash
cargo install cargo-nextest
cargo build --workspace --all-targets --locked
cargo nextest run --workspace --locked
# Regenerate OpenAPI from code (writes spec/openapi.yaml)
(cd Agent_Hub && OPENAPI_OUT=spec/openapi.yaml cargo run -p arw-svc)
# Optional: regenerate static JSON preview for docs/static/openapi.json
python3 scripts/generate_openapi_json.py
```

## Design & Standards
- Design Theme & Tokens: [design_theme.md](design_theme.md)
- UI Kit (Launcher): [ui_kit.md](ui_kit.md)
- Open Standards: [standards.md](standards.md)
- ADRs: [adr/0001-design-tokens-ssot.md](../adr/0001-design-tokens-ssot.md), [adr/0002-events-naming.md](../adr/0002-events-naming.md)

## Desktop UI (Tauri 2)
- Launcher app: `apps/arw-launcher/src-tauri`.
- Integration plugin: `crates/arw-tauri`.
- See [Launcher](../guide/launcher.md) for capabilities/permissions and [Tauri API](tauri_api.md) for API usage and upgrade notes.

## Rolling Optimizations
- Clippy- and fmt-gated CI keeps code quality high.
- Release profiles enable thin LTO and tuned codegen units.
- Docs build on CI validates that the user manual and dev docs stay in sync.

## Self‑Knowledge & Feedback
- Route metrics middleware records hits, errors, and EWMA latency.
- The event bus feeds a small counter for live Insights.
- Feedback layer collects signals, analyzes suggestions, and applies safe changes.
- Sensitive endpoints are gated; see Security notes.

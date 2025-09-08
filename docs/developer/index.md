---
title: Developer Guide
---

# Developer Guide

This workspace is designed to be clear and modular. Start with Structure for an overview of crates and apps. CI & Releases explains how we validate changes and package artifacts.

Key ideas
- Single workspace with focused crates and clean boundaries.
- Inventory-based tool registration via `#[arw_tool]`.
- Observability ready: tracing everywhere, OTEL wiring optional.

Useful commands
```bash
cargo install cargo-nextest
cargo build --workspace --all-targets --locked
cargo nextest run --workspace --locked
```

Rolling optimizations
- Clippy- and fmt-gated CI keeps code quality high.
- Release profiles enable thin LTO and tuned codegen units.
- Docs build on CI validates that the user manual and dev docs stay in sync.

Self‑knowledge & feedback
- Route metrics middleware records hits, errors, and EWMA latency.
- The event bus feeds a small counter for live Insights.
- Feedback layer collects signals, analyzes suggestions, and applies safe changes.
- Sensitive endpoints are gated; see Security notes.

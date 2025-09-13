---
title: Agents Running Wild (ARW)
---

# Agents Running Wild (ARW)

Microsummary: Local‑first agent service and CLI with a unified object graph and one live event stream (SSE). Debug UI, Recipes, and Tools all look at the same state for a coherent experience. Stable entrypoint.

10‑second pitch
- Local‑first: runs offline by default; portable, per‑user state.
- Unified object graph: one source of truth across UI surfaces.
- Live events (SSE): a single stream powers Debug UI and tools.
- Observability: tracing/logging/metrics and an optional event journal.
- Recipes + Schemas: strategy packs with JSON Schemas and policy prompts.

Updated: 2025-09-12

Who is it for?
- Builders who want strong observability and local control.
- Teams piloting recipes/tools with explicit trust boundaries.
- Researchers exploring context, retrieval, and evaluation patterns.

Non‑goals
- Not a hosted cloud platform; no hidden network egress by default.
- Not a monolithic “one‑true‑agent” — compose via recipes and tools.

## What You Can Expect
- Simple, local-first service with a small, friendly UI.
- Tools registered via a macro and discovered at runtime.
- Clear packaging and portable state for easy sharing.

Start with Quickstart to run the service, then explore Features and the new Architecture pages (Object Graph, Events Vocabulary). You can also build and run the Desktop Launcher for an integrated tray + windows UI. When you’re ready to dive deeper, the Developer section explains the workspace and CI.

Context tip: ARW treats context as a just‑in‑time working set assembled from layered memories. See Architecture → Context Working Set for how we avoid running out of context while keeping prompts small and explainable.

Strategy tip: For predictable speed vs depth, see the Performance & Reasoning Playbook (Quick/Balanced/Deep/Verified) in the User Guide.

Tip: If you’re just trying ARW, the default paths are portable. You can switch between portable and system mode with a single environment variable.

## Choose Your Path

- Run Locally: Quickstart
- Operate & Secure: Security Hardening, Deployment, Admin Endpoints
- Contribute: Developer/Overview

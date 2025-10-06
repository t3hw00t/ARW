---
title: Interface Roadmap
---

# Interface Roadmap

Updated: 2025-09-22
Type: Reference

See also: [Roadmap](ROADMAP.md)

This roadmap consolidates user-facing interface concepts to guide a unified, accessible experience across the service, launcher, and debug tooling.

## Scope Badges

Interface milestones reuse the shared scope badges so interface contributors can see how work aligns with Complexity Collapse:

- `[Kernel]` — Hardens the runtime, policy, and journal so the “Collapse the Kernel” thrust stays minimal, dependable, and auditable.
- `[Pack: Collaboration]` — Optional collaboration, UI, and workflow packs that give calm surfaces and governance without bloating the kernel.
- `[Pack: Research]` — Optional research, experimentation, and memory packs that extend retrieval, clustering, and replay while staying pluggable.
- `[Pack: Federation]` — Optional federation packs that let multiple installs cooperate under shared policy, budgets, and accountability.
- `[Future]` — Bets incubating beyond the active quarter; they stay visible but outside the current Complexity Collapse execution window.

Badges can be combined (for example, `[Pack: Collaboration][Future]`) to show both the optional pack and that the work sits beyond the active delivery window.

## Short‑Term (0–3 Months)
- [Pack: Collaboration] Guided micro-tutorials for first-time features
- [Pack: Collaboration] Micro-satisfaction elements (light animations, haptic feedback)
- [Pack: Collaboration] Contextual launcher tray actions with recent commands
- [Pack: Collaboration] Inline doc hints and contextual tooltips
- [Pack: Collaboration] Command palette/quick-action menu in the launcher
- [Pack: Collaboration] Chat/Hub provenance panes showing validation outcomes, memory evidence, and tool audit trails (Modular Cognitive Stack phase 2)
 - [Pack: Collaboration] Canonical admin routes: `/admin/debug`, `/admin/ui/*` (legacy `/debug` alias removed)
 - [Pack: Collaboration] SSE robustness: status badges + auto-reconnect with backoff and Last-Event-ID resume
 - [Pack: Collaboration] Connections: per-connection admin token and open app windows for that base
 - [Pack: Collaboration] Schema-generated forms from OpenAPI for consistent parameter UIs

## Medium‑Term (3–6 Months)
- [Pack: Collaboration] Multi-modal input (voice, stylus, gesture)
- [Pack: Collaboration] Predictive autoflow suggestions
- [Pack: Collaboration] Progressive disclosure interface pattern
- [Pack: Collaboration] Tiled debug surface with rearrangeable panels
- [Pack: Collaboration] Live linting for macros and one-click tool sandboxing
- [Pack: Collaboration] Integrated monitoring/analytics modules
- [Pack: Collaboration] Voice/terminal parity for key commands
- [Pack: Collaboration][Future] Planner orchestration dashboards with agent lineage timelines (Modular Cognitive Stack phase 3)

## Long‑Term (6+ Months)
- [Pack: Collaboration][Future] Timeline-based activity stream with timeline scrubber
- [Pack: Collaboration][Future] Dynamic flow view and declarative recipe builder
- [Pack: Collaboration][Future] Real-time collaboration cues and shareable debug sessions
- [Pack: Collaboration][Future] Cross-device handoff of in-progress sessions
- [Pack: Collaboration][Future] Minimal offline docs packaged with app
- [Pack: Collaboration][Future] Inline node graph visualization of agent events
- [Pack: Collaboration] Comprehensive debug session export/import
- [Pack: Collaboration] Dynamic "what-if" runs branching from historical states

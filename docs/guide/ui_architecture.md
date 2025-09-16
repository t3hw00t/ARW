---
title: UI Architecture
---

# UI Architecture
Updated: 2025-09-16
Type: How‑to

Layout
- Left rail: Projects, Chat, Training, Agents, Models, Tools, Logic Units, Memories, Data, Runtimes, Policies, Containers, Hardware, Settings, Plugins, Docs
- Center canvas: the active perspective (a project board, chat thread, or training scene)
- Right sidecar (always on): per‑episode timeline (obs → belief → intent → action), streaming tokens, policy prompts/decisions, runtime/memory meters

Command Palette (Ctrl/Cmd‑K)
- Global search + actions across entities (e.g., “attach agent to project”, “grant net:example.com for 15m”, “open runtime matrix”).

Three primary perspectives
- Project Hub: tasks/routines, files, notes, agents, data, previous runs; “Attach Agent” binds profile + policy + memory mounts + runtime
- Chat: an episode viewer/controller bound to a project+agent; always shows “what will go into context” and the sidecar
- Training Park: impressionistic dials for instincts/priorities, retrieval diversity, tool success, hallucination risk; tweak and see state shifts live with a minimal probe chat

Managers (single truth)
- Agents/Models/Hardware/Permissions/Containers/Plugins manage inventories; Projects/Agents hold references only.

Observability
- Global status bar: active episodes, token/s, CPU/VRAM/NPU load, runtime health
- Per‑episode timeline: stages + token stream + tool I/O; inline errors

Anti‑patterns
- Separate, stateful managers not driven by events (causes drift)
- Blocking modal policy prompts (prefer inline prompts in sidecar)
- Hidden context assembly (always show why each piece is in the prompt)

Tauri integration
- Treat the service’s SSE/WS as the source of truth for live data; Tauri’s event bus is for UI‑level signals only.
- Gate OS integrations through ARW policy prompts; render prompts and decisions inline in the sidecar for continuity.

See also: UI Flows (ASCII) — guide/ui_flows.md; Workflow Views & Sidecar — guide/workflow_views.md; UI Architecture Options (ASCII) — architecture/ui_architecture_options.md.

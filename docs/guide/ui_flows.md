---
title: UI Flows (ASCII)
---

# UI Flows (ASCII)

Flowcharts for the main user interfaces. Complements UI Architecture and Workflow Views.

Updated: 2025-10-09
Type: How‑to

## Navigation Overview

```
               +--------------------+
               |    Desktop App     |
               |    Launcher (UI)   |
               +----------+---------+
                          |
             Windows menu |  Command Palette (Ctrl/Cmd-K)
                          |                     |
                          v                     v
   +---------------------+-------------------------------+
   |                      Main Views                     |
   |                                                     |
   |  +-------------+   +-----------+   +--------------+ |
   |  | Project Hub |   |   Chat    |   | Training Park| |
   |  +------+------+   +-----+-----+   +------+-------+ |
   |         |                |                 |        |
   +---------+----------------+-----------------+--------+
             |                |                 |
             v                v                 v
       Right Sidecar (global: Timeline | Context | Policy | Metrics | Models | Activity)

   Managers (single source of truth)
     - Agents    - Models    - Hardware    - Permissions    - Containers    - Plugins

Debug UI: /admin/debug (enable `ARW_DEBUG=1`)
```

Notes
- All views share one live SSE stream and read‑models; the sidecar shows the same state everywhere.
- Managers own inventories; projects/agents hold references only.
- Command palette screenshot actions (`ui.screenshot.capture`) publish `screenshots.captured`; chat buttons reuse the same pipeline so Activity and Gallery stay in sync.

## Typical Project Flow

```
Start → Launcher → Project Hub
  |        |
  |        +--> Create/Open Project
  |                |
  |                +--> Attach Agent (profile + policy + mounts + runtime)
  |                |        |
  |                |        +--> Start Chat (episode bound to project+agent)
  |                |        |        |
  |                |        |        +--> Observe sidecar timeline (tools, prompts, tokens)
  |                |        |        |
  |                |        |        +--> Compare outputs (Text/JSON diff, images, CSV)
  |                |        |
  |                |        +--> Run Routines / Tasks (from Hub)
  |                |
  |                +--> Files/Notes (tree digest/truncation badges, notes metadata, browse, edit, upload, open in editor)
  |
  +--> Command Palette → global search & actions (e.g., set editor, open runtime matrix)
```

## Projects Admin UI (HTTP)

```
Enable admin → Start server → Open /admin/ui/projects
  |
  +--> Create project  → folder + NOTES.md
  |
  +--> Browse tree     → breadcrumbs, filter, digest/truncation badges, open (OS/editor)
  |
  +--> Edit notes      → autosave option, inline Saved indicator, metadata (bytes, hash, modified, truncation hint)
  |
  +--> Upload files    → drag & drop (size‑guarded)

Endpoints (admin‑gated): /state/projects | /projects | /state/projects/{proj}/notes | /projects/{proj}/notes | /state/projects/{proj}/tree | /state/projects/{proj}/file | /projects/{proj}/file
```

See also
- How‑to → UI Architecture (layout, sidecar), Workflow Views & Sidecar, Projects UI
- Reference → Feature Catalog, Event Topics

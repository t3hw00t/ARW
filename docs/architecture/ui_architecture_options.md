---
title: UI Architecture Options (ASCII)
---

# UI Architecture Options (ASCII)

Suggested UI/UX architecture patterns and combinations for ARW. Each option includes an ASCII diagram and when to consider it.

Updated: 2025-09-16
Type: Explanation

## A) Desktop‑First (Tauri, Single SSE)

```
        Desktop App (Tauri)
   ┌───────────────────────────┐
   │  Windows + Command Palette│
   │  Universal Right Sidecar  │
   └───────────────▲───────────┘
                   │ HTTP/SSE
                   │
             ┌─────┴──────────┐
             │   arw‑server   │
             │  (Unified API) │
             └────────────────┘
                       │
                 Workers/Plugins
```

Best for: rich desktop ergonomics, OS integrations, low latency.
Trade‑offs: packaging + updates per OS; browser access requires deep links.

## B) Browser‑First SPA/PWA (Headless‑First Server)

```
   Browser SPA (PWA)
   ┌──────────────────────────┐
   │  Tabs: Hub | Chat | ... │
   │  Right Sidecar (virtual) │
   └──────▲───────────▲───────┘
          │           │
          │  TLS      │  Cache/Offline
          │  HTTP/SSE │  Service Worker
          v           v
      ┌───────────────────┐
      │    arw‑server     │
      └───────────────────┘
```

Best for: zero‑install, easy sharing via URL, PWA offline.
Trade‑offs: limited OS features; file system access via browser APIs only.

## C) Thin Launcher + External Browser

```
      Desktop Launcher (Tauri)
      ┌──────────────────────┐
      │  System tray + auth  │
      │  Deep‑links to web   │
      └───────────▲──────────┘
                  │
            open http://...  
                  │
   Browser SPA ◀──┴──▶ arw‑server (HTTP/SSE)
```

Best for: keeping installer small; reuse browser rendering; OS tray + auto‑start.
Trade‑offs: two surfaces to coordinate; window management delegated to browser.

## D) Multi‑Client: Desktop + Web + CLI/TUI (Same Event Stream)

```
 ┌───────────────┐  ┌──────────────┐  ┌──────────────┐
 │ Desktop (UI)  │  │  Web (SPA)   │  │  CLI / TUI   │
 └──────▲────────┘  └─────▲────────┘  └─────▲────────┘
        │  HTTP/SSE        │  HTTP/SSE        │  HTTP/SSE
        └──────────┬───────┴──────────┬──────┴──────────
                   v                  v                 v
                 ┌────────────────────────────────────────┐
                 │              arw‑server                │
                 └────────────────────────────────────────┘
```

Best for: flexibility; same read‑models power all clients.
Trade‑offs: UX parity can drift; keep “UX invariants” shared.

## E) Terminal‑First (TUI) + Minimal Web Debug

```
Terminal UI (ncurses) ──HTTP/SSE──▶ arw‑server
      ▲                                   │
      │                                   └──▶ Minimal /debug for inspectors
      └── Local scripts/automation
```

Best for: servers, SSH‑only environments, power users.
Trade‑offs: limited visualizations; images/compare need web fallback.

## F) Plugin Panels / Micro‑Frontends (Isolated)

```
    Host UI (Hub/Chat/Training)
    ┌───────────────────────────────┐
    │ Slots: Sidecar lanes & Tabs   │
    │                               │
    │  ┌────────┐  ┌────────┐       │
    │  │ Plugin │  │ Plugin │  ...  │
    │  │ Panel  │  │ Panel  │       │
    │  └────────┘  └────────┘       │
    └──────────▲─────────────────────┘
               │ sandboxed iframe/WebView + message bridge
               v
          Plugin runtime (scoped APIs, policy gates)
```

Best for: extensibility without forking the app; vendor integrations.
Trade‑offs: isolation/permissions model must be clear; versioned contracts.

## G) Remote‑First Control (Thin Client → Remote Cluster)

```
   Thin Client (Desktop/Web) ──TLS HTTP/SSE──▶ Remote arw‑server
            │                                      │
            │                                      └──▶ Local/peer workers
            └── Local cache & “preview before egress” prompts
```

Best for: teams, heavier jobs, shared GPU pool.
Trade‑offs: latency; stronger auth/egress UX required.

## Combine & Compose

- Sidecar layout: persistent right sidecar vs. hideable/undocked; per‑view presets.
- Windowing: single‑window with panes vs. multi‑window (Hub/Chat pop‑outs).
- Data: server read‑models (JSON Patch) vs. client cache + revalidation.
- Transport: SSE baseline; optional WebSocket for backpressure‑aware streams.
- Offline: PWA cache + action queue with conflict‑aware merges.
- Security: inline policy prompts; scoped leases; plugin panel sandboxes.

See also
- How‑to → UI Architecture — guide/ui_architecture.md
- How‑to → Workflow Views & Sidecar — guide/workflow_views.md
- How‑to → UI Flows (ASCII) — guide/ui_flows.md
- Explanations → UX Invariants — architecture/ux_invariants.md


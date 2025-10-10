---
title: Mascot Overlay
description: Floating desktop companion that mirrors service state and offers quick actions.
---

The Mascot Overlay is a small, transparent Tauri window that “lives” on your desktop. It mirrors core status (online, starting, error), provides gentle idle movement, and offers quick actions when needed — without getting in your way.

Features
- Click‑through by default so it never blocks your work; toggle interactions with Ctrl/⌘+D.
- Subtle idle animations (blink, breathe, float); respects OS reduced‑motion.
- Edge snap to monitor edges, and optional magnet snap to sides of other ARW windows.
- Context menu with quick actions: open Conversations, open Debug UI, start/stop service, open Logs.
- Live “magnet” preview while dragging with a gentle snap/bounce when locking into place.
- Ambient glow that shifts with status (ready/thinking/concern/error) for quick at-a-glance context.
- Quiet mode option to soften glow, idle motion, and snap feedback when you need fewer distractions.
- Compact mode shrinks the mascot into a minimal badge while preserving status tinting.
- Conversation responses trigger a streaming pulse and update the badge so you can see progress without opening the full chat.
- Independent launch mode for kiosk/control scenarios.

Usage
- Show from Home → “Show mascot overlay” (Advanced → Preferences).
- Tray → Windows → “Mascot (overlay)”.
- Keyboard
  - Ctrl/⌘+D: toggle click-through vs. draggable/interactive.
  - M: toggle the actions menu (when interactions are enabled).
  - Ctrl/⌘+K: open Command Palette → search “Mascot” for actions.
- Drag + hold: preview highlight shows the edge/corner you’ll snap to; release to dock with a short bounce.
- While dragging, the mascot announces the intended dock (“Docking left edge…”) and restores the prior hint after settling.
- During conversation streaming, the glow pulses and the compact badge switches to “Streaming” until the response completes.

Settings
- Enable/disable: Home → Preferences → “Show mascot overlay”.
- Idle intensity: Low / Normal / High.
- Click‑through by default: on/off.
- Snap to window surfaces: stick to other ARW windows after dragging.
- Quick open and toggle are also available via Command Palette (Ctrl/⌘+K).
- Actions menu also offers Dock left/right/bottom-right and Reset position shortcuts.
- Quiet mode toggle available in Preferences and the mascot menu.
- Compact mode toggle available in Preferences, the mascot menu, and the Command Palette.

Preferences (persisted)
- Namespace: `mascot`
- Keys
  - `enabled` (bool)
  - `intensity` (`low` | `normal` | `high`)
  - `clickThrough` (bool)
  - `snapWindows` (bool)
  - `quietMode` (bool)
  - `compactMode` (bool)

Independent launch
- Start the launcher in mascot‑only mode:

  - Linux/macOS: `ARW_MASCOT_ONLY=1 cargo run -p arw-launcher --features launcher-linux-ui`
  - Windows (PowerShell): `$env:ARW_MASCOT_ONLY=1; cargo run -p arw-launcher`

Accessibility
- Respects `prefers-reduced-motion: reduce` and disables animations.
- Uses ARIA roles for the actions menu and a live region for short status hints.
- Defaults to click‑through to avoid intercepting pointer events unintentionally.

Notes
- Snapping aligns to the nearest edge within ~30px after releasing the drag strip.
- When “Snap to window surfaces” is disabled, it falls back to monitor edge snapping.

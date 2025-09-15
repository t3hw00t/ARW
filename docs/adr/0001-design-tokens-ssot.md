---
title: ADR 0001: Single Source of Truth for Design Tokens
status: accepted
date: 2025-09-14
---
Updated: 2025-09-14

Context
- Tokens (colors, spacing, radii, shadows) were duplicated across docs and apps.
- We want consistent visual language, dark mode parity, and simple tooling.

Decision
- Create a single source of truth under `assets/design/` and sync to consumers.
- Formats: W3C tokens (`tokens.w3c.json`) + generated CSS (`tokens.css`) + JSON mirror (`tokens.json`).
- Consumers import `tokens.css`; launcher pages include it before `ui-kit.css` and `common.css`.
- CI enforces sync via `scripts/check_tokens_sync.sh`.

Consequences
- Consistency and easier evolution; token additions are centralized.
- Optional: add Style Dictionary pipeline to emit more targets.
- Risk: accidental drift if contributors edit synced copies; mitigated by CI check.


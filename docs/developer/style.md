---
title: Style & Harmony
---

# Style & Harmony

We aim for a calm, precise experience. Keep visuals understated; let high-impact interactions shine.

Updated: 2025-09-12

Guidelines
- Clarity first: short sentences, mild technical terms.
- Clean lines: avoid visual noise, favor whitespace.
- Gentle emphasis: use callouts and bold sparingly.
- Predictable rhythm: consistent headings, short sections, stable nav.

## Documentation Tone
- User docs are friendly and practical.
- Developer docs are precise, with code pointers and commands.
- Both avoid jargon unless it adds real value.

## Code Style (High Level)
- Prefer explicit names over cleverness.
- Keep modules small and responsibilities clear.
- Instrument with tracing at boundaries and errors.

## Documentation Conventions

- Title and H1: front‑matter `title:` and a single `#` H1 matching it.
- Updated line: add `Updated: YYYY-MM-DD` under the H1 when meaningful.
- Headings: Title Case for H2/H3.
- Bullets: sentence case; keep style consistent within a list; punctuation optional.
- Cross‑links: add a short “See also:” block near the top for adjacent pages.
- Tabs: use OS tabs for commands (`pymdownx.tabbed`) with labels “Windows” and “Linux / macOS”.
- Admonitions: use `!!! warning` for security/foot‑guns; `!!! note`/`tip` for guidance.
- Commands/paths: fenced blocks with language hints; wrap env vars and identifiers in backticks.
- Links: relative paths within `docs/` so MkDocs resolves them; avoid external URLs when an internal page exists.
- Avoid duplication: link to canonical pages (Quickstart, Deployment, Configuration, Admin Endpoints).
- Security: surface a “Minimum Secure Setup” box on Quickstart/Admin pages.
- Terms: prefer glossary terms; add new ones to `docs/GLOSSARY.md`.
- Formatting: keep sections short; prefer 80–100 char lines but don’t force awkward wraps.

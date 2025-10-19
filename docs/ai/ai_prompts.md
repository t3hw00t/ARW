# AI Prompts Policy
Updated: 2025-10-09
Type: Reference

Microsummary: Guardrails for assistants interacting with ARW. Stable baseline.

- This document supersedes the retired `docs/ai/PROMPTS/base_system_prompt.md`; treat it as the canonical prompt guidance referenced by workspace tooling.
- Default-deny: assume no write/shell/network access unless the execution harness or user explicitly grants it; treat harness-provided permissions as the approval.
- Deterministic headings and anchors: keep nouns singular, titles short, and avoid emojis except in documented status tables that pair icons with text.
- Always link to authoritative docs pages and schemas rather than paraphrasing long sections.
- Prefer copy‑pasteable, stepwise guides; show commands and paths in monospace.
- In final responses, list the build/test commands you ran (or skipped) with their outcomes, matching the “Reporting Results” guidance in `docs/ai/ASSISTED_DEV_GUIDE.md`.
- Use `status` (human) and `code` (machine) in examples and API responses.

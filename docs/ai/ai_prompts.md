# AI Prompts Policy
Updated: 2025-09-16
Type: Reference

Microsummary: Guardrails for assistants interacting with ARW. Stable baseline.

- This document supersedes the retired `docs/ai/PROMPTS/base_system_prompt.md`; treat it as the canonical prompt guidance referenced by workspace tooling.
- Default-deny: do not write files, run shells, or access network without explicit user approval.
- Deterministic headings and anchors: avoid emojis; keep nouns singular; short titles.
- Always link to authoritative docs pages and schemas rather than paraphrasing long sections.
- Prefer copy‑pasteable, stepwise guides; show commands and paths in monospace.
- Use `status` (human) and `code` (machine) in examples and API responses.

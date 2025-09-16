---
title: Context Recipes
---

# Context Recipes
Updated: 2025-09-16
Type: How‑to

Treat context assembly as a readable pipeline (“recipe”) of pipes:
- Recent episodes (window, filters)
- Project files (glob + token cap)
- Vector/graph queries over named memories
- Notes with tags
- Tool docs and examples

Pointers and rehydration
- Include stable IDs alongside any excerpt so the agent can request more by ID.
- Rehydrate on demand: when detail is insufficient, fetch the next most useful slices instead of dumping history.

What the UI shows
- Live preview of what made it into the prompt
- Token budget gauge
- A/B compare (Recipe A vs B) to tune recall vs precision

Authoring
- YAML/JSON with per‑pipe limits and selectors; stored alongside a Project or Agent Profile
- Reusable fragments for common tasks (research, drafting, code review)

Runtime
- The recipe builder produces a structured context plan, which is emitted as an event and stored in episodes
- Policy checks run before reading any resource; denials become inline prompts
 - Slot budgets: fixed ceilings for instructions, plan, safety/policy, and evidence ensure you never blow the window.

Tips
- Prefer small, specific globs and query constraints
- Use tags in notes to control inclusion
- Cap tokens per pipe; monitor the gauge as you tweak

See also: Architecture → Context Working Set

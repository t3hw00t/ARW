# AI Contracts
Updated: 2025-09-15
Type: Reference

Microsummary: Norms for function/tool specs: names, descriptions, args schema, side effects, permissions. Stable.

- Tool spec fields:
  - name: kebab‑case, stable.
  - description: concise imperative; first line ≤ 120 chars.
  - args_schema: JSON Schema draft 2020‑12 with `description` per field and examples.
  - side_effects: enumerate write/shell/network; include prompts shown to users.
  - permissions: explicit capabilities with rationale; default‑deny.
- Output contracts: prefer JSON objects with `status` (human) and `code` (machine) fields; include examples.
- Stability: tag tools and fields as Stable/Beta/Experimental; include deprecation path.


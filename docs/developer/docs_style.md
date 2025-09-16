# How We Write Docs (Diátaxis)
Updated: 2025-09-16
Type: Reference

Microsummary: Our docs follow Diátaxis — Tutorials, How‑to, Reference, Explanations — so readers and tools land on the right page. Stable.

- Tutorials: learning‑oriented, step‑by‑step journeys (e.g., Quickstart).
- How‑to guides: goal‑oriented, copy‑pasteable steps (e.g., models download, security hardening).
- Reference: accurate, dry listings (API, CLI, config, schemas).
- Explanations: background, tradeoffs, and rationale (architecture and concepts).

New content should declare its type, include a 4–6 line microsummary, and use stable, deterministic headings.

Why these standards
- Diátaxis helps readers and assistants land on the right page for their need (learn, accomplish, look up, understand).
- OpenAPI + JSON Schema make the service and artifacts discoverable to tools; less reverse‑engineering.
- Keep a Changelog + SemVer make upgrades predictable for humans and automation.
- Material for MkDocs + mike provide a fast, searchable, versioned site with little maintenance.
- SPDX clarifies licensing at file and repo levels for downstream users and scanners.

## Formatting Conventions

- Updated line: include `Updated: YYYY-MM-DD` near the top (the repo provides `scripts/stamp_docs_updated.py` to auto-stamp from git history).
- Title consistency: when front‑matter has `title:`, the first H1 must match it exactly.
- Status badge: place `Status: Planned|Experimental|Beta|Stable|Deprecated` on its own line; it renders as a colored pill.
- Type badge: place `Type: Tutorial|How‑to|Reference|Explanation` on its own line; it renders next to the status badge.
- Bulk-stamp helper: `scripts/stamp_docs_type.py` inserts Type based on file path; review and adjust if needed.
- Images/tables/code: no hard widths; the theme auto-handles overflow responsively.
- Links: keep as standard anchors (buttons are for actions only).

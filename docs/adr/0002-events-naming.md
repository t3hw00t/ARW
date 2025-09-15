---
title: ADR 0002: Event Naming — dot.case Only
status: accepted
date: 2025-09-14
---
Updated: 2025-09-14

Context
- Event kinds previously had mixed naming (CamelCase and dot.case).
- We normalized on dot.case for clarity and compatibility with common tooling.

Decision
- Adopt dot.case exclusively for event kinds (e.g., `models.download.progress`).
- Update publishers and listeners; docs and examples use normalized kinds.
- CI will reject CamelCase event kinds going forward.

Consequences
- Consistency across code, docs, and UIs; easier filtering/composition.
- Breaking change for legacy consumers; migrations documented in release notes.


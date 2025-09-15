# Assisted, Iterative Coding – Working Agreement
Updated: 2025-09-12
Type: Reference

Microsummary: Small, safe changes with a written PLAN → minimal DIFF → tests → docs. Default‑deny risky edits. Stable.

Purpose
- Make small, safe changes that keep ARW coherent and testable.
- Default‑deny risky edits (security, network, filesystem) unless explicitly in scope.

Loop (every change)
1) Propose a PLAN (files to touch, exact changes, risks, test/docs impact).
2) Get ACK on the PLAN (or self‑review if solo), then implement minimal DIFF.
3) Run checks (fmt, clippy -D warnings, nextest); summarize results.
4) Update docs + microsummaries; create a tight PR with acceptance notes.

Guardrails
- Stay within the declared scope; no opportunistic refactors.
- Preserve public interfaces unless the issue marks “breaking: yes”.
- Prefer additive changes; keep diffs < ~300 lines when possible.
- Keep logging/telemetry consistent with existing tracing.
- Write rustdoc on new items; add examples when possible.

Docs & discoverability
- Update the most relevant page under docs/ (Tutorial/How‑to/Reference/Explanations).
- When adding/altering schemas or endpoints, link them from Reference and include examples.

Interfaces (for context)
- Debug UI: `/debug`; state read‑models under `/state/*`; events via SSE.
- Schemas live under `spec/schemas` (e.g., `recipe_manifest.json`).

Commit & PR hygiene
- Conventional Commits: feat(scope): … | fix(scope): … | docs(scope): … | chore: …
- PR must include: PLAN, DIFF summary, Tests run/output, Docs impact, Out‑of‑scope.

Security posture
- Respect default‑deny boundaries for file write, shell, and network.
- If a change touches permissions/trust, call it out in “Risk” and “User impact”.

Acceptance checklist (paste in PR)
- [ ] PLAN approved / self‑reviewed
- [ ] Tests pass locally
- [ ] Docs updated (incl. microsummary)
- [ ] Scope respected; no incidental refactors
- [ ] Breaking changes documented (N/A if none)


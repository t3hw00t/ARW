# Assisted, Iterative Coding – Working Agreement
Updated: 2025-09-22
Type: Reference

Microsummary: Small, safe changes with a written PLAN → minimal DIFF → tests → docs. Default‑deny risky edits. Stable.

Purpose
- Make small, safe changes that keep ARW coherent and testable.
- Default‑deny risky edits (security, network, filesystem) unless explicitly in scope.

Loop (every change)
Exception: when a change qualifies as an "ease-of-use" shortcut in the CLI (e.g., trivial edits that the harness allows without formal planning), you may skip step 1; still call out assumptions in the response.
1) Propose a PLAN (files to touch, exact changes, risks, test/docs impact).
2) Get ACK on the PLAN (or self-review if solo), then implement minimal DIFF.
3) Run checks (fmt, clippy -D warnings, nextest); summarize results.
4) Update docs + microsummaries; create a tight PR with acceptance notes.

Lightweight path (ease-of-use)
- Use only when the execution harness or reviewer explicitly treats the work as a trivial shortcut (typo fix, single-line docs tweak, metadata bump).
- Keep the edit self-contained (<20 lines, single file unless otherwise stated) and avoid behavioral changes or new dependencies.
- Spell out the assumption in your response: call out that the lightweight path was used, list skipped checks, and justify why it is safe.
- If the change grows beyond a micro-edit at any point, fall back to the full loop above.

Harness alignment
- Treat execution-harness permissions (e.g., baseline workspace writes, shell access) as explicit approvals; do not block on requesting authorization that the harness already grants or forbids.
- When harness policy prevents a required check (tests, docs build), note the skipped item and rationale in the response instead of forcing the run.
- If harness instructions contradict this guide, follow the harness and document the deviation so reviewers understand the context.

Micro change checklist
- Confirm the edit scope stays within the lightweight limits (≤ 20 lines, single file, no behavior changes).
- Explicitly list any skipped checks (fmt, clippy, tests, docs) and why the skip is safe.
- Call out security/privacy impact (typically “none”) and any follow-up the reviewer should schedule.
- Link to or restate the source of the harness approval so future agents can validate the shortcut.

Guardrails
- Stay within the declared scope; no opportunistic refactors.
- Preserve public interfaces unless the issue marks "breaking: yes".
- Prefer additive changes; keep diffs < ~300 lines when possible.
- Keep logging/telemetry consistent with existing tracing.
- Write rustdoc on new items; add examples when possible.

Docs & discoverability
- Update the most relevant page under docs/ (Tutorial/How‑to/Reference/Explanations).
- When adding/altering schemas or endpoints, link them from Reference and include examples.

Interfaces (for context)
- Debug UI: `/admin/debug`; state read‑models under `/state/*`; events via SSE.
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

# Assisted, Iterative Coding – Working Agreement
Updated: 2025-10-09
Type: Reference

Microsummary: Small, safe changes with a written PLAN → minimal DIFF → tests → docs. Default‑deny risky edits. Stable.

Harness precedence
- Follow the execution harness or user instructions when they conflict with this guide; note the deviation in your response so the next agent has the same context.
- Lightweight exceptions granted by the harness (e.g., trivial edits that explicitly skip planning) are valid—call them out when you use them.
- Prefer the headless bootstrap (`scripts/dev.sh setup-agent` or `scripts\dev.ps1 setup-agent`) when you need a non-interactive setup; it pins `--headless --minimal --no-docs`, sets `ARW_DOCGEN_SKIP_BUILDS=1`, compiles debug binaries, and installs PyYAML so docgen skips release packaging while verification still works by default.

Purpose
- Make small, safe changes that keep ARW coherent and testable.
- Default‑deny risky edits (security, network, filesystem) unless explicitly in scope.

Loop (every change)
Exception: when a change qualifies as an "ease-of-use" shortcut in the CLI (e.g., trivial edits that the harness allows without formal planning), you may skip step 1; still call out assumptions in the response.
1) Propose a PLAN (files to touch, exact changes, risks, test/docs impact).
2) Get ACK on the PLAN (or self-review if solo), then implement minimal DIFF.
3) Run relevant checks (fmt, clippy -D warnings, nextest) when source code changes. For docs-only or textual config updates, call out the skipped checks and why they are safe.
4) Update docs + microsummaries; create a tight PR with acceptance notes.

Use `scripts/dev.{sh,ps1} verify` to run the standard fmt → clippy → tests → docs sequence; pass `--fast` when you only need fmt/clippy/tests and plan to handle docs or launcher UI checks separately. The command reports skipped checks so you can note them explicitly. Prefer the task workflow? `mise run verify` and `mise run verify:fast` map to those helpers after `mise install`.
Need a lighter docs lint for fast feedback? Run `DOCS_CHECK_FAST=1 bash scripts/docs_check.sh` (or pass `--fast`) to skip the mkdocs build and deep Python sweeps; follow up with the full run before merging when time permits. Prefer tasks? `mise run docs:check` and `mise run docs:check:fast` wrap those helpers.
Missing MkDocs or Python deps? `mise run bootstrap:docs` (or `bash scripts/bootstrap_docs.sh`) installs the pinned stack defined in `requirements/docs.txt`.
Need offline installs? Run `mise run docs:cache:build` ahead of time (or grab the `docs-wheels.tar.gz` asset from releases) and point `bootstrap_docs.sh` at the extracted wheel directory with `--wheel-dir`.

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
- Explicitly list any skipped checks (fmt, clippy, tests, docs) and why the skip is safe. Skipping fmt/clippy/nextest is acceptable for non-code edits.
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

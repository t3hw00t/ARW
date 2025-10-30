# Assisted, Iterative Coding – Working Agreement
Updated: 2025-10-20
Type: Reference

Microsummary: Small, safe changes with a written PLAN → minimal DIFF → fast, targeted verification → docs. Default‑deny risky edits. Stable.

Harness precedence
- Follow the execution harness or user instructions when they conflict with this guide; note the deviation in your response so the next agent has the same context.
- Lightweight exceptions granted by the harness (e.g., trivial edits that explicitly skip planning) are valid—call them out when you use them.
- Prefer the headless bootstrap (`scripts/dev.sh setup-agent` or `scripts\dev.ps1 setup-agent`) when you need a non-interactive setup; it pins `--headless --minimal --no-docs`, sets `ARW_DOCGEN_SKIP_BUILDS=1`, installs PyYAML, and skips the `arw-cli` build by default (pass `--with-cli` / `-WithCli` to opt back in) so docgen skips release packaging while verification still works out-of-the-box.
- Need the toolchain only? Append `--skip-build` / `-SkipBuild` so the helper hydrates dependencies without compiling immediately.

Purpose
- Make small, safe changes that keep ARW coherent and testable.
- Default‑deny risky edits (security, network, filesystem) unless explicitly in scope.

Loop (every change)
Exception: when a change qualifies as an "ease-of-use" shortcut in the CLI (e.g., trivial edits that the harness allows without formal planning), you may skip step 1; still call out assumptions in the response.
1) Propose a PLAN (files to touch, exact changes, risks, test/docs impact).
2) Get ACK on the PLAN (or self-review if solo), then implement minimal DIFF.
3) Run relevant checks (fmt, clippy -D warnings, nextest) when source code changes. For docs-only or textual config updates, call out the skipped checks and why they are safe.
4) Update docs + microsummaries; create a tight PR with acceptance notes.

Use `scripts/dev.{sh,ps1} verify` to run the standard fmt → clippy → tests → docs sequence. The headless default excludes the Tauri `arw-launcher` crate; pass `--with-launcher` / `-WithLauncher` (or set `ARW_VERIFY_INCLUDE_LAUNCHER=1`) when you specifically need the desktop UI checks. On Linux, cargo needs the UI feature explicitly—set `CARGOFLAGS="--features launcher-linux-ui"` for that shell before running with `--with-launcher`, then unset it when you return to headless work. Missing Node.js now auto-skips the launcher smoke instead of failing, and you can export `ARW_VERIFY_REQUIRE_DOCS=1` to treat absent Python/PyYAML as hard failures instead of informational skips. Add `--fast` / `-Fast` to skip doc sync, docs lint, and launcher UI tests when you only need fmt/clippy/test coverage. Need the full GitHub Actions coverage locally? Append `--ci` / `-Ci` to chain registry integrity checks, doc generators in `--check` mode, env-guard lint, triad/runtime/context smokes, and the legacy surface guard. Prefer the task workflow? `mise run verify`, `mise run verify:fast`, and `mise run verify:ci` map to those helpers after `mise install`.
New to the repo or working on a machine without the docs/UI stack yet? Follow [Quick Smoke](../guide/quick_smoke.md) for a scripted `scripts/dev.sh verify --fast` walkthrough before investing in the full toolchain.
Need a lighter docs lint for fast feedback? Run `DOCS_CHECK_FAST=1 python3 scripts/docs_check.py` (use `py -3` on Windows) (or pass `--fast`) to skip the mkdocs build and deep scans; follow up with the full run before merging when time permits. Prefer tasks? `mise run docs:check` and `mise run docs:check:fast` wrap those helpers.
Missing MkDocs or Python deps? `mise run bootstrap:docs` (or `bash scripts/bootstrap_docs.sh`) installs the pinned stack defined in `requirements/docs.txt`.
Need offline installs? Run `mise run docs:cache:build` or `scripts/dev.{sh,ps1} docs-cache` ahead of time (or grab the `docs-wheels.tar.gz` asset from releases) and point `bootstrap_docs.sh` at the extracted wheel directory with `--wheel-dir`.

### Selecting Builds & Tests
Plan the minimum set of checks before editing so you can state them in the PLAN and final status.

| Change scope | Minimum checks | Notes |
| --- | --- | --- |
| Docs-only, metadata, or comments | None required | Spell out the skip in your status (`docs-only; skipped fmt/clippy/tests`). Run `scripts/dev.{sh,ps1} verify --fast` if unsure about indirect effects. |
| Rust code limited to one crate or tool | `scripts/dev.{sh,ps1} verify --fast` **or** targeted `cargo fmt`, `cargo clippy --all-targets --all-features -p <crate>`, `cargo nextest run -p <crate>` | Prefer the wrapper if multiple crates appear in the PLAN; justify any targeted runs in the summary. |
| Cross-crate Rust changes, new features, or behavioral fixes | `scripts/dev.{sh,ps1} verify` | Adds docs lint + smoke coverage; escalate to `--ci` when touching registry integrity, runtime, or docs generators. |
| Launcher / UI (Tauri, TypeScript, CSS) | `scripts/dev.{sh,ps1} verify --with-launcher` or `scripts/dev.{sh,ps1} verify --ci` | Ensure Node.js is available; call out launcher skips that stem from missing Node. |
| Dependency bumps, build scripts, migrations | Fresh build (`scripts/dev.{sh,ps1} build`) then `scripts/dev.{sh,ps1} verify` | Note any manual steps (e.g., `npm install`, `py -3 -m pip install -r requirements/...`) in the PLAN so reviewers can reproduce them. |

### Fast Feedback & Incremental Loops
- Discover the project’s shortest feedback path before editing: scan `Justfile`, `Makefile`, and `package.json`/`pyproject.toml` scripts for crate- or package-scoped commands you can reuse instead of inventing ad-hoc loops.
- Reach for incremental runners (`cargo check -p <crate>`, `cargo nextest run -p <crate> --run-ignored none`, `cargo watch -x "clippy -p <crate>"`, `npm test -- --watch`, `pytest path/test_file.py`) when the PLAN touches a narrow surface; upgrade to the full helper once changes fan out or before handing work back.
- Reuse cached artefacts when safe: prefer `cargo check` or a targeted build over `cargo clean`, and call out in your status message when you relied on a previous successful `verify` run (include the command and timestamp/commit).
- When a helper produces partial results (e.g., `verify --fast` finishes while `verify --ci` is queued), note the coverage you already have and the follow-up you still owe.

If a command writes artefacts you do not intend to commit, clean them before finishing (`cargo clean -p <crate>`, remove generated files) or document why they remain.

### Handling Flaky or Long-Running Checks
- Retry a failing test once to rule out transient issues (`cargo nextest run --filter "<name>"` or rerun the helper). Capture the command and result so reviewers understand the outcome.
- When a check is known flaky or exceeds the harness time limit, stop, document the behavior (test name, command, error snippet), and ask the maintainer whether to proceed. Do **not** silently skip.
- For multi-hour suites, coordinate with the user before running them. Offer a scoped alternative (e.g., package-only nextest run, mocked docs build) and state the trade-offs.
- If a run must be aborted, include the partial logs or the failing test names in your notes so the next attempt can pick up quickly.
- If an environment gap (missing Node/MkDocs/Python) prevents a check, attempt the documented bootstrap step once. If it still fails, record the command, failure, and remediation attempt in your status.

### Reporting Results
- In the final response, list each command you ran (`scripts/dev.ps1 verify --fast`, `cargo nextest run -p <crate>`) and whether it passed, failed, or was skipped.
- Include a short error summary for failed checks (test name, assertion) and the follow-up you proposed.
- Mention when you relied on cached builds or prior runs (for example, “reused `scripts/dev.ps1 verify --fast` from earlier today; reran `cargo nextest run -p runtime` after changes”).

### Strengthening Test Coverage
- Leave new or improved tests in place when they harden behavior—temporary scaffolding is the only thing that should be removed before hand-off.
- When you spot an obvious missing guardrail while fixing a bug, add or update a test within the touched surface and run the narrowest command that exercises it.
- If time constraints force you to defer a durability improvement, capture the proposed test (filename, outline, expected assertion) in your notes so it can be scheduled deliberately.
- When you skip a check, state the justification (“config-only change; verify skipped”) and the risk so maintainers can decide whether to rerun it.
- Attach links or reference paths instead of dumping large logs; keep snippets to the failing assertion or panic line.

### Dependency & Environment Adjustments
- After modifying `Cargo.toml`, run `cargo check` (or `scripts/dev.{sh,ps1} build`) so lockfiles regenerate under the same command the maintainers expect. For Python/Node manifest edits, run the matching installer (`py -3 -m pip install -r requirements/<file>.txt`, `npm install`, `pnpm install`) and include the command in the PLAN.
- Document environment variables or feature flags you touched (`ARW_VERIFY_INCLUDE_LAUNCHER=1`, `DOCS_CHECK_FAST=1`) so the next agent can mirror your setup.
- When scripts such as formatters or generators (`just fmt`, `npm run lint`) auto-run builds or tests, mention that they execute those steps implicitly and whether additional verification is still needed.

## Windows Execution Notes
- Harness default: call PowerShell directly—e.g., `["pwsh","-NoLogo","-NoProfile","-Command", ...]` when invoking `shell`. Only pivot to Git Bash/WSL after confirming they exist. Use PowerShell’s `&` to run scripts (`& .\scripts\dev.ps1 verify`).
- Search fallbacks: clean Windows sandboxes may ship without `rg`. Reach for `Get-ChildItem -Recurse` and `Select-String -Path <glob> -Pattern '<term>'`, and mention the fallback in your status so reviewers know why `rg` output is missing.
- Python launcher: translate any `python3` instructions to `py -3` or `python` on Windows, and note the alias you used so the run is reproducible.
- Build/test wrappers: stick to the platform helpers (`scripts\dev.ps1 build/test/verify`) or bundled `.bat`/`.cmd` shims (`& .\gradlew.bat test`, `npm.cmd test`). Quote paths containing spaces instead of using `cd`.
- Line endings & execution policy: preserve LF-only scripts (`git config core.autocrlf false`, `.gitattributes`) and fix CRLF issues with `dos2unix` or `Set-Content -NoNewline`. Unblock downloads (`Unblock-File`) and, if PowerShell blocks repo scripts, temporarily run `Set-ExecutionPolicy -Scope Process Bypass` before restoring your stricter baseline.
- WSL workflow: keep the checkout under `/home/<user>` inside WSL, run `bash scripts/env/switch.sh windows-wsl` before builds/tests, and switch back on the host (`scripts\env\switch.ps1 windows-host`) when returning to PowerShell so artefacts don’t cross between environments. Use the Linux helpers (`scripts/dev.sh`, `just`) inside WSL instead of the `.ps1` variants.

Lightweight path (ease-of-use)
- Use only when the execution harness or reviewer explicitly treats the work as a trivial shortcut (typo fix, single-line docs tweak, metadata bump).
- Keep the edit self-contained (<20 lines, single file unless otherwise stated) and avoid behavioral changes or new dependencies.
- Spell out the assumption in your response: call out that the lightweight path was used, list skipped checks, and justify why it is safe.
- If the change grows beyond a micro-edit at any point, fall back to the full loop above.

Harness alignment
- Treat execution-harness permissions (e.g., baseline workspace writes, shell access) as explicit approvals; do not block on requesting authorization that the harness already grants or forbids.
- When harness policy prevents a required check (tests, docs build), note the skipped item and rationale in the response instead of forcing the run.
- If harness instructions contradict this guide, follow the harness and document the deviation so reviewers understand the context.
- CI extends `verify` with registry syncs (`scripts/check_feature_integrity.py`, `scripts/check_system_components_integrity.py`) plus triad/runtime smokes; the triad harness now defaults to a 10 s timeout (set `TRIAD_SMOKE_TIMEOUT_SECS` higher when you are exercising a real host). Plan those runs when your change touches the relevant surfaces.

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
- Refresh the metadata header after doc edits with `python scripts/update_doc_metadata.py path/to/doc.md` (add `--dry-run` to surface stale stamps without writing).

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

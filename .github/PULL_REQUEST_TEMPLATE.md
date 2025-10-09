Title: <type>(scope): <short summary>

Microsummary
- What changed and why in 2-3 lines.

Plan (final)
- Files touched, risks, user impact. If you used the lightweight path from docs/ai/ASSISTED_DEV_GUIDE.md, call that out explicitly.

Test results
- Local checks (fmt, clippy -D warnings, nextest) and any output summaries. When checks were skipped because you used the lightweight path, note “skipped (lightweight path)” and explain why it was safe.

Docs impact
- Pages updated/added; link to sections.

Scope guard
- Out-of-scope work that was intentionally left out.

Breaking changes
- N/A or list with upgrade notes.

## Summary

Describe the change and motivation.

## Checklist

- [ ] Registry integrity: `python3 scripts/check_feature_integrity.py` and `python3 scripts/check_system_components_integrity.py`  _(mark “not run (lightweight path)” with justification when the exception applies)_
- [ ] Docs generated and committed: `just docs-build` (Bash) or `scripts/docgen.sh` / `scripts/docgen.ps1` followed by `mkdocs build --strict`  _(same note as above)_
- [ ] Lints/tests (targeted): `cargo clippy -p arw-core -p arw-server -- -D warnings` and `cargo nextest run -p arw-server`  _(same note as above)_
- [ ] For registry edits: referenced docs/paths exist
- [ ] Changelog/notes updated if user-visible

## Screenshots / Notes

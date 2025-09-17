## Summary

Describe the change and motivation.

## Checklist

- [ ] Registry integrity: `python3 scripts/check_feature_integrity.py` and `python3 scripts/check_system_components_integrity.py`
- [ ] Docs generated and committed: `just docs-build` (or `scripts/docgen.sh` + `mkdocs build --strict`)
- [ ] Lints/tests (targeted): `cargo clippy -p arw-core -p arw-server -p arw-svc -- -D warnings` and `cargo nextest run -p arw-server -p arw-svc`
- [ ] For registry edits: referenced docs/paths exist
- [ ] Changelog/notes updated if user‑visible

## Screenshots / Notes


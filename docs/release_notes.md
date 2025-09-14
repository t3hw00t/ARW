# Release Notes

Microsummary: Pointers to releases, changelog, and upgrade notes. Stable.

- GitHub Releases: https://github.com/t3hw00t/ARW/releases
- Changelog: see `CHANGELOG.md` in the repository root (follows Keep a Changelog + SemVer).
- Upgrade notes: breaking changes and config/schema migrations are called out per release.

## Event Kind Normalization

- The service publishes normalized lowercase dot kinds (e.g., `models.download.progress`).
- Legacy/dual modes have been removed; update any consumers listening to `Models.*` to normalized forms.
- SSE clients should filter with `?prefix=models.` and update matchers accordingly.

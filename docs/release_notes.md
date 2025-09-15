# Release Notes
Updated: 2025-09-14
Type: Reference

Microsummary: Pointers to releases, changelog, and upgrade notes. Stable.

- GitHub Releases: https://github.com/t3hw00t/ARW/releases
- Changelog: see `CHANGELOG.md` in the repository root (follows Keep a Changelog + SemVer).
- Upgrade notes: breaking changes and config/schema migrations are called out per release.

## Event Kind Normalization

- The service publishes normalized lowercase dot kinds (e.g., `models.download.progress`).
- Legacy/dual modes have been removed; update any consumers listening to `Models.*` to normalized forms.
- SSE clients should filter with `?prefix=models.` and update matchers accordingly.
 - Publishers now use centralized constants from `apps/arw-svc/src/ext/topics.rs`.
 - Connector publishes both cluster and node subjects in dot.case:
   - Cluster-wide: `arw.events.task.completed`
   - Node-scoped: `arw.events.node.<node_id>.task.completed`
 - New topic: `experiment.activated` (emitted when an experiment variant is applied via `/experiments/activate`).

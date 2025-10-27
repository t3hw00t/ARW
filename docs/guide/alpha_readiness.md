Updated: 2025-10-27
Type: How‑to
## Alpha Readiness (What to test and why)

Purpose: define the small set of “core, should work every time” paths we consider test‑worthy before any RC/GA discussions. This keeps the scope tight and avoids conflating preview features with production surfaces.

Core surfaces (alpha)

- SSE basics
  - Contract: /events is reachable; at least one event arrives shortly after boot (service.*, state.* acceptable).
  - Metrics visible: arw_events_sse_connections_total and/or arw_events_sse_sent_total present on /metrics.
- Economy ledger
  - Snapshot: GET /state/economy/ledger returns an object with version, entries, totals.
  - CLI parity: arw-cli state economy-ledger --json matches the snapshot shape.
  - Watcher parity: arw-mini-dashboard can hydrate once and render an update (when available).
- Route stats
  - Snapshot: GET /state/route_stats returns by_path; mini dashboard once --id route_stats prints a summary.

Preview surfaces (non‑blocking during alpha)

- Daily Brief watcher and Launcher polish (nice-to-have checks; not blockers).
- OCR “compression‑lite” metrics (behind feature flag; verify only when enabled).
- Adapters gallery health probes and viewer (DX focused; not GA surface).

How to run checks

- Use GitHub Actions → Build + Deep Checks (Manual).
  - Targets: run all (Linux/macOS/Windows) for cross‑OS signal, or linux to iterate faster.
  - Pass criteria (per job):
    - economy.json parses and contains version.
    - route_stats (or economy_ledger) mini-dashboard once JSON parses.
    - events_tail.json has at least one structured JSON line.
    - /metrics exposes arw_events_sse_connections_total or arw_events_sse_sent_total.

Exit criteria to graduate from “alpha” to “test‑worthy base”

- The Linux deep checks are consistently green for a week on main.
- No flake in SSE presence or economy snapshot shape across a few merges.
- Clear labeling in docs for preview/alpha surfaces vs. core surfaces.


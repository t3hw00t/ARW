# Deep Checks (Local)
Updated: 2025-10-30
Type: How‑to

Use these local harnesses to validate the alpha core surfaces before running CI deep checks.

What they do

- Build (release) arw-server, arw-cli, arw-mini-dashboard
- Start arw-server on port 8099
- Validate:
  - Economy snapshot JSON (has version)
  - route_stats via mini-dashboard once (JSON)
  - Events tail (structured) yields at least one JSON line
  - SSE metrics presence: arw_events_sse_connections_total or arw_events_sse_sent_total
- Stop the server; logs written to /tmp (Linux/macOS) or server.out/err (Windows)

Run it

- Linux/macOS:
  - Just: `just deep-checks base=http://127.0.0.1:8099`
  - Mise: `mise run deep:checks`
- Windows PowerShell:
  - Just: `just deep-checks-ps`
  - Mise: `mise run deep:checks:ps`

Soft mode

- Set `DEEP_SOFT=1` to relax the “no events output” failure in short tails while keeping other checks strict. Useful on slow boots.
- The CI manual workflow also accepts a “soft” input that maps to `DEEP_SOFT` for the event-tail check.

Notes

- Set `BASE` and `ARW_ADMIN_TOKEN` if you’re not using defaults.
- These checks mirror the CI “Build + Deep Checks (Manual)” workflow, but keep everything on your machine.
- macOS note: the helper falls back to an internal timer when GNU `timeout` isn’t installed; install `coreutils` if you prefer the GNU tooling.
- Prompt compression is optional but recommended: after running `scripts/bootstrap_docs.sh`, install `llmlingua` into `.venv/docs` (`source .venv/docs/bin/activate && pip install llmlingua`) so the helper can export `LLMLINGUA_PYTHON` automatically and avoid the noop fallback warning.

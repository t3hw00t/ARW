---
title: Quickstart
---

# Quickstart

!!! warning "Minimum Secure Setup"
    - Set an admin token: `ARW_ADMIN_TOKEN=your-secret`
    - Don’t enable debug in production: leave `ARW_DEBUG` unset
    - Keep the service private: bind to `127.0.0.1` or put behind TLS proxy
    - Send the header on admin calls: `X-ARW-Admin: your-secret`

Prerequisites
- Rust toolchain (`rustup`): https://rustup.rs

Build and test

=== "Windows"
```powershell
scripts/build.ps1
scripts/test.ps1
```

=== "Linux / macOS"
```bash
bash scripts/build.sh
bash scripts/test.sh
```

Run the service

=== "Windows"
```powershell
powershell -ExecutionPolicy Bypass -File scripts/start.ps1 -WaitHealth
```

=== "Linux / macOS"
```bash
bash scripts/start.sh --wait-health
```

Verify
```bash
curl -sS http://127.0.0.1:8090/healthz
```

Desktop launcher (optional)

=== "All"
- Build: `just tauri-launcher-build`
- Run: `just tauri-launcher-run`
- Features: system tray (Start/Stop/Open), Events (SSE), Logs, Debug UI opener, prefs & autostart.

=== "Linux build deps"
```bash
just tauri-deps-linux
# or
nix develop
```

Peek at what’s available
- Health: `GET /healthz`
- Events (SSE): `GET /events`
- Tools: `GET /introspect/tools`
- Schemas: `GET /introspect/schemas/{id}`
- Debug UI: open `/debug` (if provided by your build)

Debug UI tips
- Set `ARW_DEBUG=1` to enable the `/debug` page.
- Look for small “?” icons beside sections. Click to see a gentle inline tip and a link to the matching docs page.
- Set `ARW_DOCS_URL` (e.g., your GitHub Pages URL) so the “Docs” button in the header opens your hosted manual.
- The Orchestration panel groups common actions (Probe, Emit test, Refresh models, Self‑tests, Shutdown) to streamline flows.
- Profiles: use the profile picker (performance/balanced/power‑saver) to apply a runtime hint. Endpoint: `POST /governor/profile { name }`, check with `GET /governor/profile`.
- When available locally, the docs can also be served at `/docs` (see Packaging notes).

Self‑Learning panel
- Send a signal (latency/errors/memory/cpu) with a target and confidence to record an observation.
- Click “Analyze now” to produce suggestions (e.g., increase http timeout, switch profile, raise memory limit mildly).
- Apply a suggestion by id or toggle “auto‑apply safe” (for conservative changes).
- The Insights overlay shows live event totals and the top 3 routes by EWMA latency.

Security
- Sensitive endpoints (`/debug`, `/probe`, `/memory*`, `/models*`, `/governor*`, `/introspect*`, `/chat*`, `/feedback*`) are gated.
- Development: set `ARW_DEBUG=1`. Hardened: set `ARW_ADMIN_TOKEN` and send header `X-ARW-Admin: <token>`.

Portable mode
- Set `ARW_PORTABLE=1` to keep state near the app bundle.
- Paths and memory layout are reported by `GET /probe`.

Next steps
- Read the Features page to understand the capabilities.
- See Deployment to package and share a portable bundle.
- Having issues? See Troubleshooting.

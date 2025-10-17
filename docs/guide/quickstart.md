---
title: Quickstart
---

# Quickstart

Updated: 2025-10-17
Type: Tutorial

Run the unified ARW server locally in minutes. The architecture centres on the `/actions` → `/events` → `/state/*` triad; enable `ARW_DEBUG=1` to serve the browser debug panels.

!!! warning "Minimum Secure Setup"
    - Set an admin token: `ARW_ADMIN_TOKEN=your-secret`
    - Keep the service private: bind to `127.0.0.1` or front with TLS
    - Require the header on sensitive calls: `Authorization: Bearer your-secret` or `X-ARW-Admin`
    - Leave `ARW_DEBUG` unset in production

## Prerequisites
- Rust 1.90+ toolchain (`rustup`): https://rustup.rs
- `curl` for quick verification (or `Invoke-WebRequest` on Windows)

> ARW tracks the latest stable Rust release; run `rustup update` regularly to avoid toolchain drift.

Need a single command that brings in Rust, Python, Node.js, jq, and ripgrep? Install [mise](https://mise.jdx.dev) and run `mise install`; once the toolchain is hydrated you can call `mise run verify`, `mise run verify:fast`, or `mise run bootstrap:docs` as shorthand for the guardrail helpers below.
!!! note
    The first launch compiles `arw-server` (and the optional launcher). Expect a multi-minute build on initial setup; subsequent runs reuse the cached binaries.

## Select Your Environment

- Run `bash scripts/env/switch.sh <mode>` from inside the platform you are working on (`linux`, `windows-host`, `windows-wsl`, `mac`).  
  Example: `bash scripts/env/switch.sh windows-host`
- The helper updates `.arw-env`, flips `target/` and `.venv/` to the right copies for that platform, and refuses to run if you launch it from the wrong environment.
- When you move between Windows host and WSL, run the switch command on each side before rebuilding or running tests to prevent cross-platform binaries from colliding.
- For a deeper walkthrough (including toolchain notes per mode) see [Developer › Environment Modes](../developer/environment_modes.md).

## Launch Paths

### Automation — Headless Agents

Need a non-interactive bootstrap for scripted agents or CI sandboxes? Use the headless helper.

=== "Bash"
```bash
bash scripts/dev.sh setup-agent
```

=== "PowerShell"
```powershell
scripts\dev.ps1 setup-agent
```

This path pins `--headless --minimal --no-docs`, exports `ARW_DOCGEN_SKIP_BUILDS=1`, and compiles `arw-server` in the debug profile to keep turnaround short for autonomous runs (append `--with-cli` / `-WithCli` when you also need `arw-cli`). It also installs PyYAML via `pip` (setting `PIP_BREAK_SYSTEM_PACKAGES=1` when the host enforces PEP 668) so `scripts/dev.{sh,ps1} verify` can run without extra manual steps.

### Option 1 — Portable bundle (self-built)

1. Build a bundle locally:
   - Linux / macOS: `bash scripts/package.sh`
   - Windows: `pwsh -ExecutionPolicy Bypass -File scripts\package.ps1`
2. Extract the generated archive in `dist/` and run the bundled helper:
   - Linux / macOS: `./first-run.sh`
   - Windows: `pwsh -ExecutionPolicy Bypass -File .\first-run.ps1`
     (run `Unblock-File .\first-run.ps1` first if Windows marks the download as blocked)
3. The helper generates (or reuses) `state/admin-token.txt`, starts `arw-server`, and prints the Home view/Debug URLs. Append `--launcher` / `-Launcher` to launch the desktop Home view when the launcher binary is present, or `--new-token` / `-NewToken` to rotate credentials on demand.

No official bundles are published during the `0.2.0-dev` cycle, so this path is the quickest way to produce a portable archive without maintaining a full toolchain on the target machine.

### Option 2 — Build from source (Rust toolchain)

> The first build compiles `arw-server` and, when requested, the Tauri launcher. Expect a multi-minute compile on cold toolchains; subsequent runs reuse the cached artifacts.

- Cross-platform helper  
  - Linux / macOS: `bash scripts/dev.sh setup`  
  - Windows: `pwsh -NoLogo -NoProfile -File scripts\dev.ps1 setup`  
  These invocations auto-accept prompts (`-Yes`) and default to headless builds unless you pass `--with-launcher` / `-WithLauncher`.

- Linux / macOS  
  `bash scripts/setup.sh --headless`

- Windows  
  `powershell -ExecutionPolicy Bypass -File scripts\setup.ps1 -Headless`

Use `--headless` / `-Headless` to skip the launcher build when WebKitGTK 4.1 + libsoup3 (Linux) or WebView2 (Windows) isn’t available yet. Drop the flag to compile the desktop Home view once the prerequisites are installed. Add `--minimal` / `-Minimal` to build just the core binaries without packaging or docs, and `--run-tests` / `-RunTests` when you want the workspace test suite as part of setup.

!!! tip "Bootstrap without compiling immediately"
    Append `--skip-build` (or `-SkipBuild` on Windows) when you only need the toolchains and dependency checks. The workspace build/tests can run later (e.g., in CI or on demand).

## Build and Test (optional)

Use the standalone build/test helpers when you want finer-grained control or CI-style runs. The `scripts/dev.{sh,ps1}` helper wraps them (e.g., `scripts/dev.sh build`, `scripts/dev.ps1 verify`) with safe defaults.

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

The build helpers default to a headless profile that skips the Tauri launcher. Add `-WithLauncher` / `--with-launcher` (or set `ARW_BUILD_LAUNCHER=1`) when you specifically need the desktop UI and have installed the platform dependencies (WebKitGTK 4.1 + libsoup3 on Linux, WebView2 on Windows).

Prefer `make build` / `just build` when you want the same headless defaults in common automation. Use `make build-launcher` or `just build-launcher` to opt into compiling the desktop UI alongside the server.

The helpers fall back to `cargo test --workspace --locked` when `cargo-nextest` is missing and explain how to install it for faster runs.

Need a lightweight docs lint while iterating? Run:

```bash
DOCS_CHECK_FAST=1 python3 scripts/docs_check.py
```

Drop the environment variable (or omit `--fast`) to restore the full validation, including mkdocs build and deep legacy sweeps, before publishing.

Guardrail sweep (fmt → clippy → tests → docs) lives behind `bash scripts/dev.sh verify`. The default headless run skips the `arw-launcher` crate; pass `--with-launcher` / `-WithLauncher` or set `ARW_VERIFY_INCLUDE_LAUNCHER=1` when you explicitly need the desktop UI coverage, and append `--fast` / `-Fast` when you only need the Rust/test coverage and will address docs or launcher UI checks later in the workflow. When Node.js is missing the launcher smoke is auto-skipped; export `ARW_VERIFY_REQUIRE_DOCS=1` if you want absent Python/PyYAML to fail the run instead of downgrading to informational skips. Reach for `--ci` / `-Ci` when you need the GitHub Actions matrix locally (registry integrity, doc generators in `--check` mode, env-guard lint, snappy bench, triad/context/runtime smokes, and legacy surface checks).
Prefer the new task wrappers? `mise run verify` mirrors the full suite, `mise run verify:fast` maps to the lean option, and `mise run verify:ci` runs the CI-parity sweep. `mise run docs:check` and `mise run docs:check:fast` wrap the docs lint commands. Run `mise run bootstrap:docs` (or `bash scripts/bootstrap_docs.sh`) whenever you need the pinned MkDocs/Material stack installed.
Need to bump a doc header after edits? Run `python scripts/update_doc_metadata.py docs/path/to/page.md` (add `--dry-run` to preview changes).
Working offline? `mise run docs:cache:build` or `scripts/dev.{sh,ps1} docs-cache` creates `dist/docs-wheels.tar.gz` with all pinned MkDocs wheels. Release bundles ship the same archive—download it, extract on the air-gapped host, and run `mise run bootstrap:docs -- --wheel-dir <extracted-dir>`.

## Admin Token Handling

`scripts/start.sh` and `scripts/start.ps1` reuse `state/admin-token.txt` (or generate a new token automatically) and persist it to launcher preferences, so you rarely need to export `ARW_ADMIN_TOKEN` manually. Pass `--admin-token` / `-AdminToken` when you need to supply your own credential, or edit the Home view → Connection & alerts panel to rotate it later.

## Run the Unified Server

The unified server streams events and state over HTTP/SSE; choose the headless path when you only need the API.

**Headless only** — keep the service running without a desktop UI.

=== "Windows"
```powershell
# Headless server (8091 by default)
powershell -ExecutionPolicy Bypass -File scripts/start.ps1 -ServiceOnly -WaitHealth
```

=== "Linux / macOS"
```bash
# Headless server (8091 by default)
bash scripts/start.sh --service-only --wait-health
```

### Live Reload (manifests & bundles)
- Managed runtime manifests auto-reload: edits to `configs/runtime/runtimes.toml` (or a custom path via `ARW_RUNTIME_MANIFEST`) are applied within a few seconds; the supervisor reloads definitions and updates `/state/runtime_supervisor`.
- Bundle catalogs auto-reload: changes under `configs/runtime/*.json` or `<state>/runtime/bundles/` are detected and `/state/runtime/bundles` refreshes accordingly.
  - Override bundle roots with `ARW_RUNTIME_BUNDLE_DIR` (semicolon-separated).

### Observe Reloads (SSE)
- Quick tail (Bash):
  ```bash
  just sse-tail prefixes='service.health,state.read.model.patch' replay='10'
  ```
- Raw curl stream:
  ```bash
  curl -N -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
    "http://127.0.0.1:8091/events?prefix=service.health,state.read.model.patch&replay=10"
  ```
- Watcher summary snapshot:
  ```bash
  curl -s -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
    http://127.0.0.1:8091/state/runtime/watchers | jq
  # Fields include per-area status (ok/degraded), last_ok_ms/last_ok_age_ms,
  # last_error_ms/last_error_age_ms, and overall status.
  ```

### Configure Cooldown
- Default cooldown is 3 minutes; within this window, a more recent error keeps status `degraded`.
- Override via env: `ARW_RUNTIME_WATCHER_COOLDOWN_MS=600000` (10 minutes)
- Or in `configs/default.toml` under the `env` section:
  ```toml
  [env]
  ARW_RUNTIME_WATCHER_COOLDOWN_MS = 600000
  ```

**Home view + launcher** — start the service and the desktop UI together.

=== "Windows"
```powershell
# Service + launcher (8091 by default)
powershell -ExecutionPolicy Bypass -File scripts/start.ps1 -WaitHealth
# Append -InstallWebView2 to auto-install the Evergreen runtime when missing.
```

=== "Linux / macOS"
```bash
# Service + launcher (8091 by default)
bash scripts/start.sh --wait-health
```

!!! note "Linux desktop launcher"
    The Tauri launcher is optional on Linux. Install WebKitGTK 4.1 + libsoup3 first, then build it explicitly with:
    `cargo build -p arw-launcher --features launcher-linux-ui`. Skip that command (or pass `--headless` to the setup
    script) to stay on the headless/server-only profile.

!!! tip "Linux launcher requirement"
    The Tauri launcher depends on WebKitGTK 4.1 + libsoup3. Run `bash scripts/install-tauri-deps.sh` on Ubuntu 24.04+, Fedora, or Arch. If your distro lacks those packages (e.g., Ubuntu 22.04, Debian 12 stable), stay headless and open `http://127.0.0.1:8091/admin/ui/control/` or `/admin/debug` in a browser.

The Windows launcher flow prints a summary (service URL, launcher/headless mode, admin token status) and automatically falls back to headless mode if WebView2 is missing, with guidance for installing it.
Open **Launcher Settings** (Home view → Support) to tweak autostart behaviour, notifications, default port/base, and WebView2 status after the desktop UI loads.

## Verify the Server

=== "Windows (PowerShell)"
```powershell
Invoke-RestMethod http://127.0.0.1:8091/healthz
Invoke-RestMethod http://127.0.0.1:8091/about | ConvertTo-Json -Depth 6
```

=== "macOS / Linux"
```bash
curl -sS http://127.0.0.1:8091/healthz
curl -sS http://127.0.0.1:8091/about | jq
```

!!! note
    `jq` is optional—install it (`sudo apt install jq`, `brew install jq`, etc.) or switch to `curl ... | python -m json.tool` when a JSON formatter is not already available.

You should see metadata that lists the unified endpoints, performance presets, and the current security posture.

## Try an Action End-to-End

Submit a demo action, watch its lifecycle, then fetch it back:

```bash
curl -s -X POST http://127.0.0.1:8091/actions \
  -H 'content-type: application/json' \
  -d '{"kind":"demo.echo","input":{"msg":"hello"}}' | jq

curl -N -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
  http://127.0.0.1:8091/events?replay=10

curl -s -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
  http://127.0.0.1:8091/state/actions | jq
```

The events stream shows `actions.submitted`, `actions.running`, and `actions.completed`. When `ARW_ADMIN_TOKEN` is set, `/events` and sensitive `/state/*` views require the token; this is recommended for any non‑local setup.

## Explore State Views

```bash
curl -s -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
  http://127.0.0.1:8091/state/episodes | jq
curl -s -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
  http://127.0.0.1:8091/state/contributions | jq
curl -s -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
  http://127.0.0.1:8091/state/egress/settings | jq
```

Additional views expose models, self descriptions, memory lanes, logic units, orchestrator jobs, and more—capabilities that landed with the restructure and continue to expand with regular releases.

## Policy, Leases, and Context

```bash
# Inspect the effective policy
curl -s -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
  http://127.0.0.1:8091/state/policy | jq

# Create a lease that allows outbound HTTP for 10 minutes
curl -s -X POST http://127.0.0.1:8091/leases \
  -H 'content-type: application/json' \
  -d '{"capability":"net:http","ttl_secs":600}' | jq

# Assemble context (hybrid retrieval with streaming diagnostics)
curl -s -X POST http://127.0.0.1:8091/context/assemble \
  -H 'content-type: application/json' \
  -d '{"q":"demo","lanes":["semantic","procedural"],"limit":12}' | jq
```

These flows emit structured `policy.*`, `working_set.*`, and `leases.*` events. Dashboards can follow along via `/events` or state views.

## Debug UI

Enable `ARW_DEBUG=1` to expose the debug panels at `/admin/debug`.

=== "Windows"
```powershell
scripts/debug.ps1 -Interactive -Open
```

=== "Linux / macOS"
```bash
scripts/debug.sh --interactive --open
```

These helpers prompt for admin tokens, wait for `/healthz`, and launch the UI once ready.

## Desktop Launcher

The Tauri-based launcher targets the unified server. To package or run it:

```bash
just tauri-launcher-build
just tauri-launcher-run
```

## Docker & Compose

A lightweight container image is available for the unified server:

```bash
# Build locally
docker build -f apps/arw-server/Dockerfile -t arw-server:dev .

# Run headless (bind 8091)
export ARW_ADMIN_TOKEN="${ARW_ADMIN_TOKEN:-$(openssl rand -hex 32)}"
docker run --rm -p 8091:8091 \
  -e ARW_BIND=0.0.0.0 \
  -e ARW_PORT=8091 \
  -e ARW_ADMIN_TOKEN="$ARW_ADMIN_TOKEN" \
  arw-server:dev
```

Generate the secret with any equivalent tool if `openssl` is unavailable.

`docker compose up` uses the unified server by default.

## Security

- Require `ARW_ADMIN_TOKEN` before invoking `/leases`, `/egress/settings`, or other admin-grade endpoints.
- Use leases to gate outbound HTTP, filesystem writes, or app control (`app.vscode.open`).
- Enable the egress ledger and DNS guard with environment flags (`ARW_EGRESS_LEDGER_ENABLE=1`, `ARW_DNS_GUARD_ENABLE=1`).
- Keep `/events` and `/state/*` behind auth when exposed over the network; they contain action telemetry and internal state.

## Portable Mode

- Extracted release bundle? Run `./first-run.sh` (Linux/macOS) or `pwsh -ExecutionPolicy Bypass -File .\first-run.ps1` (Windows) from the archive root to generate/reuse an admin token (`state/admin-token.txt`) and start the unified server on `http://127.0.0.1:8091/`. If SmartScreen blocks the script, right-click -> **Properties** -> **Unblock** or run `Unblock-File .\first-run.ps1` once. Add `--launcher` / `-Launcher` to open the Home view or `--new-token` / `-NewToken` to rotate credentials.
- Set `ARW_STATE_DIR` to relocate state (defaults to `./state`).
- Combine with `ARW_PORTABLE=1` in launchers or scripts to keep all files beside the binaries for USB-style deployment.

## Next Steps

- Read the [Restructure Handbook](../RESTRUCTURE.md) for the canonical roadmap.
- Explore [Context Recipes](context_recipes.md) and [Performance Presets](performance_presets.md) to tune retrieval speed and coverage.
- Run `cargo run -p arw-server` during development for hot reloads and tracing; set `ARW_OTEL=1` (optionally combine with `ARW_OTEL_ENDPOINT=http://collector:4317`) to stream traces to your OTLP collector.

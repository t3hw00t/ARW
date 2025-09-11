# Agents Running Wild (ARW)

Local‑first Rust workspace for building and running personal AI agents. ARW
bundles a lightweight service, command‑line tools, and optional tray UI so you
can experiment without cloud lock‑in.

## Highlights

- User‑mode service with HTTP endpoints and a simple debug UI.
- Macro‑driven tool registration with automatic schema generation.
- Event stream and tracing hooks for observability.
- Portable packaging scripts for sharing or deployment.

## Download and Install

- Grab the latest binaries from the GitHub Releases page. Installers are built for Windows, macOS, and Linux using `cargo-dist`.
- Windows: download the `.msi` and run the installer.
- macOS: download the `.pkg` or `.tar.xz` archive and install the binary.
- Linux: download the `.tar.xz` archive, extract it, and place the binaries on your `PATH`.

## Quickstart

- Install Nix and use it as the entry point for building, testing, and packaging:
  - `nix develop` to enter a shell with all dependencies
  - Run commands directly: `nix develop --command cargo build`, `nix develop --command cargo test`, `nix develop --command scripts/package.sh`, etc.
- One‑shot setup (build, docs, package):
  - Windows: `powershell -ExecutionPolicy Bypass -File scripts/setup.ps1`
  - Linux/macOS: `bash scripts/setup.sh`
- Interactive setup & start menus:
  - Windows: `powershell -ExecutionPolicy Bypass -File scripts/interactive-setup-windows.ps1`
             `powershell -ExecutionPolicy Bypass -File scripts/interactive-start-windows.ps1`
  - Linux:   `bash scripts/interactive-setup-linux.sh` and `bash scripts/interactive-start-linux.sh`
  - macOS:   `bash scripts/interactive-setup-macos.sh` and `bash scripts/interactive-start-macos.sh`
  - Headless CI-friendly: Linux/macOS `ARW_HEADLESS=1 bash scripts/interactive-setup-linux.sh --headless --package`; Windows `powershell -ExecutionPolicy Bypass -File scripts/interactive-setup-windows.ps1 -Headless -Package`
  - Save preferences in-project via menu; scripts will auto-load `./.arw/env.sh` (Linux/macOS) or `./.arw/env.ps1` (Windows).
- Start the service (launches the tray when available) with options:
  - Windows: `powershell -ExecutionPolicy Bypass -File scripts/start.ps1 -Debug -Port 8090 -DocsUrl https://your-pages -AdminToken secret -WaitHealth`
  - Linux/macOS: `bash scripts/start.sh --debug --port 8090 --docs-url https://your-pages --admin-token secret --wait-health`
  - CLI-only (skip tray if present): set `ARW_NO_TRAY=1` before running `scripts/start.sh`
  - `arw-tray` is bundled and started automatically by the scripts when present. You can also run it manually from `target/release/` or `dist/.../bin/` to start/stop the service, open the Debug UI, or quit from the system tray.

Dev convenience
- Nix dev shell now includes `just` and `cargo-watch`.
- Fast loops:
  - `just dev` to run `arw-svc` with `ARW_DEBUG=1` (port 8090)
  - `just dev-watch` to auto-rebuild/restart the service on changes
  - `just docs-serve` to run MkDocs locally at `127.0.0.1:8000`
  - `just dev-all` to run service and docs together; the Debug UI picks up docs via `ARW_DOCS_URL`
  - `just open-debug` to open `http://127.0.0.1:8090/debug` in the browser
  - `just fmt-check`, `just lint`, `just lint-fix`, `just test-fast`, `just test-watch`
  - `just hooks-install` to install a pre-commit hook (fmt+clippy+tests)

Isolated env defaults
- Linux/macOS: when available, builds run inside `nix develop` for fully isolated toolchains (no root). Otherwise, scripts install Rust locally under `.arw/rust` and tools under `.arw/bin`; docs live in `.venv`.
- Windows: interactive setup installs Rust under `.arw\rust` (no admin) and docs into `.venv`; lightweight tools like `jq` download to `.arw\bin`.
- System package managers (apt/dnf/brew/etc.) remain off by default; you can explicitly enable them in the Dependencies menu if desired.

NATS broker
- Prefer native local broker: use the NATS manager in the start menu to install and run a project-local `nats-server` under `./.arw/nats` (no admin). PID/logs go under `./.arw/run` and `./.arw/logs`.
- Windows: native `nats-server.exe` is used. If you prefer WSL, you can install and run NATS within your distro as well.
- Fallback suggestion: Docker `docker run -p 4222:4222 nats:latest` if native install fails.
 - WSL (Windows): Start menu includes a WSL NATS manager. It installs `nats-server` inside your chosen WSL distro (no sudo required) and starts it bound to `0.0.0.0:4222`. Windows connects via `nats://127.0.0.1:4222`. It also shows WSLg (GUI) info if available and how to test GUI apps. Windows Setup also provides a one‑click elevated launcher for `wsl --install -d Ubuntu`.
- Traditional scripts (fine‑grained):
  - Build: `scripts/build.ps1` (Windows) or `scripts/build.sh` (Linux/macOS)
  - Test:  `scripts/test.ps1` or `scripts/test.sh`
  - Package: `scripts/package.ps1` or `scripts/package.sh` (creates `dist/` zip)
  - Uninstall: `scripts/uninstall.ps1` or `scripts/uninstall.sh` (removes build artifacts and MkDocs packages installed by setup)

Windows start flags
- `-NoBuild`: don’t auto-build if binaries missing; exit with error.
- `-WaitHealth`: after background start, poll `http://127.0.0.1:<port>/healthz` until ready.
- `-WaitHealthTimeoutSecs`: timeout for the health poll (default 30 in script; Start menu default 20).

Linux/macOS start flags
- `--no-build`: don’t auto-build if binaries missing; exit with error.
- `--wait-health`: after background start, poll `http://127.0.0.1:<port>/healthz` until ready.
- `--wait-health-timeout-secs`: timeout for the health poll (default 30; Start menus default 20).

## Component Overview

- **System / Host**: underlying OS, hardware, and runtime paths resolved via environment variables.
- **Core Project**: crates `arw-core`, `arw-protocol`, `arw-events`, `arw-otel`, and helper macros (`arw-macros`), plus binaries `arw-cli`, `arw-svc`, and optional `arw-tray`.
- **External Dependencies**: primary third-party crates such as Tokio, Axum, and Serde.
- **Core Plugins**: none bundled yet; future built-ins will live under `crates/plugins`.
- **Plugin Extensions**: community adapters and optional integrations may live under `crates/adapters`.

All installers and services compute effective paths at startup and write logs/state under the derived directories, keeping deployments portable across machines.

## Quick Start

```bash
# build, generate docs, and create a release package
scripts/setup.sh      # on Linux/macOS (GTK required for arw-tray)
powershell -File scripts/setup.ps1  # on Windows

# start the local service
scripts/start.sh --debug --port 8090
```

The service listens on `http://127.0.0.1:8090`; open `/debug` for a minimal UI.

Debug UI tips
- Press Ctrl/Cmd+K to open the command palette (Probe, Refresh Models, Self-tests, Toggle Insights, etc.).
- Press Ctrl+/ to view all keyboard shortcuts. Toggle keyboard shortcuts globally from the header.
- Set an admin token in the header bar to access gated endpoints when `ARW_DEBUG` is off. Optionally remember it locally.
- Admin and SSE badges show connectivity and gating status.

Models (Download/Resume/Cancel)
- Start a download:
  - `curl -sS -X POST http://127.0.0.1:8090/models/download -H 'Content-Type: application/json' -d '{"id":"qwen2.5-coder-7b","url":"https://example.com/qwen.gguf","sha256":"<hex-optional>"}'`
- Cancel:
  - `curl -sS -X POST http://127.0.0.1:8090/models/download/cancel -H 'Content-Type: application/json' -d '{"id":"qwen2.5-coder-7b"}'`
- Resume: re-issue the same request. If the server supports HTTP Range, ARW resumes from the existing `.part` file.

Chat backends
- llama.cpp server: set `ARW_LLAMA_URL` (e.g., `http://127.0.0.1:8080`). The `/chat/send` endpoint will use it when available.
- OpenAI compatible: set `ARW_OPENAI_API_KEY` (and optionally `ARW_OPENAI_BASE_URL`, `ARW_OPENAI_MODEL`). If llama is not set or fails, `/chat/send` falls back to OpenAI.

## Task Tracker (cross-machine)

- Manage a simple repo-tracked task list with `scripts/tasks.sh`.
- Tasks persist in `.arw/tasks.json` and docs render to `docs/developer/tasks.md` via `scripts/docgen.sh` and CI.
- Examples:
  - `scripts/tasks.sh add "Wire up tasks tracker" --desc "Generate docs + helper script"`
  - `scripts/tasks.sh start <task-id>`; `scripts/tasks.sh note <task-id> "progress..."`; `scripts/tasks.sh done <task-id>`
- View status in `docs/developer/tasks.md` or your published docs site if enabled.

## Documentation

- [Guide and API docs](docs/)
- [Roadmap](docs/ROADMAP.md)
- [Contributing](CONTRIBUTING.md)

## Containers

We publish `arw-svc` images to GHCR on every push to main and on tags.

Pull and run (amd64/arm64):

```bash
docker run --rm -p 8090:8090 \
  -e ARW_PORT=8090 \
  ghcr.io/t3hw00t/arw-svc:latest
```

Build locally:

```bash
docker build -f apps/arw-svc/Dockerfile -t arw-svc:dev .
docker run --rm -p 8090:8090 arw-svc:dev
```

### Docker Compose

Start the service with Compose (builds locally by default):

```bash
docker compose -f docker-compose.yml up --build -d
# open http://127.0.0.1:8090/healthz
```

Stop and remove:

```bash
docker compose down -v
```

### Helm (Kubernetes)

Render manifests:

```bash
helm template arw deploy/charts/arw-svc
```

Install/upgrade into namespace `arw`:

```bash
helm upgrade --install arw deploy/charts/arw-svc -n arw --create-namespace
```

Uninstall:

```bash
helm uninstall arw -n arw
```

### Dev Container (VS Code)

The repo includes a devcontainer configured with Nix. Open the `Agent_Hub` folder in VS Code and choose “Reopen in Container”. After creation, run:

```bash
cargo build --workspace
```

## Admin Access

- Sensitive endpoints include: `/debug`, `/probe`, `/memory/*`, `/models/*`, `/governor/*`, `/introspect/*`, `/chat/*`, `/feedback/*`.
- Access rules:
  - Set `ARW_DEBUG=1` to allow admin endpoints locally without a token (development only).
  - Or set an admin token and pass it on requests:
    - Env: `ARW_ADMIN_TOKEN=your-secret`
    - Header: `X-ARW-Admin: your-secret`
  - Rate limiting for admin endpoints: `ARW_ADMIN_RL="limit/window_secs"` (default `60/60`).
  - HTTP timeout hint can be adjusted dynamically via the debug UI or `/governor/hints`.

## Community & Support

Questions, ideas, or issues? Open a discussion or file an issue in this
repository. See the [project instructions](docs/PROJECT_INSTRUCTIONS.md) for
background and the [Quickstart guide](docs/guide/quickstart.md) for common setup.

---

ARW is released under the MIT OR Apache‑2.0 license.

Docs
- Source files live in `docs/`.

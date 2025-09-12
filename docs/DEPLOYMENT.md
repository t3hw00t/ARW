---
title: Deployment & Isolation
---

# Deployment & Isolation
Updated: 2025-09-06

See also: [Security Hardening](guide/security_hardening.md), [Configuration](CONFIGURATION.md)

## Goals
Run ARW as **unintrusively** as possible:
- Per‑user, self‑contained installs
- No admin rights required for core flows
- No background services by default
- Clean removal (portable mode)

## Modes

### 1) Native user‑mode (recommended)
- Best access to GPU/NPU via DirectML/CUDA/ROCm/CoreML/OpenVINO where available.
- Install ARW in a user folder (e.g., `%USERPROFILE%\arw`) and enable **portable mode** in config.

### 2) Windows Sandbox (.wsb)
- Ephemeral VM; resets after close.
- Use `sandbox/ARW.wsb` to map your project folder inside. Accelerator availability may be limited.
- Good for trials and risky experiments; less ideal for heavy local acceleration.

### 3) WSL2 / Full VMs
- Useful for Linux tooling; accelerator pass‑through depends on vendor/runtime.
- ARW will **fallback to CPU** automatically if accelerators are not accessible.

## Portable Mode

Set in `configs/default.toml`:

```toml
[runtime]
portable = true
state_dir = "%LOCALAPPDATA%/arw"
cache_dir = "%LOCALAPPDATA%/arw/cache"
logs_dir  = "%LOCALAPPDATA%/arw/logs"
```

You can override with `ARW_PORTABLE=1` env var, or set `state_dir` to a path inside the app folder for a single‑directory portable bundle.

## Filesystem & Registry

- ARW does **not** change system PATH or registry unless you explicitly run an installer that does so.
- Tauri apps (Launcher/Debug UI/Model Manager) ship as per‑user apps with no admin rights by default.

## Uninstall

- In portable mode: delete the ARW folder.
- Otherwise: remove `%LOCALAPPDATA%\arw` state directory and app directories under `%LOCALAPPDATA%\Programs` if present.

## Security & Policy

- Outbound network calls respect policy allowlists.
- Tools declare permission manifests; users grant consent per capability.
- Logs/telemetry default to local; exporting to OpenTelemetry is opt‑in.

## Known Constraints

- Virtualized environments can restrict GPU/NPU access.
- Some vendor accelerators require host‑level drivers; ARW degrades gracefully to CPU.

## Tauri Launcher (Desktop UI) Prerequisites

Tauri 2 apps use the system webview. On Linux, install these dev packages to build locally:

- Debian/Ubuntu: `sudo apt install -y libgtk-3-dev libwebkit2gtk-4.1-dev libjavascriptcoregtk-4.1-dev libsoup-3.0-dev`
- Fedora: `sudo dnf install -y gtk3-devel webkit2gtk4.1-devel libsoup3-devel`
- Arch: `sudo pacman -S --needed gtk3 webkit2gtk-4.1 libsoup3`

Alternatively use the project’s Nix dev shell which includes the required libraries:

```bash
nix develop
```

## Containers

Run a published image (amd64/arm64). Native binaries are provided for Windows (x64/ARM64), macOS (x64/ARM64), and Linux (x64/ARM64):

```bash
docker run --rm -p 8090:8090 \
  -e ARW_PORT=8090 \
  ghcr.io/t3hw00t/arw-svc:latest
```

Build locally and run:

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

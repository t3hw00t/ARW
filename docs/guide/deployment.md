---
title: Deployment & Isolation
---

# Deployment & Isolation

Updated: 2025-10-09
Type: How‑to

Run the unified `arw-server` in the environment that fits your workflow while keeping state portable and scoped. These recipes focus on the `/actions` → `/events` → `/state/*` stack.

## Goals
- Keep installs per-user and easy to remove.
- Avoid admin elevation for day-to-day use.
- Default to local bindings and explicit policy for any egress.
- Preserve portability: everything lives beside the binary unless you opt into system services.

## Modes

### 1) Native user-mode (recommended)
- Best access to GPU/NPU backends (DirectML/CUDA/ROCm/CoreML/OpenVINO where available).
- Install under your user directory (for example `%USERPROFILE%\\arw` or `~/arw`) and enable portable mode in config.
- Launch with the helper scripts (`scripts/start.ps1 -ServiceOnly` or `scripts/start.sh --service-only --wait-health`).

### 2) Windows Sandbox (.wsb)
- Ephemeral VM that resets on close; useful for evaluation or risky experiments.
- Use `sandbox/ARW.wsb` to map the project folder inside. Accelerator access may be limited by the host.
- Expect CPU fallback when the sandbox cannot see GPUs.

### 3) WSL2 / Full VMs
- Handy for Linux tooling on Windows or for isolating workloads.
- GPU/NPU pass-through depends on vendor support; the runtime falls back to CPU automatically when accelerators are missing.

## Portable Mode

Set in `configs/default.toml`:

```toml
[runtime]
portable = true
state_dir = "%LOCALAPPDATA%/arw"
cache_dir = "%LOCALAPPDATA%/arw/cache"
logs_dir  = "%LOCALAPPDATA%/arw/logs"
```

Overrides:
- `ARW_PORTABLE=1` keeps state/cache/logs beside the binary.
- `ARW_CONFIG` points to a specific primary config file.
- `ARW_CONFIG_DIR` adds extra config search paths (policy, gating, feedback, etc.).

## Filesystem & Registry
- The unified server does **not** touch PATH or the registry unless you run an installer that does so on purpose.
- Tauri apps (Launcher, Debug UI, future companions) remain per-user installs with no elevation required.

## Uninstall
- Portable mode: delete the ARW folder.
- Non-portable: remove `%LOCALAPPDATA%\\arw` (Windows) or `~/Library/Application Support/arw` (macOS) plus the install directory under `Programs/` if you used the MSI.

## Security & Policy Defaults
- `ARW_BIND` defaults to `127.0.0.1`; binding to anything else requires `ARW_ADMIN_TOKEN`.
- Permission leases gate tool access; outbound calls stay behind the egress firewall when enabled.
- Telemetry is local-first. Exporting spans/logs/metrics via OpenTelemetry is opt-in.

## Known Constraints
- Virtualized environments can hide GPUs or NPUs. Keep an eye on `/admin/probe/hw` to confirm what the runtime sees.
- Some vendor stacks need host drivers (Intel/NVIDIA/AMD). The runtime degrades gracefully to CPU when capabilities are absent.

## Tauri Launcher (Unified)
- Launcher bundles layer tray controls and inspectors on top of `arw-server`; enable `ARW_DEBUG=1` to serve the debug panels from the same process.
- Linux builds need `libgtk-3-dev`, `libwebkit2gtk-4.1-dev`, `libjavascriptcoregtk-4.1-dev`, and `libsoup-3.0-dev` (or the Nix shell via `nix develop`). These packages are
  available on Ubuntu 24.04 LTS and newer; Ubuntu 22.04 is not supported because
  it lacks WebKitGTK 4.1 packages.
- Use `just tauri-launcher-run -- --open` to launch the UI with `/admin/debug` available during development.

## Containers

Run the unified server image (amd64/arm64):

```bash
export ARW_ADMIN_TOKEN="${ARW_ADMIN_TOKEN:-$(openssl rand -hex 32)}"
docker run --rm -p 8091:8091 \
  -e ARW_BIND=0.0.0.0 \
  -e ARW_PORT=8091 \
  -e ARW_ADMIN_TOKEN="$ARW_ADMIN_TOKEN" \
  ghcr.io/t3hw00t/arw-server:latest
```

Use any equivalent tool to generate the secret if `openssl` is unavailable.

Build locally if you prefer:

```bash
docker build -f apps/arw-server/Dockerfile -t arw-server:dev .
export ARW_ADMIN_TOKEN="${ARW_ADMIN_TOKEN:-$(openssl rand -hex 32)}"
docker run --rm -p 8091:8091 \
  -e ARW_BIND=0.0.0.0 \
  -e ARW_PORT=8091 \
  -e ARW_ADMIN_TOKEN="$ARW_ADMIN_TOKEN" \
  arw-server:dev
```

Unified images replace the legacy bridge; new deployments should target `arw-server`.

### Docker Compose

```bash
docker compose up --build -d
curl -sS http://127.0.0.1:8091/healthz
```

Override `ARW_BIND=0.0.0.0` **and** set a strong `ARW_ADMIN_TOKEN` before exposing the container beyond localhost. The compose file defaults to the unified server.

### Helm (Kubernetes)

Render manifests for the new chart:

```bash
helm template arw deploy/charts/arw-server
```

Key values:
- `image.repository=ghcr.io/t3hw00t/arw-server` (override if you publish a fork)
- `image.tag=vX.Y.Z`
- `service.type=ClusterIP` (default) — front with your own ingress/TLS
- `env.ARW_BIND=0.0.0.0` and an admin token for any externally reachable deployment. Prefer: `adminToken.existingSecret=<secret-name>` (key defaults to `ARW_ADMIN_TOKEN`).
- Egress safety on by default in the chart: `env.ARW_EGRESS_BLOCK_IP_LITERALS=1`, `env.ARW_DNS_GUARD_ENABLE=1`. Adjust per cluster policy if needed.
- Optional hardening knobs:
  - `networkPolicy.enabled=true` with `networkPolicy.allowedCidrs=["10.0.0.0/8"]`
  - `pdb.enabled=true` with `pdb.minAvailable=0|1`
  - `autoscaling.enabled=true` with CPU‑based targets
  - `egressPolicy.enabled=true` to restrict outbound traffic. Allow DNS with `egressPolicy.dnsCidrs=["10.96.0.0/12"]` (adjust for your cluster) and add explicit `egressPolicy.allowedCidrs` as needed.

Legacy charts have been removed; use `deploy/charts/arw-server` for Kubernetes deployments.

## Rolling Access Logs

Structured access logs are available in the unified server, including rolling file support:

```bash
# Reuse the strong token exported earlier (or export a new one)
docker run --rm -p 8091:8091 \
  -e ARW_BIND=0.0.0.0 \
  -e ARW_PORT=8091 \
  -e ARW_ACCESS_LOG=1 \
  -e ARW_ACCESS_SAMPLE_N=1 \
  -e ARW_ACCESS_LOG_ROLL=1 \
  -e ARW_ACCESS_LOG_DIR=/var/log/arw \
  -e ARW_ACCESS_LOG_PREFIX=http-access \
  -e ARW_ACCESS_LOG_ROTATION=daily \
  -e ARW_ADMIN_TOKEN="$ARW_ADMIN_TOKEN" \
  -v $(pwd)/logs:/var/log/arw \
  ghcr.io/t3hw00t/arw-server:latest
```

Add `ARW_ACCESS_UA=1 ARW_ACCESS_UA_HASH=1 ARW_ACCESS_REF=1` when you need user-agent and referer fields (hashing keeps sensitive inputs obscured). Replace `t3hw00t` with your registry owner if you publish a forked image.

## Verification Checklist
- `GET /healthz` — liveness
- `GET /about` — metadata (port, presets, policy mode)
- `GET /state/runtime_matrix` — confirms discovery of local or remote workers as they land
- `GET /admin/probe` — effective state paths (requires admin token)
- `GET /state/egress/settings` — confirm DNS guard (`dns_guard_enable=true`) and proxy (`proxy_enable=true`) defaults remain enabled unless the deployment explicitly opted out
- `GET /metrics` — ensure `arw_legacy_capsule_headers_total` stays at zero before cutting over from legacy traffic

Keep the unified server as the default; the legacy bridge has been removed, so all surfaces run on `arw-server`.

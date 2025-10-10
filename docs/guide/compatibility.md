---
title: Compatibility Notes
---

# Compatibility Notes
Updated: 2025-10-10
Type: How‑to

This page summarizes high‑level notes and known issues when running ARW on
different operating systems and environments. It’s intended for users, not
developers — quick guidance on what to expect and how to resolve common
problems.

## Operating Systems

- Windows 10/11 (x64/ARM64)
  - Service and CLI run without admin rights.
  - Desktop Launcher (Tauri) requires the system WebView (WebView2). It is
    generally available on modern Windows and will be installed by the OS or
    the Tauri runtime if missing.
  - GPU/NPU acceleration depends on drivers (e.g., DirectML, vendor drivers);
    ARW gracefully falls back to CPU.

- macOS (Intel/Apple Silicon)
  - Works on recent macOS versions. The Desktop Launcher uses the system WebKit
    view; no additional runtime is required.
  - Apple Silicon is supported; acceleration depends on the selected backend
    (e.g., Metal via CoreML, when available in your adapter).

- Linux (x64/ARM64)
  - Service and CLI have minimal dependencies.
  - Desktop Launcher (Tauri 2) relies on WebKitGTK 4.1 and libsoup3. Ubuntu 24.04 LTS
    (or newer) ships those libraries; Ubuntu 22.04 is no longer supported because the
    required 4.1 packages do not exist there.
    - Debian testing/unstable, Ubuntu 24.04+: `sudo apt install -y libgtk-3-dev libwebkit2gtk-4.1-dev libjavascriptcoregtk-4.1-dev libsoup-3.0-dev`
    - Fedora: `sudo dnf install -y gtk3-devel webkit2gtk4.1-devel libsoup3-devel`
    - Arch: `sudo pacman -S --needed gtk3 webkit2gtk-4.1 libsoup3`
    - Stuck on a distro without WebKitGTK 4.1 packages (e.g., Ubuntu 22.04, Debian 12 stable)? Run the service headless instead: `bash scripts/start.sh --service-only --wait-health`, then open the browser Home view at `http://127.0.0.1:8091/admin/ui/control/`. Desktop-only actions (start/stop service, local log tails) still need the launcher or CLI. Saved Connections also let you point a desktop launcher running on another machine that meets the requirements.
    - `scripts/setup.sh` and `scripts/start.sh` emit a preflight warning when these libraries are missing so you can install them (`scripts/install-tauri-deps.sh`) before attempting another launcher build.
    - Use `scripts/setup.sh --headless` when you want the install to succeed without building the launcher (for example on Ubuntu 22.04); add `--minimal` if you only need `arw-server` and `arw-cli` without docs or packaging.
    - Desktop launcher opt-in: build it explicitly with `cargo build -p arw-launcher --features launcher-linux-ui`. Omit this step to stay headless.
  - Headless components (server/CLI) often continue to run on older glibc-based
    distros, but we only validate and support the full stack on Ubuntu 24.04 LTS+
    and equivalents.
  - Using Nix: `nix develop` provides the required libraries in the dev shell.

!!! note "GTK3 security advisories"
    `cargo audit`/Dependabot flag the GTK3-era crates (gtk/gdk/pango/glib, etc.) as unmaintained.
    They are still required for the Linux launcher until the upstream Wry/Tauri stack ships GTK4
    support. We acknowledge the advisories in `cargo-audit.toml` and will upgrade once the GTK4
    backend is released.

## Containers and Cloud

- Service in containers
  - Default deployment: use the unified `arw-server` image
    (`ghcr.io/t3hw00t/arw-server`; replace `t3hw00t` if you publish your own build).
    Expose it with hardened defaults:
    ```bash
    export ARW_ADMIN_TOKEN="${ARW_ADMIN_TOKEN:-$(openssl rand -hex 32)}"
    docker run --rm -p 8091:8091 \
      -e ARW_BIND=0.0.0.0 \
      -e ARW_PORT=8091 \
      -e ARW_ADMIN_TOKEN="$ARW_ADMIN_TOKEN" \
      ghcr.io/t3hw00t/arw-server:latest
    ```
    Substitute any equivalent secret generator if `openssl` is unavailable.
  - Desktop Launcher is not intended for headless containers; use a host
    desktop environment or run the Launcher outside the container.
  - GPU access in containers requires host support and appropriate device
    passthrough (vendor‑specific).

- Kubernetes
  - A sample Helm chart is provided. Ensure appropriate resource limits and, if
    applicable, GPU device plugins.

- Cloud VMs
  - Expect CPU fallback when accelerators are not available. Performance
    depends on VM size and storage bandwidth.

## WSL (Windows Subsystem for Linux)

- Service works inside WSL; expose port to Windows as needed.
- With WSLg, GUI apps can run, but Desktop Launcher is best run on the Windows
  host. Use the Launcher on Windows and point it at the service running in WSL.

## Filesystem & Disk Space

- Model downloads require adequate free space. ARW reserves a safety margin to
  avoid filling the disk completely. You can adjust the reserve via
  `ARW_MODELS_DISK_RESERVE_MB` (default: 256 MB). If there isn’t enough space,
  downloads error with an “insufficient disk space” message.
- Filenames from URLs are sanitized for cross‑platform compatibility; reserved
  characters are replaced and overly long names truncated (extension preserved).
- On Windows, if a file with the destination name already exists, finalize logic
  removes the old file and renames the temporary file.

## Networking

- The Desktop Launcher communicates with the service over `http://127.0.0.1:<port>`.
  Firewalls or endpoint security software may block local web requests. Allow
  local loopback where needed.
- If your environment requires an HTTP proxy, standard environment variables
  (e.g., `HTTP_PROXY`, `HTTPS_PROXY`) are respected by the underlying HTTP
  client.

## Permissions & Admin Access

- Sensitive service endpoints (e.g., models, memory, governor, introspection)
  are admin‑gated.
  - Development: set `ARW_DEBUG=1` to allow local admin access without a token.
  - Hardened: set `ARW_ADMIN_TOKEN` and send header `X-ARW-Admin: <token>`.
- The Launcher can store an admin token in its preferences (user config dir) and
  uses it automatically for admin actions.

## Known Issues / Tips

- WebView dependencies on Linux must match your distro; use the listed package
  names or `nix develop` for a preconfigured shell.
- Anti‑virus/EDR may slow down large file writes (model downloads). Consider
  excluding the ARW state directory if safe in your environment.
- SSE event streams rely on long‑lived HTTP connections; some corporate proxies
  or network policies may disrupt them. If you see frequent disconnects, review
  local proxy settings and firewall rules.

---

If you encounter a compatibility issue not covered here, please open an issue
with your OS version, environment (bare metal / VM / container), and a short
description.

## Hardware Detection (GPU/NPU)

ARW collects a best‑effort hardware snapshot for display in the Debug UI and for basic scheduling hints.

- GPUs
  - Linux: probes `/sys/class/drm` for discrete adapters and, when available, enriches vendor‑specific hints (e.g., NVIDIA model, AMD VRAM totals). If `ARW_ROCM_SMI=1`, ARW attempts an ROCm SMI JSON probe for metrics.
  - Cross-platform: when built with the `gpu_wgpu` feature, ARW enumerates adapters via `wgpu` across Vulkan/Metal/DX12/GL to report name/vendor/device/backend/type.
  - Windows/macOS: the `wgpu` probe provides a portable fallback. Additional platform probes may appear over time.

- NPUs
  - Linux: probes `/sys/class/accel` and scans kernel modules for hints (e.g., `intel_vpu`, `amdxdna`).
  - macOS: reports the Apple Neural Engine presence on Apple Silicon.
  - Windows (optional): when built with `npu_dxcore` and `ARW_DXCORE_NPU=1`, ARW uses DXCore to enumerate compute‑capable adapters as a proxy for NPU presence.

Notes
- These probes are read‑only and best‑effort; absence of a device in the snapshot does not prevent using an accelerator through a model runtime.
- Accelerator availability for inference depends on your chosen backend (e.g., ONNX Runtime, DirectML/ROCm/CUDA, CoreML) and its own driver/runtime requirements.

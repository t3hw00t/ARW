---
title: Compatibility Notes
---

# Compatibility Notes
Updated: 2025-09-12
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
  - Desktop Launcher (Tauri 2) uses WebKitGTK 4.1 and libsoup3. Install these
    packages to build locally:
    - Debian/Ubuntu: `sudo apt install -y libgtk-3-dev libwebkit2gtk-4.1-dev libjavascriptcoregtk-4.1-dev libsoup-3.0-dev`
    - Fedora: `sudo dnf install -y gtk3-devel webkit2gtk4.1-devel libsoup3-devel`
    - Arch: `sudo pacman -S --needed gtk3 webkit2gtk-4.1 libsoup3`
  - Using Nix: `nix develop` provides the required libraries in the dev shell.

## Containers and Cloud

- Service in containers
  - `arw-svc` runs fine in Docker/Podman containers; expose the HTTP port
    (default 8090). Example: `docker run -p 8090:8090 arw-svc:dev`.
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
  - Cross‑platform: when built with the `gpu_wgpu` feature (default in `arw-svc`), ARW enumerates adapters via `wgpu` across Vulkan/Metal/DX12/GL to report name/vendor/device/backend/type.
  - Windows/macOS: the `wgpu` probe provides a portable fallback. Additional platform probes may appear over time.

- NPUs
  - Linux: probes `/sys/class/accel` and scans kernel modules for hints (e.g., `intel_vpu`, `amdxdna`).
  - macOS: reports the Apple Neural Engine presence on Apple Silicon.
  - Windows (optional): when built with `npu_dxcore` and `ARW_DXCORE_NPU=1`, ARW uses DXCore to enumerate compute‑capable adapters as a proxy for NPU presence.

Notes
- These probes are read‑only and best‑effort; absence of a device in the snapshot does not prevent using an accelerator through a model runtime.
- Accelerator availability for inference depends on your chosen backend (e.g., ONNX Runtime, DirectML/ROCm/CUDA, CoreML) and its own driver/runtime requirements.

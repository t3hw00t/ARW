# ARW Deployment & Isolation Guide
Updated: 2025-09-06

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

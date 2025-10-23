---
title: Environment Modes
---

# Environment Modes

Updated: 2025-10-23
Type: Reference

## Overview

`arw` now tracks the active development platform so build artefacts, Python
virtualenvs, and helper scripts stay consistent when you hop between Linux,
Windows (host or WSL), and macOS. The current mode lives in `.arw-env` and is
enforced by `scripts/lib/env_mode.sh`, which every helper sources.

The available modes are:

| Mode          | When to use it                                     |
| ------------- | -------------------------------------------------- |
| `linux`       | Native Linux desktops, containers, or CI runners   |
| `windows-host`| Git Bash / PowerShell directly on Windows          |
| `windows-wsl` | Ubuntu (or other distros) inside Windows Subsystem |
| `mac`         | macOS hosts                                        |

Each mode owns its own `target/` and `.venv/` trees (`target.<mode>`,
`.venv-<mode>`). When you switch modes we stash the previous tree and reactivate
the one that belongs to the new environment so Cargo/Rust never mixes binaries
between platforms.

## Switching Modes

Run the switch script **inside the environment you want to activate**:

```bash
bash scripts/env/switch.sh windows-host
bash scripts/env/switch.sh windows-wsl
bash scripts/env/switch.sh linux
bash scripts/env/switch.sh mac
```

PowerShell (Windows host):

```powershell
scripts\env\switch.ps1 windows-host
```

Check the current state at any time with:

```bash
just env
```

The script:

1. Verifies that the host platform matches the requested mode.
2. Updates `.arw-env`.
3. Re-homes `target/` and `.venv/` for the new mode (storing the old copies as
   `target.<old>` / `.venv-<old>`).

If you see an error like:

```
[env-mode] Active environment mismatch:
  host:    windows-host
  current: windows-wsl (.arw-env)
Run: bash scripts/env/switch.sh windows-host
```

Run the suggested switch command and re-run your task.

## Mode Notes

### Linux

- Standard workflow; `target/` lives directly under the repo.
- Bootstrap via `bash scripts/setup.sh --yes`.

### Windows Host

This is the default workspace for Windows contributors; switch to WSL only when you specifically need Linux-only tooling.

- Use PowerShell as the primary interface (`pwsh -File scripts/dev.ps1 …`).
- Git Bash is used internally to run a few cross‑platform shell helpers, but WSL is not required.
- Requires a Rust toolchain installed via `rustup-init.exe`.
- When switching **from** WSL make sure no WSL processes are holding open the
  workspace so directory renames succeed.

### Windows WSL

 - Clone the repository inside the Linux filesystem (`/home/<user>`).
 - Run `bash scripts/env/switch.sh windows-wsl` from the WSL shell before using
   `scripts/dev.sh`.
 - Pip installs should stay inside the project virtualenv; global `sudo pip`
   remains blocked by policy.

### MacOS

- Requires the Homebrew toolchain (Rust via `rustup`, Python 3.11+, Node 18+).
- Switching away from macOS stashes `target` as `target.mac`.

## Directory Layout

- Active mode: `target/` and `.venv/`
- Stashed modes: `target.<mode>` and `.venv-<mode>`
- Sentinel: `target/.arw-mode` and `.venv/.arw-mode`

This layout lets you swap modes without cleaning builds—restoring your previous
environment just renames the stored directories back into place.

## CI & Automation

- Set `ARW_ENV_MODE_FORCE=<mode>` in headless environments to override any
  existing `.arw-env` value (GitHub Actions uses this to ensure Linux runners
  ignore developer-local settings).
- Restore caches per mode (`target` contents already record the mode via
  `target/.arw-mode`; when using `Swatinem/rust-cache`, set
  `shared-key: linux`/`windows-host`/etc.).

## Troubleshooting

- **Switch fails because directories are busy**: make sure shells or editors
  rooted in the repo are closed in the previous environment before switching.
- **Cargo still using the wrong artefacts**: delete the stale directory
  (`rm -rf target.unmanaged*`) and re-run `bash scripts/env/switch.sh <mode>`.
- **Git diff shows `.arw-env`**: the file is ignored, but double-check you did
  not accidentally stage it; reset if necessary.

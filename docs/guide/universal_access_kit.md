---
title: Universal Access Starter Kit
---

# Universal Access Starter Kit

Updated: 2025-10-23  
Type: How-to

The starter kit bundles eco-friendly defaults, offline documentation pointers, and a preview persona seed so anyone can launch ARW on a low-spec machine without relying on network access. Use it when preparing USB installers, classroom labs, or recovery media for trusted peers.

## Prerequisites
- Python 3.11 or newer (ships with `tomllib`)
- A cloned ARW workspace
- Optional: PowerShell 7+ on Windows for environment import helpers

## Generate the Kit
```bash
# From the repo root
python scripts/universal_access_kit.py --force --zip
```

This creates:

```
dist/universal-access-kit/
├── README.txt
├── docs/
│   ├── GOAL_ROADMAP_CROSSWALK.md
│   ├── offline_sync.md
│   ├── performance_presets.md
│   ├── persona_quickstart.md
│   ├── quickstart.md
│   └── runtime_quickstart.md
└── config/
    ├── eco-preset.env
    ├── kit-notes.md
    └── persona_seed.json
```

Passing `--zip` writes `dist/universal-access-kit.zip` alongside the folder so you can copy a single archive to other machines. Use `--check-only` (optionally with `--zip`) to validate an existing kit in place.

Validate a generated kit:
```bash
python scripts/universal_access_kit.py --check-only --zip
```

## Apply Eco Defaults

- **PowerShell**
  ```powershell
  Get-Content .\config\eco-preset.env |
    ForEach-Object {
      if ($_ -and $_ -notmatch '^#') {
        $name, $value = $_.Split('=', 2)
        Set-Item -Path Env:$name -Value $value
      }
    }
  ```

- **Bash / Zsh**
  ```bash
  set -a
  source ./config/eco-preset.env
  set +a
  ```

The file always includes `ARW_PERF_PRESET=eco` and the current eco tier overrides pulled from `configs/presets/examples.toml`, so it stays in sync with upstream preset tuning.

## Seed the Preview Persona

```bash
arw-cli admin persona seed --from ./config/persona_seed.json
```

The template keeps telemetry disabled by default. Update the JSON before seeding if you want to opt-in to vibe feedback (`preferences.telemetry.vibe.enabled: true`).

## Keep Getting Ready
- Read `config/kit-notes.md` and `README.txt` for validation steps (health checks, smoke runs, docs wheels).
- Generate offline MkDocs wheels when you have network access:
  ```bash
  # Linux/macOS
  bash scripts/dev.sh docs-cache

  # Windows PowerShell
  pwsh -File scripts\dev.ps1 docs-cache
  ```
- Copy the kit folder or zip to your target machine, then follow [Quickstart](quickstart.md) or [Runtime Quickstart](runtime_quickstart.md) locally.

## Automation Tips
- Use the bundled recipes: `just kit-universal` to rebuild + zip, `just kit-universal-check` to verify contents before publishing.
- Pair the kit with the `dist/docs-wheels.tar.gz` archive for a complete offline bundle.
- Re-run the generator whenever presets, docs, or persona templates shift; the script pulls live data from the repository each time.

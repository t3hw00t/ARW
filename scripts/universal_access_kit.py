#!/usr/bin/env python3
"""Assemble the ARW universal access starter kit.

The kit bundles eco-friendly presets, quick reference docs, and starter persona
material so new operators can get going without network access. By default the
assets land in ``dist/universal-access-kit``; pass ``--output`` to override the
directory or ``--zip`` to emit a zipped archive alongside the folder.
"""

from __future__ import annotations

import argparse
import json
import shutil
import sys
import zipfile
from pathlib import Path
import subprocess
import os

try:
    import tomllib  # Python 3.11+
except ModuleNotFoundError:  # pragma: no cover - guard for Python < 3.11
    try:
        import tomli as tomllib  # type: ignore
    except ModuleNotFoundError as exc:  # pragma: no cover - guardrail
        raise SystemExit(
            "python 3.11+ or the tomli package is required (pip install tomli)"
        ) from exc

REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_OUTPUT = REPO_ROOT / "dist" / "universal-access-kit"

DOCS_TO_COPY = [
    "docs/guide/quickstart.md",
    "docs/guide/runtime_quickstart.md",
    "docs/guide/offline_sync.md",
    "docs/guide/performance_presets.md",
    "docs/guide/persona_quickstart.md",
    "docs/GOAL_ROADMAP_CROSSWALK.md",
]

DOC_FILENAMES = [Path(p).name for p in DOCS_TO_COPY]
CONFIG_FILENAMES = ["eco-preset.env", "persona_seed.json", "kit-notes.md"]
ROOT_FILENAMES = ["README.txt", "README.html"]

PERSONA_TEMPLATE = {
    "id": "persona-example",
    "owner_kind": "workspace",
    "name": "Starter Companion",
    "archetype": "coach",
    "traits": {
        "tone": "warm",
        "focus": ["learning", "planning"],
        "limits": ["never store secrets", "prioritise consent"],
    },
    "preferences": {
        "responses": {
            "cite_sources": True,
            "summary_first": True,
            "format": "plain_text",
        },
        "telemetry": {
            "vibe": {
                "enabled": False,
                "scope": "workspace",
            }
        },
    },
    "worldview": {
        "principles": [
            "Respect local data boundaries.",
            "Offer reflective prompts when confidence is low.",
        ]
    },
    "vibe_profile": {},
    "calibration": {
        "confidence_floor": 0.4,
        "last_reviewed": None,
    },
}

README_TEMPLATE = """\
Universal Access Starter Kit
============================

This bundle collects the minimum artefacts you need to bootstrap ARW on a
low-spec, offline-friendly machine. Everything is safe to customise. Items:

docs/
  Quickstart guides, offline sync instructions, and the goal ↔ roadmap crosswalk.

config/
  eco-preset.env      -> environment variables for the eco tier
  persona_seed.json   -> starter persona template
  kit-notes.md        -> reminder checklist

bin/ (optional)
  arw-mini-dashboard  -> tiny offline watcher for read-models (built if Cargo is available)

Get Started
-----------

1. (Optional) Copy `eco-preset.env` next to your launch script and source it:

   Windows PowerShell::
      Get-Content .\\config\\eco-preset.env |
        ForEach-Object {{ if ($_ -notmatch '^#') {{ $name,$value = $_.Split('=',2); Set-Item -Path Env:$name -Value $value }} }}

   Bash::
      set -a && source ./config/eco-preset.env && set +a

2. Seed a persona locally (preview feature):

   `arw-cli admin persona seed --from ./config/persona_seed.json`

3. Generate offline docs wheels if you need MkDocs without a network connection:

   `scripts/dev.sh docs-cache` (Bash) or `scripts\dev.ps1 docs-cache` (PowerShell)

4. Review `config/kit-notes.md` for validation steps (ports, health check, smoke).

5. When ready, you can repackage the kit (with local modifications) by rerunning:

   `python scripts/universal_access_kit.py --output <dir> --zip`

Need more context? See docs/guide/offline_sync.md and docs/guide/quickstart.md.
"""

README_HTML = """<!doctype html>
<html lang=\"en\"><head>
  <meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">
  <title>ARW Universal Access Kit</title>
  <style>
    body{font-family:system-ui,-apple-system,Segoe UI,Roboto,Ubuntu,\"Helvetica Neue\",sans-serif;line-height:1.45;margin:24px;max-width:860px}
    h1{margin:0 0 8px 0} .dim{color:#6b7280}
    .card{border:1px solid #e5e7eb;border-radius:10px;padding:16px;margin:12px 0;background:#fff}
    code, pre{background:#f8fafc;border:1px solid #e5e7eb;border-radius:6px;padding:2px 4px}
    a.button{display:inline-block;border:1px solid #111827;padding:8px 12px;border-radius:8px;text-decoration:none}
  </style>
  </head><body>
  <h1>Universal Access Starter Kit</h1>
  <p class=\"dim\">Offline entry page. Open the docs site if present, or the Markdown quickstarts below.</p>

  <div class=\"card\">
    <h2>Docs</h2>
    <ul>
      <li><a href=\"site/index.html\">Open offline docs site</a> (if the <code>site/</code> folder is present)</li>
      <li><a href=\"docs/quickstart.md\">Quickstart (Markdown)</a></li>
      <li><a href=\"docs/runtime_quickstart.md\">Runtime Quickstart</a></li>
      <li><a href=\"docs/offline_sync.md\">Offline &amp; Sync</a></li>
      <li><a href=\"docs/performance_presets.md\">Performance Presets</a></li>
      <li><a href=\"docs/persona_quickstart.md\">Persona Quickstart</a></li>
    </ul>
  </div>

  <div class=\"card\">
    <h2>Config</h2>
    <ul>
      <li><code>config/eco-preset.env</code> — eco tier environment variables</li>
      <li><code>config/persona_seed.json</code> — starter persona</li>
      <li><code>config/kit-notes.md</code> — checklist &amp; smoke</li>
    </ul>
  </div>

  <div class=\"card\">
    <h2>Mini Dashboard</h2>
    <p>If bundled, run: <code>./bin/arw-mini-dashboard --base http://127.0.0.1:8091</code></p>
  </div>

  <p class=\"dim\">See also README.txt for a plain text version.</p>
  </body></html>
"""

KIT_NOTES_MD = """\
# Starter Kit Checklist

- [ ] Source `eco-preset.env` before launching the server.
- [ ] Set `ARW_ADMIN_TOKEN` explicitly and record it securely.
- [ ] Run `scripts/start.ps1 -ServiceOnly -WaitHealth` (Windows) or
      `scripts/start.sh --service-only --wait-health` (Linux/macOS).
- [ ] Verify `/healthz` and `/about` while connected locally.
- [ ] Optional: run `scripts/dev.sh verify --fast` (or PowerShell variant) once.
- [ ] Capture persona updates through `/state/persona` before enabling empathy features.
- [ ] Generate docs wheels with `scripts/dev.sh docs-cache` (or `scripts\\dev.ps1 docs-cache`) so future rebuilds remain offline-friendly.

## Quick Smoke

- [ ] If `bin/arw-mini-dashboard` exists, run a one-shot metrics check:

      `./bin/arw-mini-dashboard --base http://127.0.0.1:8091 --id route_stats --json --once`

      Expect a JSON object with `by_path` populated. If not present, confirm the server is healthy and generating `route_stats`.

- [ ] Otherwise, from the repo root (with Rust installed):

      `mise run mini:dashboard ID=route_stats` (Ctrl+C to exit)
"""


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output",
        type=Path,
        default=DEFAULT_OUTPUT,
        help=f"destination directory (default: {DEFAULT_OUTPUT})",
    )
    parser.add_argument(
        "--force",
        action="store_true",
        help="remove the output directory before building the kit",
    )
    parser.add_argument(
        "--zip",
        action="store_true",
        help="produce a zip archive alongside the folder output",
    )
    parser.add_argument(
        "--check-only",
        action="store_true",
        help="validate an existing kit at --output (optionally with --zip) and exit",
    )
    return parser.parse_args(argv)


def load_eco_env() -> dict[str, str]:
    preset_path = REPO_ROOT / "configs" / "presets" / "examples.toml"
    data = tomllib.loads(preset_path.read_text(encoding="utf-8"))
    eco = data.get("eco")
    if not eco:
        raise RuntimeError("could not locate eco preset in configs/presets/examples.toml")
    # Always include the top-level preset marker.
    env = {"ARW_PERF_PRESET": "eco"}
    env.update({key: str(value) for key, value in eco.items()})
    return env


def write_env_file(path: Path, env: dict[str, str]) -> None:
    with path.open("w", encoding="utf-8") as handle:
        handle.write("# Eco tier environment variables (source before launching ARW)\n")
        for key, value in env.items():
            handle.write(f"{key}={value}\n")


def write_persona_seed(path: Path) -> None:
    with path.open("w", encoding="utf-8") as handle:
        json.dump(PERSONA_TEMPLATE, handle, indent=2)
        handle.write("\n")


def copy_docs(destination: Path) -> None:
    for relative in DOCS_TO_COPY:
        source = REPO_ROOT / relative
        target = destination / Path(relative).name
        target.write_text(source.read_text(encoding="utf-8"), encoding="utf-8")


def assemble_kit(output_dir: Path, force: bool, zip_output: bool) -> tuple[Path, Path | None]:
    if output_dir.exists():
        if not force:
            raise SystemExit(f"[kit] output directory exists: {output_dir} (use --force)")
        shutil.rmtree(output_dir)
    (output_dir / "docs").mkdir(parents=True, exist_ok=True)
    (output_dir / "config").mkdir(parents=True, exist_ok=True)
    (output_dir / "bin").mkdir(parents=True, exist_ok=True)

    copy_docs(output_dir / "docs")
    write_env_file(output_dir / "config" / "eco-preset.env", load_eco_env())
    write_persona_seed(output_dir / "config" / "persona_seed.json")
    (output_dir / "README.txt").write_text(README_TEMPLATE, encoding="utf-8")
    (output_dir / "README.html").write_text(README_HTML, encoding="utf-8")
    (output_dir / "config" / "kit-notes.md").write_text(KIT_NOTES_MD, encoding="utf-8")

    # Optional site build if mkdocs is available
    try:
        mk = shutil.which("mkdocs")
        if mk:
            site_dir = output_dir / "site"
            site_dir.mkdir(parents=True, exist_ok=True)
            subprocess.run(
                [mk, "build", "--strict", "-f", str(REPO_ROOT / "mkdocs.yml"), "-d", str(site_dir)],
                cwd=str(REPO_ROOT),
                check=True,
            )
            print(f"[kit] mkdocs site built at {site_dir}")
        else:
            print("[kit] mkdocs not found; skipping site build")
    except Exception as exc:
        print(f"[kit] mkdocs build failed: {exc}")

    # Optional mini dashboard build if cargo is available
    try:
        cargo = shutil.which("cargo")
        if cargo:
            subprocess.run([cargo, "build", "-p", "arw-mini-dashboard", "--release"], cwd=str(REPO_ROOT), check=True)
            exe = "arw-mini-dashboard.exe" if sys.platform.startswith("win") else "arw-mini-dashboard"
            src = REPO_ROOT / "target" / "release" / exe
            if src.exists():
                shutil.copy2(src, output_dir / "bin" / exe)
                print(f"[kit] bundled mini dashboard: {exe}")
            else:
                print("[kit] mini dashboard binary not found after build")
        else:
            print("[kit] cargo not found; skipping mini dashboard build")
    except Exception as exc:
        print(f"[kit] mini dashboard build failed: {exc}")

    zip_path: Path | None = None
    if zip_output:
        archive_path = shutil.make_archive(
            base_name=str(output_dir),
            format="zip",
            root_dir=output_dir.parent,
            base_dir=output_dir.name,
        )
        zip_path = Path(archive_path)
        print(f"[kit] wrote archive {archive_path}")
    print(f"[kit] assets written to {output_dir}")
    # Optional extras (skip in CI or when ARW_KIT_SKIP_OPTIONAL is set)
    skip_optional = os.environ.get("ARW_KIT_SKIP_OPTIONAL", "0").lower() in ("1", "true", "yes") or bool(os.environ.get("CI"))

    if skip_optional:
        print("[kit] skipping optional extras (site and binary) due to CI/ARW_KIT_SKIP_OPTIONAL")
    else:
        # Optional site build if mkdocs is available
        try:
            mk = shutil.which("mkdocs")
            if mk:
                site_dir = output_dir / "site"
                site_dir.mkdir(parents=True, exist_ok=True)
                subprocess.run(
                    [mk, "build", "--strict", "-f", str(REPO_ROOT / "mkdocs.yml"), "-d", str(site_dir)],
                    cwd=str(REPO_ROOT),
                    check=True,
                )
                print(f"[kit] mkdocs site built at {site_dir}")
            else:
                print("[kit] mkdocs not found; skipping site build")
        except Exception as exc:
            print(f"[kit] mkdocs build failed: {exc}")

        # Optional mini dashboard build if cargo is available
        try:
            cargo = shutil.which("cargo")
            if cargo:
                subprocess.run([cargo, "build", "-p", "arw-mini-dashboard", "--release"], cwd=str(REPO_ROOT), check=True)
                exe = "arw-mini-dashboard.exe" if sys.platform.startswith("win") else "arw-mini-dashboard"
                src = REPO_ROOT / "target" / "release" / exe
                if src.exists():
                    shutil.copy2(src, output_dir / "bin" / exe)
                    print(f"[kit] bundled mini dashboard: {exe}")
                else:
                    print("[kit] mini dashboard binary not found after build")
            else:
                print("[kit] cargo not found; skipping mini dashboard build")
        except Exception as exc:
            print(f"[kit] mini dashboard build failed: {exc}")

    return output_dir, zip_path


def validate_kit(base: Path, zip_path: Path | None = None) -> list[str]:
    errors: list[str] = []
    if not base.exists():
        return [f"missing kit directory: {base}"]

    docs_dir = base / "docs"
    config_dir = base / "config"
    if not docs_dir.is_dir():
        errors.append(f"missing docs directory: {docs_dir}")
    if not config_dir.is_dir():
        errors.append(f"missing config directory: {config_dir}")

    for filename in ROOT_FILENAMES:
        if not (base / filename).is_file():
            errors.append(f"missing file: {filename}")

    for filename in DOC_FILENAMES:
        if not (docs_dir / filename).is_file():
            errors.append(f"missing doc file: docs/{filename}")

    env_path = config_dir / "eco-preset.env"
    if env_path.is_file():
        env_text = env_path.read_text(encoding="utf-8")
        if "ARW_PERF_PRESET=eco" not in env_text:
            errors.append("eco-preset.env missing ARW_PERF_PRESET=eco")
    else:
        errors.append("missing config file: config/eco-preset.env")

    persona_path = config_dir / "persona_seed.json"
    if persona_path.is_file():
        try:
            data = json.loads(persona_path.read_text(encoding="utf-8"))
            if not isinstance(data, dict) or data.get("id") != "persona-example":
                errors.append("persona_seed.json unexpected structure or id")
        except json.JSONDecodeError as exc:
            errors.append(f"persona_seed.json invalid JSON: {exc}")
    else:
        errors.append("missing config file: config/persona_seed.json")

    kit_notes = config_dir / "kit-notes.md"
    if not kit_notes.is_file():
        errors.append("missing config file: config/kit-notes.md")

    if zip_path:
        if not zip_path.exists():
            errors.append(f"missing zip archive: {zip_path}")
        else:
            try:
                with zipfile.ZipFile(zip_path) as archive:
                    members = set(archive.namelist())
                base_prefix = f"{base.name}/"
                expected_members = {base_prefix + name for name in ROOT_FILENAMES}
                expected_members.update(f"{base_prefix}docs/{name}" for name in DOC_FILENAMES)
                expected_members.update(f"{base_prefix}config/{name}" for name in CONFIG_FILENAMES)
                for member in expected_members:
                    if member not in members:
                        errors.append(f"zip archive missing {member}")
            except zipfile.BadZipFile as exc:
                errors.append(f"zip archive corrupted: {exc}")
    return errors


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    output_dir = args.output.resolve()

    if args.check_only:
        zip_path = output_dir.with_suffix(".zip") if args.zip else None
        errors = validate_kit(output_dir, zip_path)
        if errors:
            for msg in errors:
                print(f"[kit-check] {msg}", file=sys.stderr)
            return 1
        print(f"[kit-check] {output_dir} OK")
        if zip_path:
            print(f"[kit-check] {zip_path} OK")
        return 0

    kit_dir, archive_path = assemble_kit(output_dir, args.force, args.zip)
    errors = validate_kit(kit_dir, archive_path)
    if errors:
        for msg in errors:
            print(f"[kit] validation error: {msg}", file=sys.stderr)
        return 1
    print(f"[kit] validation passed for {kit_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))




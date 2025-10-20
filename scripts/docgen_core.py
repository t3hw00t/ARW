#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Callable, Iterable

ROOT = Path(__file__).resolve().parent.parent
DOCS_DIR = ROOT / "docs"
SPEC_DIR = ROOT / "spec"
STATUS_MD = DOCS_DIR / "developer" / "status.md"
TASKS_JSON = ROOT / ".arw" / "tasks.json"
TASKS_MD = DOCS_DIR / "developer" / "tasks.md"
GEN_COMMENT = "<!-- GENERATED FILE: scripts/docgen_core.py; do not edit by hand -->"
SAFE_TRUE = {"1", "true", "yes", "on"}


def log_info(message: str) -> None:
    print(f"[docgen] {message}")


def log_warn(message: str) -> None:
    print(f"[docgen] warning: {message}", file=sys.stderr)


def log_error(message: str) -> None:
    print(f"[docgen] error: {message}", file=sys.stderr)


def which(cmd: str) -> str | None:
    return shutil.which(cmd)


class Runner:
    def __init__(self, quiet: bool = False):
        self.failures: list[str] = []
        self.warnings: list[str] = []
        self.quiet = quiet

    def run(
        self,
        label: str,
        cmd: Iterable[str],
        *,
        required: bool = True,
        env: dict[str, str] | None = None,
    ) -> bool:
        if not self.quiet:
            log_info(f"{label} ({' '.join(cmd)})")
        try:
            result = subprocess.run(
                list(cmd),
                cwd=ROOT,
                env=env,
                check=False,
            )
        except FileNotFoundError:
            msg = f"{label}: command not found ({cmd})"
            self.warnings.append(msg)
            log_warn(msg)
            return False
        if result.returncode != 0:
            msg = f"{label} exited with {result.returncode}"
            if required:
                self.failures.append(msg)
                log_error(msg)
            else:
                self.warnings.append(msg)
                log_warn(msg)
            return False
        return True

    def capture(
        self,
        label: str,
        cmd: Iterable[str],
        *,
        required: bool = True,
        env: dict[str, str] | None = None,
        text: bool = True,
    ) -> str | None:
        if not self.quiet:
            log_info(f"{label} ({' '.join(cmd)})")
        try:
            result = subprocess.run(
                list(cmd),
                cwd=ROOT,
                env=env,
                check=False,
                text=text,
                capture_output=True,
            )
        except FileNotFoundError:
            msg = f"{label}: command not found ({cmd})"
            self.warnings.append(msg)
            log_warn(msg)
            return None
        if result.returncode != 0:
            msg = f"{label} exited with {result.returncode}"
            if required:
                self.failures.append(msg)
                log_error(msg)
            else:
                self.warnings.append(msg)
                log_warn(f"{msg}\n{result.stderr.strip()}")
            return None
        return result.stdout if text else result.stdout.decode()


def normalize_generated_markdown(text: str) -> str:
    lines = text.replace("\r\n", "\n").split("\n")
    normalized: list[str] = []
    for line in lines:
        if line.startswith("Generated: "):
            normalized.append("Generated: <timestamp>")
        elif line.startswith("Updated: "):
            normalized.append("Updated: <timestamp>")
        else:
            normalized.append(line)
    return "\n".join(normalized)


def ensure_updated_from_generated(markdown: str) -> str:
    """Insert or refresh an Updated line based on the Generated timestamp."""
    if "Generated:" not in markdown:
        return markdown
    ends_with_newline = markdown.endswith("\n")
    lines = markdown.replace("\r\n", "\n").split("\n")
    try:
        generated_idx = next(i for i, line in enumerate(lines) if line.startswith("Generated: "))
    except StopIteration:
        return markdown
    generated_value = lines[generated_idx].split("Generated:", 1)[1].strip()
    generated_date = generated_value.split(" ", 1)[0] if generated_value else ""
    if not generated_date:
        return markdown
    updated_line = f"Updated: {generated_date}"
    try:
        updated_idx = next(i for i, line in enumerate(lines) if line.startswith("Updated: "))
    except StopIteration:
        updated_idx = None
    if updated_idx is not None:
        lines[updated_idx] = updated_line
    else:
        # Place directly above the Generated line to match manual stamps.
        lines.insert(generated_idx, updated_line)
    normalized = "\n".join(lines)
    if ends_with_newline and not normalized.endswith("\n"):
        normalized += "\n"
    return normalized


def normalize_generated_json(text: str) -> str:
    try:
        payload = json.loads(text)
    except json.JSONDecodeError:
        return text.replace("\r\n", "\n")
    if isinstance(payload, dict) and "generated" in payload:
        payload = dict(payload)
        payload["generated"] = "<timestamp>"
    return json.dumps(payload, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def write_if_changed(
    dest: Path, content: str, normalizer: Callable[[str], str] | None = None
) -> None:
    if dest.exists():
        existing = dest.read_text(encoding="utf-8")
        existing_norm = normalizer(existing) if normalizer else existing
        incoming_norm = normalizer(content) if normalizer else content
        if existing_norm == incoming_norm:
            if normalizer:
                log_info(f"Skipping update for {dest} (timestamp-only changes)")
            return
    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.write_text(content, encoding="utf-8")


def guess_repo_blob_base(runner: Runner) -> str:
    env_override = os.environ.get("REPO_BLOB_BASE")
    if env_override:
        return env_override.rstrip("/") + "/"
    git = which("git")
    if not git:
        return "https://github.com/t3hw00t/ARW/blob/main/"
    remote = runner.capture(
        "Reading git remote",
        [git, "config", "--get", "remote.origin.url"],
        required=False,
    )
    if not remote:
        return "https://github.com/t3hw00t/ARW/blob/main/"
    remote = remote.strip()
    slug = None
    if remote.startswith("git@github.com:"):
        slug = remote.split("git@github.com:", 1)[1]
    elif "github.com/" in remote:
        slug = remote.split("github.com/", 1)[1]
    if not slug:
        return "https://github.com/t3hw00t/ARW/blob/main/"
    slug = slug.rstrip("/")
    if slug.endswith(".git"):
        slug = slug[:-4]
    return f"https://github.com/{slug}/blob/main/"


def write_status_doc(metadata: dict, repo_base: str) -> None:
    packages = metadata.get("packages", [])
    libs: list[tuple[str, str, Path]] = []
    bins: list[tuple[str, str, Path]] = []
    for pkg in packages:
        manifest = Path(pkg["manifest_path"]).resolve()
        try:
            rel_manifest = manifest.relative_to(ROOT)
        except ValueError:
            # Skip packages outside the repository (e.g. toolchains)
            continue
        kinds = {kind for target in pkg.get("targets", []) for kind in target.get("kind", [])}
        entry = (pkg["name"], pkg["version"], rel_manifest)
        if "lib" in kinds:
            libs.append(entry)
        if "bin" in kinds:
            bins.append(entry)

    libs.sort(key=lambda item: item[0])
    bins.sort(key=lambda item: item[0])
    timestamp = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M")

    lines = [
        "---",
        "title: Workspace Status",
        "---",
        "",
        "# Workspace Status",
        "",
        GEN_COMMENT,
        "",
        f"Generated: {timestamp} UTC",
        "",
        "## Libraries",
    ]
    if libs:
        for name, version, rel_manifest in libs:
            rel = "/".join(rel_manifest.parts)
            lines.append(f"- **{name}**: {version} — [{rel}]({repo_base}{rel})")
    else:
        lines.append("_none_")
    lines.extend(["", "## Binaries"])
    if bins:
        for name, version, rel_manifest in bins:
            rel = "/".join(rel_manifest.parts)
            lines.append(f"- **{name}**: {version} — [{rel}]({repo_base}{rel})")
    else:
        lines.append("_none_")
    lines.append("")
    STATUS_MD.parent.mkdir(parents=True, exist_ok=True)
    STATUS_MD.write_text("\n".join(lines), encoding="utf-8")


def update_tasks_docs(runner: Runner) -> None:
    timestamp = datetime.now(timezone.utc)
    timestamp_str = timestamp.strftime("%Y-%m-%d %H:%M")
    timestamp_label = f"{timestamp_str} UTC"
    existing_tasks_json = None
    existing_updated = None
    existing_text = None
    tasks_data = {"updated": timestamp_label, "tasks": []}
    if TASKS_JSON.exists():
        existing_text = TASKS_JSON.read_text(encoding="utf-8")
        try:
            tasks_data = json.loads(existing_text)
        except json.JSONDecodeError as exc:
            msg = f"Failed to parse {TASKS_JSON}: {exc}"
            runner.warnings.append(msg)
            log_warn(msg)
            tasks_data = {"updated": timestamp_label, "tasks": []}
        else:
            tasks = tasks_data.get("tasks") or []
            if not isinstance(tasks, list):
                tasks = []
            tasks_data["tasks"] = tasks
            existing_tasks_json = json.dumps(tasks, sort_keys=True)
            existing_updated = tasks_data.get("updated")

    if existing_tasks_json is None:
        existing_tasks_json = json.dumps(tasks_data.get("tasks", []), sort_keys=True)

    current_tasks_json = json.dumps(tasks_data.get("tasks", []), sort_keys=True)
    if current_tasks_json == existing_tasks_json and existing_updated:
        tasks_data["updated"] = existing_updated
    else:
        tasks_data["updated"] = timestamp_label

    serialized = json.dumps(tasks_data, indent=2) + "\n"
    if existing_text is None or existing_text != serialized:
        TASKS_JSON.parent.mkdir(parents=True, exist_ok=True)
        TASKS_JSON.write_text(serialized, encoding="utf-8")

    tasks = tasks_data.get("tasks", [])
    grouped: dict[str, list[dict]] = defaultdict(list)
    for task in tasks:
        status = (task.get("status") or "todo").lower()
        grouped[status].append(task)
    for bucket in grouped.values():
        bucket.sort(key=lambda item: item.get("updated") or "", reverse=True)

    sections = [
        ("todo", "To Do"),
        ("in_progress", "In Progress"),
        ("paused", "Paused"),
        ("done", "Done"),
    ]
    lines = [
        "---",
        "title: Tasks Status",
        "---",
        "",
        "# Tasks Status",
        "",
        GEN_COMMENT,
        "",
        f"Updated: {timestamp_label}",
        "",
    ]
    for key, title in sections:
        lines.append(f"## {title}")
        entries = grouped.get(key, [])
        if not entries:
            lines.append("")
            continue
        lines.extend(format_task_entry(entry) for entry in entries)
        lines.append("")
    TASKS_MD.parent.mkdir(parents=True, exist_ok=True)
    TASKS_MD.write_text("\n".join(lines).rstrip() + "\n", encoding="utf-8")


def format_task_entry(task: dict) -> str:
    task_id = task.get("id") or "?"
    title = task.get("title") or "(untitled)"
    status = (task.get("status") or "todo").lower()
    updated = task.get("updated")
    line = f"- [{task_id}] {title} — {status}"
    if updated:
        line += f" (updated: {updated})"
    notes = task.get("notes") or []
    if isinstance(notes, list) and notes:
        note_lines = []
        for note in notes:
            if not isinstance(note, dict):
                continue
            time = note.get("time") or ""
            text = note.get("text") or ""
            note_lines.append(f"  - {time}: {text}")
        if note_lines:
            line += "\n" + "\n".join(note_lines)
    return line


def run_docgen(args: argparse.Namespace) -> int:
    runner = Runner(quiet=args.quiet)
    python_cmd = sys.executable
    repo_base = guess_repo_blob_base(runner)

    # Validation and doc regeneration (required for correctness)
    scripts = [
        ("Validating feature registry", ROOT / "scripts" / "check_feature_integrity.py"),
        ("Validating system component registry", ROOT / "scripts" / "check_system_components_integrity.py"),
        ("Generating feature matrix", ROOT / "scripts" / "gen_feature_matrix.py"),
        ("Generating feature catalog", ROOT / "scripts" / "gen_feature_catalog.py"),
        ("Generating system components doc", ROOT / "scripts" / "gen_system_components.py"),
        ("Regenerating topics reference", ROOT / "scripts" / "gen_topics_doc.py"),
    ]
    for label, path in scripts:
        runner.run(label, [python_cmd, str(path)])

    cargo_cmd = which("cargo")
    metadata = None
    if cargo_cmd:
        stdout = runner.capture(
            "Collecting cargo metadata",
            [cargo_cmd, "metadata", "--no-deps", "--locked", "--format-version", "1"],
        )
        if stdout:
            try:
                metadata = json.loads(stdout)
            except json.JSONDecodeError as exc:
                msg = f"cargo metadata parsing failed: {exc}"
                runner.failures.append(msg)
                log_error(msg)
    else:
        runner.warnings.append("cargo not found; skipping status doc generation")
        log_warn("cargo not found; skipping status doc generation")

    if metadata:
        write_status_doc(metadata, repo_base)
    else:
        runner.warnings.append("Status doc not refreshed (missing cargo metadata)")

    update_tasks_docs(runner)

    skip_builds = args.skip_builds or os.environ.get("ARW_DOCGEN_SKIP_BUILDS", "").lower() in SAFE_TRUE
    build_ok = False
    if skip_builds:
        log_info("Skipping release builds (ARW_DOCGEN_SKIP_BUILDS enabled)")
    elif cargo_cmd:
        build_ok = runner.run(
            "Building release arw-server/arw-cli",
            [cargo_cmd, "build", "--release", "--locked", "-p", "arw-server", "-p", "arw-cli"],
            required=False,
        )
    else:
        runner.warnings.append("cargo not available; skipping release builds")
        log_warn("cargo not available; skipping release builds")

    exe_suffix = ".exe" if os.name == "nt" else ""
    cli_path = ROOT / "target" / "release" / f"arw-cli{exe_suffix}"
    server_path = ROOT / "target" / "release" / f"arw-server{exe_suffix}"

    if build_ok and cli_path.exists():
        generate_gating_docs(runner, cli_path)
        generate_mcp_tools(runner, cli_path)
    elif build_ok:
        runner.warnings.append("arw-cli binary missing after build; skipping gating docs")
        log_warn("arw-cli binary missing after build; skipping gating docs")

    if build_ok and server_path.exists():
        generate_openapi(runner, server_path, python_cmd)
    elif build_ok:
        runner.warnings.append("arw-server binary missing after build; skipping OpenAPI export")
        log_warn("arw-server binary missing after build; skipping OpenAPI export")

    if runner.failures:
        log_error(f"{len(runner.failures)} step(s) failed")
        return 1
    if runner.warnings and not runner.quiet:
        log_warn(f"{len(runner.warnings)} warning(s) emitted")
    return 0


def generate_gating_docs(runner: Runner, cli_path: Path) -> None:
    docs_dir = DOCS_DIR
    (docs_dir / "reference").mkdir(parents=True, exist_ok=True)

    commands = [
        (
            "Rendering gating keys (json)",
            [str(cli_path), "gate", "keys", "--json", "--pretty"],
            docs_dir / "GATING_KEYS.json",
            normalize_generated_json,
        ),
        (
            "Rendering gating keys (markdown)",
            [str(cli_path), "gate", "keys", "--doc"],
            docs_dir / "GATING_KEYS.md",
            normalize_generated_markdown,
        ),
        (
            "Rendering gating config schema",
            [str(cli_path), "gate", "config", "schema", "--pretty"],
            docs_dir / "reference" / "gating_config.schema.json",
            None,
        ),
        (
            "Rendering gating config reference",
            [str(cli_path), "gate", "config", "doc"],
            docs_dir / "reference" / "gating_config.md",
            normalize_generated_markdown,
        ),
    ]
    for label, cmd, dest, normalizer in commands:
        stdout = runner.capture(label, cmd, required=False)
        if stdout is None:
            continue
        if dest.name == "GATING_KEYS.md":
            stdout = ensure_updated_from_generated(stdout)
        write_if_changed(dest, stdout, normalizer)


def generate_mcp_tools(runner: Runner, cli_path: Path) -> None:
    stdout = runner.capture(
        "Rendering MCP tools spec",
        [str(cli_path), "tools"],
        required=False,
    )
    if stdout is None:
        return
    SPEC_DIR.mkdir(parents=True, exist_ok=True)
    (SPEC_DIR / "mcp-tools.json").write_text(stdout, encoding="utf-8")


def generate_openapi(runner: Runner, server_path: Path, python_cmd: str) -> None:
    SPEC_DIR.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env["OPENAPI_OUT"] = str(SPEC_DIR / "openapi.yaml")
    runner.run("Exporting OpenAPI spec", [str(server_path)], required=False, env=env)
    runner.run(
        "Normalizing OpenAPI descriptions",
        [python_cmd, str(ROOT / "scripts" / "ensure_openapi_descriptions.py")],
        required=False,
    )


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Cross-platform doc generation helper.")
    parser.add_argument(
        "--skip-builds",
        action="store_true",
        help="Skip release builds and downstream artifacts (respects ARW_DOCGEN_SKIP_BUILDS).",
    )
    parser.add_argument(
        "--quiet",
        action="store_true",
        help="Suppress informational logs.",
    )
    return parser.parse_args(argv)


if __name__ == "__main__":
    sys.exit(run_docgen(parse_args(sys.argv[1:])))

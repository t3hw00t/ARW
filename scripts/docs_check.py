#!/usr/bin/env python3
"""Cross-platform docs check helper.

Reimplements scripts/docs_check.sh so Windows hosts are no longer required to
run Git Bash. Behaviour matches the original Bash script closely, including
environment variables, warnings, and exit codes.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Iterable, List, Tuple


BLUE = "\033[34m"
YELLOW = "\033[33m"
RED = "\033[31m"
RESET = "\033[0m"


class DocsCheck:
    def __init__(self, repo_root: Path, docs_dir: Path, mkdocs_yml: Path) -> None:
        self.repo_root = repo_root
        self.docs_dir = docs_dir
        self.mkdocs_yml = mkdocs_yml
        self.errors = 0
        self.warnings = 0

    # Logging helpers -----------------------------------------------------
    def info(self, message: str) -> None:
        print(f"{BLUE}[docs-check]{RESET} {message}")

    def warn(self, message: str) -> None:
        self.warnings += 1
        print(f"{YELLOW}[warn]{RESET} {message}")

    def error(self, message: str) -> None:
        self.errors += 1
        print(f"{RED}[error]{RESET} {message}")

    # Helpers --------------------------------------------------------------
    @staticmethod
    def _normalize_title(value: str) -> str:
        value = value.lower()
        return "".join(ch for ch in value if ch.isalnum() or ch == " ")

    def _heading_not_title_case(self, heading: str) -> bool:
        stripped = heading.lstrip("# ")
        return bool(stripped) and stripped[0].islower()

    # MkDocs build --------------------------------------------------------
    def run_mkdocs(self, skip: bool, fast_mode: bool) -> None:
        if skip:
            self.warn("Skipping mkdocs build (--skip-mkdocs or DOCS_CHECK_SKIP_MKDOCS=1)")
            return

        self.info("Building docs with mkdocs --strict to catch nav issues")
        venv_mkdocs = self.repo_root / ".venv" / "bin" / "mkdocs"
        if os.name == "nt":
            venv_mkdocs = self.repo_root / ".venv" / "Scripts" / "mkdocs.exe"

        mkdocs_cmd: List[str]
        if venv_mkdocs.exists():
            mkdocs_cmd = [str(venv_mkdocs)]
        else:
            mkdocs_path = shutil.which("mkdocs")
            if not mkdocs_path:
                self.warn("mkdocs not found; skipping build check")
                self.warn(
                    "Install docs toolchain via 'mise run bootstrap:docs' or 'bash scripts/bootstrap_docs.sh'."
                )
                return
            mkdocs_cmd = [mkdocs_path]

        mkdocs_cmd.extend(["build", "--strict", "-f", str(self.mkdocs_yml)])
        proc = subprocess.run(mkdocs_cmd, cwd=self.repo_root)
        if proc.returncode != 0:
            self.error("mkdocs build --strict failed")

    # Markdown scans ------------------------------------------------------
    def scan_markdown(self) -> None:
        files = sorted(self.docs_dir.rglob("*.md"))
        for path in files:
            rel = path.relative_to(self.docs_dir)
            text = path.read_text(encoding="utf-8")
            lines = text.splitlines()

            # Front-matter title vs H1
            fm_title = ""
            if lines and lines[0].strip() == "---":
                for idx in range(1, len(lines)):
                    line = lines[idx]
                    if line.strip() == "---":
                        break
                    if line.startswith("title:"):
                        fm_title = line.split(":", 1)[1].strip()
                        break

            h1 = ""
            for line in lines:
                if line.startswith("# "):
                    h1 = line[2:].strip()
                    break

            if fm_title and h1:
                if self._normalize_title(fm_title) != self._normalize_title(h1):
                    self.warn(f"{rel}: title/front-matter and H1 differ — '{fm_title}' vs '{h1}'")

            # Updated/Generated line check (first 40 lines)
            found_marker = any(
                re.match(r"\s*(Updated:|Generated:|_Last updated:|_Generated |Base: )", line)
                for line in lines[:40]
            )
            if not found_marker:
                self.warn(f"{rel}: missing Updated:/Generated: information")

            # Heading case for H2/H3
            for line in lines:
                if line.startswith("## ") or line.startswith("### "):
                    if self._heading_not_title_case(line):
                        heading_text = line.lstrip("# ").strip()
                        self.warn(f"{rel}: heading not Title Case -> '{heading_text}'")

    # ripgrep helpers -----------------------------------------------------
    def _run_rg(self, pattern: str, search_path: Path, options: Iterable[str]) -> str:
        rg = shutil.which("rg")
        if not rg:
            raise FileNotFoundError
        args = [rg, "--no-messages", "--with-filename", "--line-number"]
        args.extend(options)
        args.append(pattern)
        args.append(str(search_path))
        proc = subprocess.run(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        return proc.stdout.strip()

    def run_rg_checks(self, fast_mode: bool) -> None:
        rg = shutil.which("rg")
        if not rg:
            self.warn("ripgrep (rg) not found; skipping legacy term sweeps. Install via 'mise install' or 'brew install ripgrep'.")
            return

        if fast_mode:
            self.warn("Skipping legacy admin route scan (fast mode)")
        else:
            try:
                output = self._run_rg(
                    "/admin/(state|projects)/",
                    self.repo_root,
                    [
                        "--glob", "!docs/**",
                        "--glob", "!spec/**",
                        "--glob", "!.git/**",
                        "--glob", "!target/**",
                        "--glob", "!site/**",
                        "--glob", "!vendor/**",
                        "--glob", "!sandbox/**",
                        "--glob", "!node_modules/**",
                        "--glob", "!dist/**",
                    ],
                )
            except FileNotFoundError:
                output = ""
            if output:
                self.error("Legacy admin route references detected:")
                for line in output.splitlines():
                    print(f"  {line}")
                self.errors += len(output.splitlines()) - 1  # already incremented once above

        if fast_mode:
            self.warn("Skipping capsule header sweep (fast mode)")
        else:
            try:
                output = self._run_rg(
                    "X-ARW-Gate",
                    self.repo_root,
                    [
                        "--glob", "!docs/**",
                        "--glob", "!target/**",
                        "--glob", "!site/**",
                        "--glob", "!vendor/**",
                        "--glob", "!sandbox/**",
                        "--glob", "!node_modules/**",
                        "--glob", "!dist/**",
                        "--glob", "!spec/**",
                    ],
                )
            except FileNotFoundError:
                output = ""
            if output:
                filtered: List[str] = []
                for line in output.splitlines():
                    if len(line) > 2 and line[1:3] in (":\\", ":/"):
                        parts = line.split(":", 2)
                        if len(parts) >= 2:
                            file_part = parts[0] + ":" + parts[1]
                        else:
                            file_part = line
                    else:
                        file_part = line.split(":", 1)[0]
                    norm = file_part.replace("\\", "/")
                    if norm.endswith("apps/arw-server/src/capsule_guard.rs") or \
                       norm.endswith("scripts/docs_check.py") or \
                       norm.endswith("scripts/check_legacy_surface.sh") or \
                       norm.endswith("CHANGELOG.md"):
                        continue
                    filtered.append(line)
                if filtered:
                    self.error("Legacy capsule header detected:")
                    for line in filtered:
                        print(f"  {line}")
                    self.errors += len(filtered) - 1

        if fast_mode:
            self.warn("Skipping legacy Models.* sweep (fast mode)")
        else:
            try:
                output = self._run_rg(
                    "Models\\.(?!\\*)",
                    self.docs_dir,
                    ["--pcre2", "--glob", "!release_notes.md"],
                )
            except FileNotFoundError:
                output = ""
            if output:
                self.error("Legacy Models.* references detected:")
                for line in output.splitlines():
                    print(f"  {line}")
                self.errors += len(output.splitlines()) - 1

    # Link check ----------------------------------------------------------
    def link_check(self, fast_mode: bool) -> None:
        if fast_mode:
            self.warn("Skipping relative link scan (fast mode)")
            return

        self.info("Checking relative links to .md files")
        errors = 0
        for root, _, files in os.walk(self.docs_dir):
            for name in files:
                if not name.endswith(".md"):
                    continue
                path = Path(root) / name
                rel = path.relative_to(self.docs_dir)
                try:
                    text = path.read_text(encoding="utf-8")
                except Exception as exc:  # noqa: BLE001
                    print(f"[error] {rel}: cannot read: {exc}")
                    errors += 1
                    continue
                pattern = re.compile(r"\[[^\]]+\]\(([^)]+\.md)(#[^)]+)?\)")
                for match in pattern.finditer(text):
                    href = match.group(1)
                    if href.startswith("http://") or href.startswith("https://"):
                        continue
                    target = (path.parent / href).resolve()
                    if not target.exists():
                        print(f"[error] {rel}: broken link → {href}")
                        errors += 1

        if errors:
            self.errors += errors


def parse_args() -> Tuple[bool, bool]:
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--skip-mkdocs", action="store_true")
    parser.add_argument("--fast", action="store_true")
    parser.add_argument("-h", "--help", action="help")
    ns = parser.parse_args()

    skip_mkdocs = bool(int(os.environ.get("DOCS_CHECK_SKIP_MKDOCS", "0")))
    fast_env = bool(int(os.environ.get("DOCS_CHECK_FAST", "0")))

    if ns.fast or fast_env:
        return True, True
    return ns.skip_mkdocs or skip_mkdocs, False


def main() -> None:
    repo_root = Path(__file__).resolve().parent.parent
    docs_dir = repo_root / "docs"
    mkdocs_yml = repo_root / "mkdocs.yml"

    skip_mkdocs, fast_mode = parse_args()

    checker = DocsCheck(repo_root, docs_dir, mkdocs_yml)

    if fast_mode:
        checker.warn("DOCS_CHECK_FAST enabled: skipping mkdocs build and Python-based sweeps.")

    checker.run_mkdocs(skip_mkdocs or fast_mode, fast_mode)
    checker.info("Scanning markdown files under docs/")
    checker.scan_markdown()
    checker.run_rg_checks(fast_mode)
    checker.link_check(fast_mode)

    checker.info(f"Done. warnings={checker.warnings} errors={checker.errors}")
    if checker.errors > 0:
        checker.error(f"Found {checker.errors} errors")
        raise SystemExit(1)


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Extract Prometheus/Grafana ops assets into a ready-to-deploy directory."""

from __future__ import annotations

import argparse
import os
import re
import sys
from pathlib import Path


def extract_block(source: Path, fence: str) -> str:
    text = source.read_text(encoding="utf-8")
    pattern = re.compile(rf"```{re.escape(fence)}\s*(.*?)```", re.DOTALL)
    match = pattern.search(text)
    if not match:
        raise SystemExit(f"could not find fenced block `{fence}` in {source}")
    # Strip trailing whitespace so files stay tidy and add a single newline.
    return match.group(1).rstrip() + "\n"


def export_ops_assets(out_dir: Path) -> None:
    repo_root = Path(__file__).resolve().parents[1]
    out_dir = out_dir if out_dir.is_absolute() else repo_root / out_dir
    out_dir.mkdir(parents=True, exist_ok=True)

    assets = [
        (
            "docs/snippets/prometheus_recording_rules.md",
            "yaml",
            out_dir / "prometheus_recording_rules.yaml",
        ),
        (
            "docs/snippets/prometheus_alerting_rules.md",
            "yaml",
            out_dir / "prometheus_alerting_rules.yaml",
        ),
        (
            "docs/snippets/grafana_quick_panels.md",
            "json",
            out_dir / "grafana_quick_panels.json",
        ),
    ]

    for rel_path, fence, target in assets:
        source = repo_root / rel_path
        content = extract_block(source, fence)
        target.write_text(content, encoding="utf-8")

    print(f"export-ops assets written to {out_dir}")


def main(args: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--out",
        help="Destination directory (defaults to $ARW_EXPORT_OUTDIR or ops/out)",
    )
    namespace = parser.parse_args(args)

    out_dir_env = os.environ.get("ARW_EXPORT_OUTDIR")
    out_dir = namespace.out or out_dir_env or "ops/out"

    export_ops_assets(Path(out_dir))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

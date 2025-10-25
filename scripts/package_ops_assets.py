#!/usr/bin/env python3
"""Export ops assets and bundle them into a zip archive."""

from __future__ import annotations

import argparse
import sys
import zipfile
from pathlib import Path


def export_assets(out_dir: Path) -> None:
    repo_root = Path(__file__).resolve().parents[1]
    sys.path.insert(0, str(repo_root / "scripts"))
    from export_ops_assets import export_ops_assets  # type: ignore

    export_ops_assets(out_dir)


def main(args: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--out",
        help="Directory where assets will be written prior to packaging (default: ops/out)",
    )
    parser.add_argument(
        "--archive",
        help="Destination archive path (default: dist/ops-assets.zip)",
    )
    namespace = parser.parse_args(args)

    repo_root = Path(__file__).resolve().parents[1]

    out_dir = Path(namespace.out or "ops/out")
    if not out_dir.is_absolute():
        out_dir = repo_root / out_dir
    out_dir.mkdir(parents=True, exist_ok=True)

    archive_path = Path(namespace.archive or "dist/ops-assets.zip")
    if not archive_path.is_absolute():
        archive_path = repo_root / archive_path
    archive_path.parent.mkdir(parents=True, exist_ok=True)

    export_assets(out_dir)

    if archive_path.exists():
        archive_path.unlink()

    with zipfile.ZipFile(archive_path, "w", compression=zipfile.ZIP_DEFLATED) as zf:
        for path in out_dir.iterdir():
            if path.is_file():
                zf.write(path, arcname=path.name)

    print(f"package-ops archive created at {archive_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

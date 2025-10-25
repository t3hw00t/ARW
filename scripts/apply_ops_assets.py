#!/usr/bin/env python3
"""
Deploy packaged Prometheus/Grafana assets and trigger optional reloads.

This helper is cross-platform and works with the archive produced by
`scripts/package_ops_assets.py`. It copies recording/alerting rules into the
target directory, optionally imports the Grafana dashboard via HTTP, and can
poke the Prometheus/Alertmanager reload endpoints when provided.

Examples
--------

Copy assets and reload Prometheus:

    python scripts/apply_ops_assets.py ^
        --archive dist/ops-assets.zip ^
        --rules-dir C:\prom\rules ^
        --prometheus-reload http://127.0.0.1:9090/-/reload

Import Grafana dashboard:

    python scripts/apply_ops_assets.py \
        --archive dist/ops-assets.zip \
        --rules-dir /etc/prometheus/rules \
        --grafana-url https://grafana.example.com \
        --grafana-api-key $GRAFANA_API_KEY \
        --grafana-folder 0
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import sys
import tempfile
import urllib.error
import urllib.request
import zipfile
from pathlib import Path
from typing import Optional

DEFAULT_ARCHIVE = "dist/ops-assets.zip"


def copy_if_exists(src: Path, dest_dir: Path) -> Optional[Path]:
    if not src.exists():
        return None
    dest_dir.mkdir(parents=True, exist_ok=True)
    target = dest_dir / src.name
    shutil.copy2(src, target)
    return target


def post(url: str, data: Optional[bytes], headers: dict[str, str]) -> None:
    request = urllib.request.Request(url, data=data, headers=headers, method="POST")
    with urllib.request.urlopen(request) as response:  # noqa: S310 - trusted admin endpoint
        response.read()


def import_grafana_dashboard(
    grafana_url: str,
    api_key: str,
    dashboard: Path,
    folder_id: int,
    message: str,
) -> None:
    payload = {
        "dashboard": json.loads(dashboard.read_text(encoding="utf-8")),
        "folderId": folder_id,
        "overwrite": True,
        "message": message,
    }
    body = json.dumps(payload).encode("utf-8")
    headers = {
        "Content-Type": "application/json",
        "Authorization": f"Bearer {api_key}",
    }
    url = grafana_url.rstrip("/") + "/api/dashboards/db"
    post(url, body, headers)


def hit_reload(endpoint: str, name: str) -> None:
    try:
        post(endpoint, b"", {})
    except urllib.error.URLError as exc:  # pragma: no cover - network specific
        raise SystemExit(f"failed to hit {name} reload endpoint {endpoint}: {exc}") from exc


def extract_archive(archive: Path, temp_dir: Path) -> None:
    with zipfile.ZipFile(archive) as zf:
        zf.extractall(temp_dir)


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--archive",
        default=os.environ.get("ARW_OPS_ARCHIVE", DEFAULT_ARCHIVE),
        help=f"Path to ops assets archive (default: %(default)s)",
    )
    parser.add_argument(
        "--rules-dir",
        required=True,
        help="Destination directory for Prometheus rule files",
    )
    parser.add_argument(
        "--prometheus-reload",
        help="Optional Prometheus reload endpoint (e.g. http://127.0.0.1:9090/-/reload)",
    )
    parser.add_argument(
        "--alertmanager-reload",
        help="Optional Alertmanager reload endpoint",
    )
    parser.add_argument(
        "--grafana-url",
        help="Grafana base URL (include https://)",
    )
    parser.add_argument(
        "--grafana-api-key",
        help="Grafana API token (required when --grafana-url is set)",
    )
    parser.add_argument(
        "--grafana-folder",
        type=int,
        default=0,
        help="Grafana folder ID for import (default: root folder 0)",
    )
    parser.add_argument(
        "--grafana-message",
        default="ARW ops asset import",
        help="Commit message attached to Grafana dashboard imports",
    )
    args = parser.parse_args(argv)

    archive = Path(args.archive).resolve()
    if not archive.exists():
        raise SystemExit(f"archive not found: {archive}")

    rules_dir = Path(args.rules_dir).resolve()

    with tempfile.TemporaryDirectory(prefix="arw-ops-assets-") as tmpdir:
        extract_archive(archive, Path(tmpdir))
        staging_candidates = [
            Path(tmpdir),
            Path(tmpdir) / "ops",
            Path(tmpdir) / "ops" / "out",
        ]
        staging = None
        for candidate in staging_candidates:
            if (candidate / "prometheus_recording_rules.yaml").exists():
                staging = candidate
                break
        if staging is None:
            raise SystemExit(
                "archive does not contain prometheus_recording_rules.yaml; "
                "regenerate with scripts/package_ops_assets.py"
            )

        recording = copy_if_exists(staging / "prometheus_recording_rules.yaml", rules_dir)
        alerting = copy_if_exists(staging / "prometheus_alerting_rules.yaml", rules_dir)
        grafana = staging / "grafana_quick_panels.json"

        print("copied assets:")
        if recording:
            print(f"  - {recording}")
        if alerting:
            print(f"  - {alerting}")
        print(f"  - grafana panels: {grafana if grafana.exists() else 'missing'}")

        if args.grafana_url:
            if not args.grafana_api_key:
                raise SystemExit("--grafana-api-key is required when --grafana-url is set")
            if grafana.exists():
                print(f"importing grafana dashboard into folder {args.grafana_folder} …")
                import_grafana_dashboard(
                    args.grafana_url,
                    args.grafana_api_key,
                    grafana,
                    args.grafana_folder,
                    args.grafana_message,
                )
                print("grafana import complete")
            else:
                print("warning: grafana JSON missing; skipping import")

    if args.prometheus_reload:
        print(f"hitting Prometheus reload endpoint {args.prometheus_reload} …")
        hit_reload(args.prometheus_reload, "Prometheus")
        print("Prometheus reload complete")

    if args.alertmanager_reload:
        print(f"hitting Alertmanager reload endpoint {args.alertmanager_reload} …")
        hit_reload(args.alertmanager_reload, "Alertmanager")
        print("Alertmanager reload complete")

    print("ops asset deployment completed")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

#!/usr/bin/env python3
"""Validate that runtime bundle catalogs include consent metadata for audio/vision modalities."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Iterable, List

REQUIRED_MODALITIES = {"audio", "vision"}
BASE_DIR = Path(__file__).resolve().parent.parent
CONFIG_DIR = BASE_DIR / "configs" / "runtime"


def parse_args(argv: Iterable[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Validate that runtime bundle catalogs include consent metadata for audio/vision modalities.",
    )
    parser.add_argument(
        "catalogs",
        nargs="*",
        help="Specific catalog paths to validate (defaults to configs/runtime/bundles*.json).",
    )
    return parser.parse_args(list(argv))


def load_catalog(path: Path) -> dict:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:  # pragma: no cover - fails fast
        raise SystemExit(f"{path}: invalid JSON: {exc}")


def bundle_needs_consent(bundle: dict) -> bool:
    modalities = bundle.get("modalities", [])
    return any(mod in REQUIRED_MODALITIES for mod in modalities)


def has_consent_metadata(bundle: dict) -> bool:
    metadata = bundle.get("metadata")
    if not isinstance(metadata, dict):
        return False
    consent = metadata.get("consent")
    return isinstance(consent, dict)


def validate_catalog(path: Path) -> List[str]:
    data = load_catalog(path)
    errors: List[str] = []
    for bundle in data.get("bundles", []):
        bundle_id = bundle.get("id", "<unknown>")
        if bundle_needs_consent(bundle) and not has_consent_metadata(bundle):
            errors.append(
                f"{path}: bundle '{bundle_id}' missing metadata.consent for audio/vision modalities"
            )
    return errors


def run(catalogs: Iterable[Path]) -> List[str]:
    all_errors: List[str] = []
    for catalog_path in catalogs:
        all_errors.extend(validate_catalog(catalog_path))
    return all_errors


def main(argv: Iterable[str]) -> int:
    args = parse_args(argv)
    if args.catalogs:
        catalog_paths = [Path(arg) for arg in args.catalogs]
    else:
        catalog_paths = sorted(CONFIG_DIR.glob("bundles*.json"))

    if not catalog_paths:
        print("No runtime bundle catalogs found; skipping consent validation.")
        return 0

    errors = run(catalog_paths)
    if errors:
        for line in errors:
            print(line, file=sys.stderr)
        return 1

    print("Consent metadata validation passed")
    return 0


if __name__ == "__main__":  # pragma: no cover
    sys.exit(main(sys.argv[1:]))

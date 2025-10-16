#!/usr/bin/env python3
"""
Generate managed runtime bundle catalogs with artifact metadata and optional signing.

This helper scans bundle workspaces created by the packaging pipeline (e.g.
``dist/bundles/<bundle-id>``), computes SHA-256 digests for each artifact, and
rewrites the matching catalog file under ``configs/runtime`` so operators receive
fresh URLs and integrity metadata. When a publishing key is supplied the script
can also sign each bundle manifest in place using the ``arw-cli`` helper.

Typical usage (preview channel):

    python scripts/runtime_bundle_publish.py \
      --bundle-root dist/bundles/preview \
      --catalog configs/runtime/bundles.llama.json \
      --base-url https://ghcr.io/t3hw00t/arw-bundles \
      --sign-key-file ops/keys/runtime_bundle_ed25519.sk \
      --sign-issuer bundle-ci@arw \
      --sign-key-id preview-bundle-signing
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Dict, Iterable, List, Optional, Tuple


ArtifactMap = Dict[int, Path]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--bundle-root",
        required=True,
        type=Path,
        help="Root directory containing bundle workspaces (e.g. dist/bundles/preview)",
    )
    parser.add_argument(
        "--catalog",
        required=True,
        type=Path,
        help="Catalog JSON to update (e.g. configs/runtime/bundles.llama.json)",
    )
    parser.add_argument(
        "--channel",
        type=str,
        help="Override the catalog channel label",
    )
    parser.add_argument(
        "--notes",
        type=str,
        help="Override catalog notes",
    )
    parser.add_argument(
        "--base-url",
        type=str,
        help=(
            "Base URL prepended to artifact filenames "
            "(e.g. https://ghcr.io/<org>/arw-bundles)"
        ),
    )
    parser.add_argument(
        "--output",
        type=Path,
        help="Write the catalog to a different path instead of rewriting --catalog",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Preview catalog updates without writing files",
    )
    parser.add_argument(
        "--sign",
        action="store_true",
        help="Sign bundle manifests located under each bundle workspace",
    )
    parser.add_argument(
        "--sign-key-b64",
        type=str,
        help="Base64 encoded ed25519 private key for manifest signing",
    )
    parser.add_argument(
        "--sign-key-file",
        type=Path,
        help="File containing base64 encoded ed25519 private key",
    )
    parser.add_argument(
        "--sign-key-id",
        type=str,
        help="Optional key identifier for signatures (defaults to CLI fingerprint)",
    )
    parser.add_argument(
        "--sign-issuer",
        type=str,
        help="Optional issuer label embedded in the signature entry",
    )
    parser.add_argument(
        "--sign-cli",
        type=str,
        default="arw-cli",
        help="Path to arw-cli when signing manifests (default: arw-cli on PATH)",
    )
    parser.add_argument(
        "--sign-compact",
        action="store_true",
        help="Write compact JSON when signing manifests (defaults to pretty output)",
    )
    return parser.parse_args()


def load_json(path: Path) -> Dict:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def write_json(path: Path, payload: Dict) -> None:
    serialized = json.dumps(payload, indent=2, sort_keys=False)
    with path.open("w", encoding="utf-8") as handle:
        handle.write(serialized)
        handle.write("\n")


def iter_files(path: Path) -> Iterable[Path]:
    if not path.exists():
        return []
    for entry in sorted(path.iterdir()):
        if entry.is_file():
            yield entry


def pick_artifact_files(entries: List[Dict], artifact_dir: Path) -> ArtifactMap:
    """
    Heuristically map artifact entries to files discovered on disk.

    Strategy:
      * If the counts match, zip by index.
      * Otherwise try to select by file extension derived from artifact.format.
      * Fall back to lexicographically sorted files.
    """
    files = list(iter_files(artifact_dir))
    mapping: ArtifactMap = {}
    if not files:
        return mapping

    if len(files) == len(entries):
        return {idx: files[idx] for idx in range(len(entries))}

    remaining = files.copy()
    for idx, entry in enumerate(entries):
        fmt = entry.get("format")
        if not fmt:
            continue
        suffix = f".{fmt}".replace("..", ".")
        match = next((f for f in remaining if f.name.endswith(suffix)), None)
        if match:
            mapping[idx] = match
            remaining.remove(match)

    # Assign leftovers in lexical order
    for idx in range(len(entries)):
        if idx in mapping:
            continue
        if not remaining:
            break
        mapping[idx] = remaining.pop(0)
    return mapping


def sha256sum(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


@dataclass
class SigningOptions:
    cli_binary: str
    key_b64: Optional[str]
    key_file: Optional[Path]
    key_id: Optional[str]
    issuer: Optional[str]
    compact: bool

    def resolve_key_b64(self) -> Optional[str]:
        if self.key_b64:
            return self.key_b64.strip()
        if self.key_file:
            data = self.key_file.read_text(encoding="utf-8").strip()
            return data if data else None
        return None


def sign_manifest(manifest: Path, options: SigningOptions) -> None:
    key_b64 = options.resolve_key_b64()
    if not key_b64:
        raise RuntimeError(
            "Signing requested but no key provided "
            "(supply --sign-key-b64 or --sign-key-file)"
        )

    cmd = [
        options.cli_binary,
        "runtime",
        "bundles",
        "manifest",
        "sign",
        str(manifest),
        "--key-b64",
        key_b64,
    ]
    if options.key_id:
        cmd.extend(["--key-id", options.key_id])
    if options.issuer:
        cmd.extend(["--issuer", options.issuer])
    if options.compact:
        cmd.append("--compact")

    try:
        subprocess.run(cmd, check=True)
    except FileNotFoundError as exc:
        raise RuntimeError(f"arw-cli not found ({options.cli_binary})") from exc
    except subprocess.CalledProcessError as exc:
        raise RuntimeError(f"arw-cli manifest sign failed ({exc.returncode})") from exc


def update_catalog(
    catalog: Dict,
    bundle_root: Path,
    base_url: Optional[str],
) -> Tuple[Dict, List[str]]:
    updates: List[str] = []
    base_url = base_url.rstrip("/") if base_url else None

    bundles = catalog.get("bundles", [])
    for bundle in bundles:
        bundle_id = bundle.get("id")
        if not bundle_id:
            continue
        workspace = bundle_root / bundle_id
        artifacts_dir = workspace / "artifacts"
        artifact_entries = bundle.get("artifacts", [])
        mapping = pick_artifact_files(artifact_entries, artifacts_dir)
        if not mapping:
            updates.append(f"{bundle_id}: no artifacts discovered under {artifacts_dir}")
            continue

        for idx, entry in enumerate(artifact_entries):
            file_path = mapping.get(idx)
            if not file_path:
                continue
            digest = sha256sum(file_path)
            size_bytes = file_path.stat().st_size
            entry["sha256"] = digest
            entry["size_bytes"] = size_bytes
            if base_url:
                entry["url"] = f"{base_url}/{bundle_id}/{file_path.name}"
            updates.append(
                f"{bundle_id}: {file_path.name} sha256={digest[:12]}… size={size_bytes}"
            )

    return catalog, updates


def main() -> int:
    args = parse_args()

    catalog = load_json(args.catalog)
    if args.channel is not None:
        catalog["channel"] = args.channel
    if args.notes is not None:
        catalog["notes"] = args.notes

    bundle_root = args.bundle_root
    if not bundle_root.exists():
        print(f"[runtime-bundles] bundle root not found: {bundle_root}", file=sys.stderr)

    catalog, updates = update_catalog(catalog, bundle_root, args.base_url)

    for line in updates:
        print(f"[runtime-bundles] {line}")

    if args.dry_run:
        print("[runtime-bundles] dry-run requested; catalog not written")
    else:
        output = args.output or args.catalog
        write_json(output, catalog)
        print(f"[runtime-bundles] wrote catalog: {output}")

    if args.sign:
        signing = SigningOptions(
            cli_binary=args.sign_cli,
            key_b64=args.sign_key_b64,
            key_file=args.sign_key_file,
            key_id=args.sign_key_id,
            issuer=args.sign_issuer,
            compact=args.sign_compact,
        )
        bundles = catalog.get("bundles", [])
        for bundle in bundles:
            bundle_id = bundle.get("id")
            if not bundle_id:
                continue
            manifest_path = bundle_root / bundle_id / "bundle.json"
            if not manifest_path.exists():
                print(
                    f"[runtime-bundles] skipping manifest signing "
                    f"(missing {manifest_path})",
                    file=sys.stderr,
                )
                continue
            try:
                sign_manifest(manifest_path, signing)
                print(
                    f"[runtime-bundles] signed manifest for bundle {bundle_id} "
                    f"({manifest_path})"
                )
            except RuntimeError as exc:
                print(f"[runtime-bundles] {exc}", file=sys.stderr)
                return 1

    return 0


if __name__ == "__main__":
    raise SystemExit(main())

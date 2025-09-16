#!/usr/bin/env python3
"""Validate feature registry metadata for consistency.

Checks:
- all feature IDs are unique
- dependencies point to known feature IDs
- SSOT file paths exist on disk
- referenced event topics exist in crates/arw-topics/src/lib.rs
- feature catalog covers every feature exactly once
"""
import json
import pathlib
import re
import sys
from collections import Counter

ROOT = pathlib.Path(__file__).resolve().parents[1]
FEATURES_JSON = ROOT / "interfaces" / "features.json"
CATALOG_JSON = ROOT / "interfaces" / "feature_catalog.json"
TOPICS_RS = ROOT / "crates" / "arw-topics" / "src" / "lib.rs"


def load_json(path: pathlib.Path) -> dict:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:
        raise SystemExit(f"error: missing {path}") from exc


def gather_topics(source: pathlib.Path) -> set[str]:
    text = source.read_text(encoding="utf-8")
    pattern = re.compile(r"pub const [A-Z0-9_]+: &str = \"([^\"]+)\";")
    return {match.group(1) for match in pattern.finditer(text)}


def main() -> int:
    features_doc = load_json(FEATURES_JSON)
    catalog_doc = load_json(CATALOG_JSON)
    topic_names = gather_topics(TOPICS_RS)

    features = features_doc.get("features", [])
    if not isinstance(features, list):
        raise SystemExit("error: interfaces/features.json missing 'features' list")

    feature_ids = [f.get("id") for f in features if f.get("id")]
    duplicates = [item for item, count in Counter(feature_ids).items() if count > 1]

    errors: list[str] = []
    warnings: list[str] = []
    if duplicates:
        errors.append(f"duplicate feature ids: {', '.join(sorted(duplicates))}")

    feature_map = {f.get("id"): f for f in features if f.get("id")}

    # Dependency validation
    for feature in features:
        fid = feature.get("id", "(unknown)")
        for dep in feature.get("deps", []):
            if dep not in feature_map:
                errors.append(f"{fid} depends on unknown feature '{dep}'")

    # Topic validation
    for feature in features:
        fid = feature.get("id", "(unknown)")
        for topic in feature.get("topics", []):
            if topic not in topic_names:
                errors.append(f"{fid} references unknown topic '{topic}'")

    # SSOT validation
    for feature in features:
        fid = feature.get("id", "(unknown)")
        for entry in feature.get("ssot", []):
            path = entry.get("path")
            if not path:
                continue
            candidate = ROOT / path
            if not candidate.exists():
                errors.append(f"{fid} lists missing SSOT path '{path}'")

    # Catalog coverage: every feature should appear exactly once
    catalog_counts: Counter[str] = Counter()
    for pillar in catalog_doc.get("pillars", []):
        for journey in pillar.get("journeys", []):
            for fid in journey.get("features", []):
                catalog_counts[fid] += 1

    missing_in_catalog = sorted(feature_map.keys() - catalog_counts.keys())
    if missing_in_catalog:
        errors.append(
            "catalog is missing features: " + ", ".join(missing_in_catalog)
        )

    duplicated_in_catalog = {
        fid: count for fid, count in catalog_counts.items() if count > 1
    }
    if duplicated_in_catalog:
        warnings.append(
            "catalog references features multiple times: "
            + ", ".join(f"{fid} (x{count})" for fid, count in sorted(duplicated_in_catalog.items()))
        )

    unknown_catalog_refs = sorted(set(catalog_counts.keys()) - set(feature_map.keys()))
    if unknown_catalog_refs:
        errors.append(
            "catalog references unknown feature ids: "
            + ", ".join(unknown_catalog_refs)
        )

    if errors:
        for err in errors:
            print(f"error: {err}", file=sys.stderr)
        return 1

    if warnings:
        for warn in warnings:
            print(f"warning: {warn}", file=sys.stderr)

    print(
        f"validated {len(features)} features, {len(topic_names)} topics, "
        f"and catalog coverage ({sum(catalog_counts.values())} assignments)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

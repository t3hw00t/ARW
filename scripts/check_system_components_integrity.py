#!/usr/bin/env python3
from __future__ import annotations

import json
import sys
from typing import Dict, List, Set, Tuple

from doc_utils import ROOT, check_paths_exist, parse_topics_rs

COMPONENTS_JSON = ROOT / "interfaces" / "system_components.json"
FEATURES_JSON = ROOT / "interfaces" / "features.json"


def load_json(path):
    with path.open("r", encoding="utf-8") as fh:
        return json.load(fh)


def collect_doc_paths(component: dict, feature: dict | None) -> List[str]:
    paths: List[str] = []
    if feature:
        for entry in feature.get("docs", []):
            if isinstance(entry, dict):
                val = entry.get("path")
            else:
                val = entry
            if val:
                paths.append(val)
    for entry in component.get("docs", []):
        if isinstance(entry, dict):
            val = entry.get("path")
        else:
            val = entry
        if val:
            paths.append(val)
    return paths


def collect_topics(component: dict) -> Set[str]:
    topics: Set[str] = set()
    for topic in component.get("signals", []):
        if topic:
            topics.add(topic)
    for topic in component.get("interfaces", {}).get("topics", []) or []:
        if topic:
            topics.add(topic)
    return topics


def parse_http_entry(entry) -> Tuple[str | None, str | None]:
    method: str | None = None
    path: str | None = None
    if isinstance(entry, str):
        raw = entry.strip()
        if not raw:
            return method, path
        parts = raw.split(None, 1)
        if len(parts) == 2 and parts[0].isalpha():
            method = parts[0].upper()
            path = parts[1].strip()
        else:
            path = raw
    elif isinstance(entry, dict):
        raw_method = entry.get("method")
        raw_path = entry.get("path")
        if raw_method:
            method = str(raw_method).strip().upper() or None
        if raw_path:
            path = str(raw_path).strip() or None
    return method, path


def main() -> int:
    try:
        components_doc = load_json(COMPONENTS_JSON)
    except Exception as exc:
        print(f"error: failed to load system components: {exc}", file=sys.stderr)
        return 2
    try:
        features_doc = load_json(FEATURES_JSON)
    except Exception as exc:
        print(f"error: failed to load features registry: {exc}", file=sys.stderr)
        return 2

    features: Dict[str, dict] = {
        entry.get("id"): entry for entry in features_doc.get("features", []) if entry.get("id")
    }
    component_ids = {comp.get("id") for comp in components_doc.get("components", []) if comp.get("id")}

    errors: List[str] = []

    known_topics = parse_topics_rs(include_defaults={"state.read.model.patch"})

    for comp in components_doc.get("components", []):
        cid = comp.get("id")
        feature = None
        fid = comp.get("feature_id")
        if fid:
            feature = features.get(fid)
            if feature is None:
                errors.append(f"component {cid or '(missing id)'} references unknown feature_id '{fid}'")
        feature_http: Set[Tuple[str | None, str | None]] = set()
        feature_http_paths: Set[str] = set()
        if feature:
            for http_entry in feature.get("http", []) or []:
                method, path = parse_http_entry(http_entry)
                feature_http.add((method, path))
                if path:
                    feature_http_paths.add(path)
        for dep in comp.get("depends", []) or []:
            if dep not in component_ids:
                errors.append(f"component {cid or '(missing id)'} depends on unknown component '{dep}'")
        doc_paths = collect_doc_paths(comp, feature)
        missing_paths = check_paths_exist(doc_paths)
        if missing_paths:
            joined = ", ".join(sorted(set(missing_paths)))
            errors.append(f"component {cid or '(missing id)'} references missing files: {joined}")
        interfaces = comp.get("interfaces", {}) or {}
        for http_entry in interfaces.get("http", []) or []:
            method, path = parse_http_entry(http_entry)
            if not method and not path:
                errors.append(
                    f"component {cid or '(missing id)'} declares an invalid HTTP entry: {http_entry!r}"
                )
                continue
            if feature and path:
                key = (method, path)
                if key not in feature_http and path not in feature_http_paths:
                    errors.append(
                        f"component {cid or '(missing id)'} HTTP '{method or 'ANY'} {path}' "
                        f"is not declared by feature '{fid}'"
                    )
        for topic in sorted(collect_topics(comp)):
            if "*" in topic:
                continue
            if topic not in known_topics:
                errors.append(f"component {cid or '(missing id)'} references unknown topic '{topic}'")

    if errors:
        for err in errors:
            print(f"error: {err}", file=sys.stderr)
        return 1

    print("system component registry ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

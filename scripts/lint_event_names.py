#!/usr/bin/env python3
"""Ensure event kinds (`bus.publish`) and subjects stay in dot.case."""

import argparse
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import textwrap
from typing import Iterable, List, Sequence, Set, Tuple

DEFAULT_ALLOWLIST: Set[str] = set([
    # allow non-dot topics in third-party or test code if needed
])

PUBLISH_PATTERN = r"bus\.publish\(|publish\("
SUBJECT_RE = re.compile(r'"(arw\.events[^"]*)"')


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "paths",
        nargs="*",
        default=["apps", "crates"],
        help="Repo-relative paths to scan (default: apps crates)",
    )
    parser.add_argument(
        "--allow",
        action="append",
        default=[],
        metavar="TOPIC",
        help="Add an extra topic allowlist entry (repeatable)",
    )
    parser.add_argument(
        "--skip-topics",
        action="store_true",
        help="Skip checking bus.publish(...) event kinds",
    )
    parser.add_argument(
        "--skip-subjects",
        action="store_true",
        help="Skip checking arw.events* subject strings",
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="Run builtin smoke tests for the linter and exit",
    )
    return parser.parse_args()


def is_dot_case(text: str) -> bool:
    # Allow dot.case with underscores inside segments to support multiword tokens
    return bool(re.fullmatch(r"[a-z0-9_]+(\\.[a-z0-9_]+)*", text))


def _collect_lines(pattern: str, paths: Sequence[str]) -> List[str]:
    if shutil.which("rg"):
        try:
            out = subprocess.check_output(
                ["rg", "-n", pattern, *paths], text=True, stderr=subprocess.DEVNULL
            )
        except subprocess.CalledProcessError as exc:
            out = exc.stdout or ""
        return out.splitlines()

    lines: List[str] = []
    plain_pattern = re.compile(pattern)
    for base in paths:
        pth = pathlib.Path(base)
        if not pth.exists():
            continue
        for src in pth.rglob("*.rs"):
            try:
                for idx, line in enumerate(
                    src.read_text(encoding="utf-8", errors="ignore").splitlines(), start=1
                ):
                    if plain_pattern.search(line):
                        lines.append(f"{src}:{idx}:{line}")
            except Exception:
                continue
    return lines


def scan_publish_topics(paths: Sequence[str], allowlist: Iterable[str]) -> List[Tuple[str, str]]:
    bad: List[Tuple[str, str]] = []
    lines = _collect_lines(PUBLISH_PATTERN, paths)
    allow = set(allowlist)
    for line in lines:
        match = re.search(r'publish\(\s*([A-Z0-9_]+|"[^"]+")', line)
        if not match:
            continue
        token = match.group(1)
        if token.startswith('"'):
            topic = token.strip('"')
            if topic in allow:
                continue
            if not is_dot_case(topic):
                bad.append((line, topic))
        # Constant names assumed to map to dot.case values in topics.rs
    return bad


def segment_is_lowerish(segment: str) -> bool:
    cleaned = segment.replace("{", "").replace("}", "")
    if cleaned.startswith("<") and cleaned.endswith(">"):
        return True
    return not any(ch.isupper() for ch in cleaned)


def scan_subjects(paths: Sequence[str]) -> List[Tuple[str, str]]:
    bad: List[Tuple[str, str]] = []
    lines = _collect_lines(r"arw\.events", paths)
    for line in lines:
        for match in SUBJECT_RE.finditer(line):
            subject = match.group(1)
            parts = subject.split('.')
            if not all(segment_is_lowerish(seg) for seg in parts):
                bad.append((line, subject))
                break
    return bad


def main() -> None:
    args = parse_args()
    if args.self_test:
        run_self_test()
        return
    allowlist = DEFAULT_ALLOWLIST.union(args.allow)
    bad_topics = [] if args.skip_topics else scan_publish_topics(args.paths, allowlist)
    bad_subjects = [] if args.skip_subjects else scan_subjects(args.paths)

    if bad_topics or bad_subjects:
        if bad_topics:
            print("Found non-dot.case event kinds:")
            for line, topic in sorted(bad_topics):
                print(f" - {topic} :: {line}")
        if bad_subjects:
            print("Found legacy/uppercase event subjects:")
            for line, subject in sorted(bad_subjects):
                print(f" - {subject} :: {line}")
        sys.exit(2)

    print("Event names OK (dot.case)")


def run_self_test() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        tmp_path = pathlib.Path(tmp)
        good_rs = tmp_path / "good.rs"
        good_rs.write_text(
            textwrap.dedent(
                """
                fn good() {
                    let payload = serde_json::json!({"ok": true});
                    bus.publish(TOPIC_RUNTIME_HEALTH, &payload);
                }
                """
            ),
            encoding="utf-8",
        )
        bad_rs = tmp_path / "bad.rs"
        bad_rs.write_text(
            textwrap.dedent(
                """
                fn bad() {
                    let payload = serde_json::json!({"ok": false});
                    bus.publish("BadTopic", &payload);
                }
                """
            ),
            encoding="utf-8",
        )
        allowlist: Set[str] = set()
        bad_topics = scan_publish_topics([str(tmp_path)], allowlist)
        assert bad_topics, "expected bad topic to be reported"
        bad_subjects = scan_subjects([str(tmp_path)])
        assert not bad_subjects, "no subjects declared in fixture"

        original_which = shutil.which
        shutil.which = lambda *_args, **_kwargs: None  # force fallback path
        try:
            fallback_bad_topics = scan_publish_topics([str(tmp_path)], allowlist)
        finally:
            shutil.which = original_which
        assert fallback_bad_topics, "fallback path missed bad topic"

        bad_rs.unlink()
        shutil.which = lambda *_args, **_kwargs: None
        try:
            clean_topics = scan_publish_topics([str(tmp_path)], allowlist)
        finally:
            shutil.which = original_which
        assert not clean_topics, "expected clean run after removing bad topic"
    print("Self-test OK")


if __name__ == "__main__":
    main()

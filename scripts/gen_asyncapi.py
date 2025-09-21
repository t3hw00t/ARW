#!/usr/bin/env python3
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TOPICS_FILE = ROOT / "crates" / "arw-topics" / "src" / "lib.rs"
CARGO_SERVER = ROOT / "apps" / "arw-server" / "Cargo.toml"
OUTPUT = ROOT / "spec" / "asyncapi.yaml"

TOPIC_PATTERN = re.compile(r'^pub const [A-Z0-9_]+: &str = "([^"]+)";', re.MULTILINE)
VERSION_PATTERN = re.compile(r'^version\s*=\s*"([^"]+)"', re.MULTILINE)

def load_topics() -> list[str]:
    text = TOPICS_FILE.read_text(encoding="utf-8")
    topics = sorted(set(TOPIC_PATTERN.findall(text)))
    return topics


def load_version() -> str:
    text = CARGO_SERVER.read_text(encoding="utf-8")
    match = VERSION_PATTERN.search(text)
    return match.group(1) if match else "0.0.0"


def operation_id(topic: str) -> str:
    sanitized = re.sub(r"[^a-zA-Z0-9]+", "_", topic)
    sanitized = re.sub(r"_+", "_", sanitized).strip("_")
    return f"{sanitized}_event" if sanitized else "event"


def build_yaml(topics: list[str], version: str) -> str:
    lines = []
    lines.append("asyncapi: 2.6.0")
    lines.append("info:")
    lines.append("  title: \"arw-server events\"")
    lines.append(f"  version: \"{version}\"")
    lines.append("  description: \"Normalized dot.case event channels for the unified server.\"")
    lines.append("  license:")
    lines.append("    name: \"MIT OR Apache-2.0\"")
    lines.append("defaultContentType: application/json")
    lines.append("channels:")
    for topic in topics:
        oid = operation_id(topic)
        lines.append(f"  '{topic}':")
        lines.append("    subscribe:")
        lines.append(f"      operationId: {oid}")
        lines.append(f"      summary: \"{topic} event\"")
        lines.append("      message:")
        lines.append(f"        name: '{topic}'")
        lines.append("        payload:")
        lines.append("          type: object")
        lines.append("          additionalProperties: true")
    return "\n".join(lines) + "\n"


def main() -> None:
    topics = load_topics()
    version = load_version()
    yaml = build_yaml(topics, version)
    OUTPUT.write_text(yaml, encoding="utf-8")


if __name__ == "__main__":
    main()

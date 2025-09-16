#!/usr/bin/env python3
import re, sys, subprocess

ALLOWLIST = set([
    # allow non-dot topics in third-party or test code if needed
])

def is_dot_case(s: str) -> bool:
    # Allow dot.case with underscores inside segments to support multiword tokens
    return bool(re.fullmatch(r"[a-z0-9_]+(\.[a-z0-9_]+)*", s))

def scan_paths(paths):
    bad = []
    rg_pat = r"bus\.publish\(|publish\("
    try:
        out = subprocess.check_output(["rg","-n","-S",rg_pat,*paths], text=True, stderr=subprocess.DEVNULL)
    except subprocess.CalledProcessError as e:
        out = e.stdout or ""
    for line in out.splitlines():
        # Example: file.rs:123:    bus.publish(TOPIC_SOMETHING, &payload);
        # or: bus.publish("models.download.progress", &payload)
        m = re.search(r"publish\(\s*([A-Z0-9_]+|\"[^\"]+\")", line)
        if not m:
            continue
        token = m.group(1)
        if token.startswith('"'):
            topic = token.strip('"')
            if topic in ALLOWLIST: continue
            if not is_dot_case(topic):
                bad.append((line, topic))
        else:
            # Constant name; skip (assume defined in topics.rs as dot.case values)
            pass
    return bad

def main():
    paths = ["apps"]
    bad = scan_paths(paths)
    if bad:
        print("Found non-dot.case event kinds:")
        for l, t in bad:
            print(f" - {t} :: {l}")
        sys.exit(2)
    print("Event names OK (dot.case)")

if __name__ == '__main__':
    main()

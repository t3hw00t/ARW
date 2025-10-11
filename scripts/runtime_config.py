
#!/usr/bin/env python3
import argparse
import json
import re
import sys
import urllib.request
import urllib.error
import urllib.request
import urllib.error
import subprocess
import shutil
from pathlib import Path

DEFAULT_CONFIG = Path('configs/runtime/model_sources.json')
URL_RE = re.compile(r'^https://')
CHECKSUM_RE = re.compile(r'^sha256:[0-9a-fA-F]{64}$')


def load_config(config_path: Path):
    try:
        data = json.loads(config_path.read_text(encoding='utf-8'))
    except FileNotFoundError:
        return {'default': [], 'mirrors': []}
    except json.JSONDecodeError as exc:
        print(f"Invalid JSON in {config_path}: {exc}", file=sys.stderr)
        sys.exit(1)

    def validate_default(entries):
        cleaned = []
        for entry in entries:
            repo = entry.get('repo')
            file = entry.get('file')
            checksum = entry.get('checksum', '')
            if not repo or not file:
                print(f"Skipping source missing repo/file: {entry}")
                continue
            if checksum and not CHECKSUM_RE.match(checksum):
                print(f"Skipping source with invalid checksum: {entry}")
                continue
            cleaned.append({'repo': repo, 'file': file, 'checksum': checksum})
        return cleaned

    def validate_mirrors(entries):
        cleaned = []
        for entry in entries:
            url = entry.get('url', '')
            checksum = entry.get('checksum', '')
            notes = entry.get('notes', '')
            name = entry.get('name', entry.get('url', ''))
            if not URL_RE.match(url):
                print(f"Skipping mirror with invalid URL (must be https://*): {entry}")
                continue
            if checksum and not CHECKSUM_RE.match(checksum):
                print(f"Skipping mirror with invalid checksum: {entry}")
                continue
            cleaned.append({'name': name or url, 'url': url, 'checksum': checksum, 'notes': notes})
        return cleaned

    return {
        'default': validate_default(data.get('default', [])),
        'mirrors': validate_mirrors(data.get('mirrors', []))
    }


def render_table(headers, rows):
    if not rows:
        return '(none configured)'
    widths = [len(h) for h in headers]
    for row in rows:
        widths = [max(widths[i], len(str(col))) for i, col in enumerate(row)]
    sep = '  '

    def fmt(row):
        return sep.join(str(col).ljust(widths[i]) for i, col in enumerate(row))

    lines = [fmt(headers), fmt(tuple('-' * w for w in widths))]
    lines.extend(fmt(row) for row in rows)
    return '\n'.join(lines)


def curl_probe(url: str, timeout: float, method: str = 'HEAD') -> str | None:
    if shutil.which('curl') is None:
        return None
    cmd = ['curl', '-sS', '-o', '/dev/null', '-w', '%{http_code}', '--max-time', str(timeout)]
    if method.upper() == 'HEAD':
        cmd.append('-I')
    cmd.append(url)
    try:
        result = subprocess.run(cmd, capture_output=True, text=True, check=False)
    except Exception:
        return None
    if result.returncode != 0:
        return None
    code = result.stdout.strip()
    if code.isdigit():
        return f"HTTP {code}"
    return None


def probe_url(url: str, timeout: float) -> str:
    req = urllib.request.Request(url, method='HEAD')
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return f"HTTP {resp.status}"
    except urllib.error.HTTPError as exc:
        if exc.code == 405:
            try:
                with urllib.request.urlopen(url, timeout=timeout) as resp:
                    return f"HTTP {resp.status}"
            except Exception:
                fallback = curl_probe(url, timeout, method='GET')
                if fallback:
                    return fallback
                return 'error: GET_FAILED'
        fallback = curl_probe(url, timeout, method='HEAD')
        if fallback:
            return fallback
        return f"HTTP {exc.code}"
    except Exception:
        fallback = curl_probe(url, timeout, method='HEAD')
        if fallback:
            return fallback
        return 'error: HEAD_FAILED'


def main():
    parser = argparse.ArgumentParser(description='Inspect runtime model source mirrors and defaults.')
    parser.add_argument('--config', default=str(DEFAULT_CONFIG), help='Path to model_sources.json (default: configs/runtime/model_sources.json)')
    parser.add_argument('--check', action='store_true', help='Send a HEAD request to each mirror to verify availability (best-effort).')
    parser.add_argument('--timeout', type=float, default=5.0, help='Timeout in seconds for mirror availability checks (default: 5s).')
    args = parser.parse_args()

    config_path = Path(args.config)
    cfg = load_config(config_path)
    defaults = [(entry['repo'], entry['file'], entry['checksum'] or '-', probe_url(entry['url'], args.timeout) if args.check and entry.get('url') else '-') for entry in cfg['default']]

    mirror_rows = []
    for entry in cfg['mirrors']:
        status = '-'
        if args.check:
            status = probe_url(entry['url'], args.timeout)
        mirror_rows.append((entry['name'], entry['url'], entry['checksum'] or '-', status, entry['notes'] or '-'))

    print(f'[DEFAULT SOURCES] (config: {config_path})')
    if args.check:
        print(render_table(('Repo', 'File', 'Checksum', 'Status'), defaults))
    else:
        defaults_no_status = [(repo, file, checksum) for repo, file, checksum, _ in defaults]
        print(render_table(('Repo', 'File', 'Checksum'), defaults_no_status))
    print('\n[MIRRORS]')
    if args.check:
        mirrors_table = [(name, url, checksum, status, notes) for name, url, checksum, status, notes in mirror_rows]
        print(render_table(('Name', 'URL', 'Checksum', 'Status', 'Notes'), mirrors_table))
    else:
        mirrors_table = [(name, url, checksum, notes) for name, url, checksum, _, notes in mirror_rows]
        print(render_table(('Name', 'URL', 'Checksum', 'Notes'), mirrors_table))


if __name__ == '__main__':
    main()

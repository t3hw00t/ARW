#!/usr/bin/env python3
import argparse
import json
import os
import sys
from pathlib import Path
from typing import Dict, List, Tuple
from urllib import request, error

DEFAULT_CONFIG = Path(__file__).resolve().parent.parent / 'configs' / 'runtime' / 'model_sources.json'
DEFAULT_SOURCES = [
    ('ggml-org/tinyllama-1.1b-chat', 'tinyllama-1.1b-chat-q4_k_m.gguf'),
    ('TheBloke/TinyLlama-1.1B-Chat-GGUF', 'TinyLlama-1.1B-Chat-q4_k_m.gguf'),
]


def load_config(path: Path) -> Dict[str, List[Dict[str, str]]]:
    try:
        data = json.loads(path.read_text(encoding='utf-8'))
    except FileNotFoundError:
        return {'default': [], 'mirrors': []}
    except json.JSONDecodeError as exc:
        print(f"Invalid JSON in {path}: {exc}", file=sys.stderr)
        sys.exit(1)
    return {
        'default': [entry for entry in data.get('default', []) if entry.get('repo') and entry.get('file')],
        'mirrors': data.get('mirrors', []),
    }


def resolve_sources(args, cfg) -> List[Tuple[str, str]]:
    if args.sources:
        return [tuple(part.split('::', 1)) if '::' in part else (part, '') for part in args.sources.split(',')]
    default = [(entry['repo'], entry['file']) for entry in cfg['default']]
    if default:
        return default
    return DEFAULT_SOURCES


def build_checksum_map(cfg) -> Dict[Tuple[str, str], str]:
    mapping: Dict[Tuple[str, str], str] = {}
    for entry in cfg['default']:
        checksum = entry.get('checksum')
        if checksum:
            mapping[(entry['repo'], entry['file'])] = checksum
    return mapping


def print_mirrors(cfg) -> None:
    mirrors = cfg.get('mirrors', [])
    if not mirrors:
        return
    print('[runtime-weights] Mirrors (verify before use):')
    for entry in mirrors:
        url = entry.get('url')
        if not url:
            continue
        name = entry.get('name') or url
        checksum = entry.get('checksum')
        notes = entry.get('notes') or ''
        suffix = ''
        if notes:
            suffix += f' — {notes}'
        if checksum:
            suffix += f' (checksum {checksum})'
        print(f'  • {name}: {url}{suffix}')


def ensure_token() -> str:
    token = os.getenv('HF_TOKEN') or os.getenv('HUGGINGFACEHUB_API_TOKEN')
    if not token:
        print('[runtime-weights] A Hugging Face token is required (set HF_TOKEN).', file=sys.stderr)
        sys.exit(1)
    return token


def download(repo: str, file_name: str, token: str, dest: Path) -> None:
    url = f"https://huggingface.co/{repo}/resolve/main/{file_name}"
    req = request.Request(url, headers={'Authorization': f'Bearer {token}'})
    try:
        with request.urlopen(req) as resp, dest.open('wb') as out:
            out.write(resp.read())
    except error.HTTPError as exc:
        raise RuntimeError(f'HTTP error {exc.code}') from exc
    except Exception as exc:
        raise RuntimeError(str(exc)) from exc


def checksum_file(path: Path) -> str:
    import hashlib
    h = hashlib.sha256()
    with path.open('rb') as fh:
        for chunk in iter(lambda: fh.read(8192), b''):
            h.update(chunk)
    return h.hexdigest()


def verify_checksum(path: Path, expected: str) -> bool:
    expected = expected.removeprefix('sha256:')
    actual = checksum_file(path)
    return actual == expected


def main() -> None:
    parser = argparse.ArgumentParser(description='Download runtime GGUF weights with checksum validation.')
    parser.add_argument('--sources', help='Override sources (comma-separated repo::file entries).')
    parser.add_argument('--dest', default=str(Path(__file__).resolve().parent.parent / 'cache' / 'models'))
    parser.add_argument('--config', default=str(os.getenv('RUNTIME_MODEL_SOURCES_CONFIG') or DEFAULT_CONFIG))
    parser.add_argument('--all', action='store_true', help='Download every source instead of stopping after first success.')
    parser.add_argument('--overwrite', action='store_true', help='Redownload even if target exists.')
    parser.add_argument('--quiet', action='store_true', help='Reduce logging noise.')
    args = parser.parse_args()

    config_path = Path(args.config)
    cfg = load_config(config_path)
    sources = resolve_sources(args, cfg)
    checksums = build_checksum_map(cfg)

    if not args.quiet:
        print_mirrors(cfg)

    dest_dir = Path(args.dest)
    dest_dir.mkdir(parents=True, exist_ok=True)

    token = ensure_token()

    downloaded = []
    failures = []

    for repo, file_name in sources:
        if not file_name:
            failures.append(f'{repo}::<missing>')
            continue
        target = dest_dir / file_name
        if target.exists() and not args.overwrite:
            if not args.quiet:
                print(f'[runtime-weights] Using cached weight: {target}')
            downloaded.append(str(target))
            if not args.all:
                break
            continue

        if not args.quiet:
            print(f'[runtime-weights] Downloading {file_name} from {repo}...')
        tmp = target.with_suffix('.download')
        if tmp.exists():
            tmp.unlink()
        try:
            download(repo, file_name, token, tmp)
        except RuntimeError as exc:
            failures.append(f'{repo}::{file_name} ({exc})')
            if tmp.exists():
                tmp.unlink()
            continue

        expected_checksum = checksums.get((repo, file_name))
        if expected_checksum:
            if not verify_checksum(tmp, expected_checksum):
                failures.append(f'{repo}::{file_name} (checksum mismatch)')
                tmp.unlink(missing_ok=True)
                continue

        tmp.replace(target)
        downloaded.append(str(target))
        if not args.quiet:
            print(f'[runtime-weights] Saved {target}')
        if not args.all:
            break

    if not args.quiet:
        if downloaded:
            print('[runtime-weights] Completed downloads:')
            for item in downloaded:
                print(f'  - {item}')
        if failures:
            print('[runtime-weights] Failures:')
            for item in failures:
                print(f'  - {item}')

    if failures and not downloaded:
        sys.exit(1)


if __name__ == '__main__':
    main()

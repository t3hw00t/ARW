#!/usr/bin/env bash
set -euo pipefail

target_triple=""
nobuild=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --no-build) nobuild=1; shift;;
    --target) target_triple="$2"; shift 2;;
    -h|--help)
      echo "Usage: $0 [--no-build] [--target <triple>]"; exit 0;;
    *) echo "Unknown option: $1"; exit 1;;
  esac
done

command -v cargo >/dev/null || { echo 'cargo not found'; exit 1; }

if [[ $nobuild -eq 0 ]]; then
  echo '[package] Building (release)'
  if [[ -n "$target_triple" ]]; then
    cargo build --release --locked --target "$target_triple" -p arw-svc -p arw-cli || true
    # Try launcher too, but don't fail the packaging if it doesn't build
    cargo build --release --locked --target "$target_triple" -p arw-launcher || true
  else
    cargo build --workspace --release --locked || true
  fi
fi

root_dir=$(cd "$(dirname "$0")/.." && pwd)
version=$(grep -m1 '^version\s*=\s*"' "$root_dir/Cargo.toml" | sed -E 's/.*"([^"]+)".*/\1/')
version=${version:-0.0.0}

# Derive OS/arch from target triple when provided; else from host
if [[ -n "$target_triple" ]]; then
  case "$target_triple" in
    *-unknown-linux-gnu) os=linux;;
    *-apple-darwin)      os=macos;;
    *-pc-windows-msvc)   os=windows;;
    *) os=$(uname -s | tr '[:upper:]' '[:lower:]');;
  esac
  case "$target_triple" in
    aarch64-*) arch=arm64;;
    x86_64-*)  arch=x64;;
    *) arch=$(uname -m);; 
  esac
  bin_dir="$root_dir/target/$target_triple/release"
else
  uname_s=$(uname -s | tr '[:upper:]' '[:lower:]')
  uname_m=$(uname -m)
  case "$uname_s" in
    darwin) os=macos;;
    linux)  os=linux;;
    msys*|mingw*|cygwin*) os=windows;;
    *) os=$uname_s;;
  esac
  case "$uname_m" in
    x86_64|amd64) arch=x64;;
    arm64|aarch64) arch=arm64;;
    *) arch=$uname_m;;
  esac
  bin_dir="$root_dir/target/release"
fi

name="arw-$version-$os-$arch"
dist="$root_dir/dist"
out="$dist/$name"
rm -rf "$out" && mkdir -p "$out/bin" "$out/configs"

exe=''
[[ "$os" == windows ]] && exe='.exe'

cp "$bin_dir/arw-svc$exe" "$out/bin/arw-svc$exe"
cp "$bin_dir/arw-cli$exe" "$out/bin/arw-cli$exe" 2>/dev/null || true
if [[ -f "$bin_dir/arw-tray$exe" ]]; then
  cp "$bin_dir/arw-tray$exe" "$out/bin/arw-tray$exe"
fi
if [[ -f "$bin_dir/arw-launcher$exe" ]]; then
  cp "$bin_dir/arw-launcher$exe" "$out/bin/arw-launcher$exe"
fi
cp "$root_dir/configs/default.toml" "$out/configs/default.toml"
cp -r "$root_dir/docs" "$out/docs"
if [[ -d "$root_dir/site" ]]; then
  cp -r "$root_dir/site" "$out/docs-site"
fi
if [[ "$os" == windows && -f "$root_dir/sandbox/ARW.wsb" ]]; then
  mkdir -p "$out/sandbox" && cp "$root_dir/sandbox/ARW.wsb" "$out/sandbox/ARW.wsb"
fi

cat > "$out/README.txt" << EOF
ARW portable bundle ($name)

Contents
- bin/        arw-svc, arw-cli, (optional) arw-launcher
- configs/    default.toml (portable state paths)
- docs/       project docs
- sandbox/    Windows Sandbox config (Windows only)

Usage
- Run service: bin/arw-svc$exe
- Debug UI:    http://127.0.0.1:8090/debug
- CLI sanity:  bin/arw-cli$exe
 - Launcher:    bin/arw-launcher$exe (includes tray)

Notes
- To force portable mode: export ARW_PORTABLE=1
EOF

mkdir -p "$dist"
zip -qr "$dist/$name.zip" -j "$out/bin"/* "$out/README.txt"
if [[ -d "$out/sandbox" ]]; then
  zip -qr "$dist/$name.zip" "$out/configs" "$out/docs" "$out/sandbox"
else
  zip -qr "$dist/$name.zip" "$out/configs" "$out/docs"
fi
if [[ -d "$out/docs-site" ]]; then zip -qr "$dist/$name.zip" "$out/docs-site"; fi

echo "[package] Wrote $dist/$name.zip"

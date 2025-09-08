#!/usr/bin/env bash
set -euo pipefail

nobuild=0
if [[ ${1:-} == '--no-build' ]]; then nobuild=1; fi

command -v cargo >/dev/null || { echo 'cargo not found'; exit 1; }

if [[ $nobuild -eq 0 ]]; then
  echo '[package] Building (release)'
  cargo build --workspace --release --locked
fi

root_dir=$(cd "$(dirname "$0")/.." && pwd)
version=$(grep -m1 '^version\s*=\s*"' "$root_dir/Cargo.toml" | sed -E 's/.*"([^"]+)".*/\1/')
version=${version:-0.0.0}

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

name="arw-$version-$os-$arch"
dist="$root_dir/dist"
out="$dist/$name"
rm -rf "$out" && mkdir -p "$out/bin" "$out/configs"

exe=''
[[ "$os" == windows ]] && exe='.exe'

cp "$root_dir/target/release/arw-svc$exe" "$out/bin/arw-svc$exe"
cp "$root_dir/target/release/arw-cli$exe" "$out/bin/arw-cli$exe"
if [[ -f "$root_dir/target/release/arw-tray$exe" ]]; then
  cp "$root_dir/target/release/arw-tray$exe" "$out/bin/arw-tray$exe"
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
- bin/        arw-svc, arw-cli
- configs/    default.toml (portable state paths)
- docs/       project docs
- sandbox/    Windows Sandbox config (Windows only)

Usage
- Run service: bin/arw-svc$exe
- Debug UI:    http://127.0.0.1:8090/debug
- CLI sanity:  bin/arw-cli$exe

Notes
- To force portable mode: export ARW_PORTABLE=1
EOF

mkdir -p "$dist"
zip -qr "$dist/$name.zip" -j "$out/bin"/* "$out/README.txt"
zip -qr "$dist/$name.zip" "$out/configs" "$out/docs" $( [[ -d "$out/sandbox" ]] && echo "$out/sandbox" )
if [[ -d "$out/docs-site" ]]; then zip -qr "$dist/$name.zip" "$out/docs-site"; fi

echo "[package] Wrote $dist/$name.zip"

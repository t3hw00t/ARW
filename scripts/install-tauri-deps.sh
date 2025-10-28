#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: bash scripts/install-tauri-deps.sh [--yes] [--print-only]

Installs the GTK/WebKit dependencies that the ARW launcher needs on Linux (incl. WSL).
  --yes         Run non-interactively (auto-confirm package installation).
  --print-only  Show the commands that would run without executing them.
EOF
}

auto_confirm=0
print_only=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --yes|-y) auto_confirm=1; shift ;;
    --print-only|--dry-run) print_only=1; shift ;;
    --help|-h) usage; exit 0 ;;
    *) echo "Unknown option: $1" >&2; usage; exit 2 ;;
  esac
done

if [[ "${OSTYPE:-}" != linux* ]]; then
  echo "This helper targets Linux. On macOS/Windows, no extra system libs are required."
  exit 0
fi

echo "Detecting Linux distribution for Tauri 2 WebKit dependencies..."

ID="unknown"
if [[ -f /etc/os-release ]]; then
  . /etc/os-release
fi

run_cmd() {
  if [[ $print_only -eq 1 ]]; then
    echo "[dry-run] $*"
    return 0
  fi
  echo "[run] $*"
  eval "$@"
}

ensure_sudo() {
  if [[ $print_only -eq 1 ]]; then
    return 0
  fi
  if [[ $EUID -ne 0 ]]; then
    if ! command -v sudo >/dev/null 2>&1; then
      echo "sudo is required to install packages. Install sudo or run this script as root." >&2
      exit 1
    fi
  fi
}

maybe_prompt() {
  local message="$1"
  if [[ $auto_confirm -eq 1 || $print_only -eq 1 ]]; then
    return 0
  fi
  read -r -p "$message [Y/n]: " reply
  case "${reply:-}" in
    ""|Y|y|Yes|yes) return 0 ;;
    *) echo "Aborting."; exit 0 ;;
  esac
}

case "${ID:-}" in
  ubuntu|debian)
    if [[ "${ID}" == "ubuntu" && "${VERSION_ID:-}" =~ ^22\.04 ]]; then
      echo "Ubuntu 22.04 (Jammy) is unsupported: WebKitGTK 4.1 + libsoup3 packages are unavailable."
      echo "Use a newer Ubuntu (24.04+), the Nix dev shell, or run the launcher in a browser."
      exit 0
    fi
    if [[ "${ID}" == "debian" && "${VERSION_CODENAME:-}" == "bookworm" ]]; then
      echo "Debian 12 (bookworm) lacks WebKitGTK 4.1 packages."
      echo "Switch to testing/unstable, use the Nix dev shell, or run the launcher in a browser."
      exit 0
    fi
    packages=(libgtk-3-dev libwebkit2gtk-4.1-dev libjavascriptcoregtk-4.1-dev libsoup-3.0-dev)
    echo "Debian/Ubuntu detected. Packages to install: ${packages[*]}"
    if ! command -v apt-get >/dev/null 2>&1; then
      echo "apt-get not found; cannot install automatically." >&2
      exit 1
    fi
    maybe_prompt "Proceed with 'sudo apt-get update' and 'sudo apt-get install -y ${packages[*]}'?"
    ensure_sudo
    run_cmd "sudo apt-get update"
    run_cmd "sudo apt-get install -y ${packages[*]}"
    ;;
  fedora)
    packages=(gtk3-devel webkit2gtk4.1-devel libsoup3-devel)
    echo "Fedora detected. Packages to install: ${packages[*]}"
    if ! command -v dnf >/dev/null 2>&1; then
      echo "dnf not found; cannot install automatically." >&2
      exit 1
    fi
    maybe_prompt "Proceed with 'sudo dnf install -y ${packages[*]}'?"
    ensure_sudo
    run_cmd "sudo dnf install -y ${packages[*]}"
    ;;
  arch)
    packages=(gtk3 webkit2gtk-4.1 libsoup3)
    echo "Arch detected. Packages to install: ${packages[*]}"
    if ! command -v pacman >/dev/null 2>&1; then
      echo "pacman not found; cannot install automatically." >&2
      exit 1
    fi
    maybe_prompt "Proceed with 'sudo pacman -S --needed ${packages[*]}'?"
    ensure_sudo
    run_cmd "sudo pacman -S --needed ${packages[*]}"
    ;;
  *)
    echo "Unknown distro (${ID}). Please install WebKitGTK 4.1 + libsoup3 development packages manually."
    ;;
esac

cat <<'NOTE'
Note: If you use Nix, 'nix develop' in this repo already provides these libs.
If WebKitGTK 4.1 packages are unavailable on your distro, you can still run the Control Room in a browser:
  1) Start the service only: bash scripts/start.sh --service-only --wait-health
  2) Open http://127.0.0.1:8091/admin/ui/control/ (or /admin/debug) in a modern browser.
Saved connections let you aim the desktop launcher from another machine that has the required runtime.
NOTE

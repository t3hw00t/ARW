#!/usr/bin/env bash
set -euo pipefail

echo "Detecting Linux distribution to suggest Tauri 2 WebKit deps..."

if [[ "${OSTYPE:-}" != linux* ]]; then
  echo "This helper targets Linux. On macOS/Windows, no extra system libs are required."
  exit 0
fi

ID="unknown"
if [[ -f /etc/os-release ]]; then
  . /etc/os-release
fi

case "${ID:-}" in
  ubuntu|debian)
    if [[ "${ID}" == "ubuntu" && "${VERSION_ID:-}" =~ ^22\.04 ]]; then
      echo "Ubuntu 22.04 (Jammy) is unsupported: WebKitGTK 4.1 + libsoup3 packages are unavailable."
      echo "You can still run the ARW service headless and use the browser UI, or enter 'nix develop' for a bundled toolchain."
      exit 0
    fi
    if [[ "${ID}" == "debian" && "${VERSION_CODENAME:-}" == "bookworm" ]]; then
      echo "Debian 12 (bookworm) lacks WebKitGTK 4.1 packages."
      echo "Switch to testing/unstable, use the Nix dev shell, or run the launcher surfaces from a browser."
      exit 0
    fi
    echo "Debian/Ubuntu detected. Suggested packages (Ubuntu 24.04+):"
    echo "  sudo apt update && sudo apt install -y libgtk-3-dev libwebkit2gtk-4.1-dev libjavascriptcoregtk-4.1-dev libsoup-3.0-dev"
    ;;
  fedora)
    echo "Fedora detected. Suggested packages:"
    echo "  sudo dnf install -y gtk3-devel webkit2gtk4.1-devel libsoup3-devel"
    ;;
  arch)
    echo "Arch detected. Suggested packages:"
    echo "  sudo pacman -S --needed gtk3 webkit2gtk-4.1 libsoup3"
    ;;
  *)
    echo "Unknown distro (${ID}). Please install WebKitGTK 4.1 + libsoup3 development packages."
    ;;
esac

cat <<'NOTE'
Note: If you use Nix, 'nix develop' in this repo already provides these libs.
If WebKitGTK 4.1 packages are unavailable on your distro, you can still run the Control Room in a browser:
  1) Start the service only: bash scripts/start.sh --service-only --wait-health
  2) Open http://127.0.0.1:8091/admin/ui/control/ (or /admin/debug) in a modern browser.
Saved connections let you aim the desktop launcher from another machine that has the required runtime.
NOTE

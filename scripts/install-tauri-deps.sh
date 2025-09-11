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
    echo "Debian/Ubuntu detected. Suggested packages:"
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

echo "Note: If you use Nix, 'nix develop' in this repo already provides these libs."


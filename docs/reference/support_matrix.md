# Support Matrix
Updated: 2025-10-09
Type: Reference

Microsummary: Supported OS/arch, minimum Rust, optional Nix, and container tags. Stable and updated as releases ship.

- OS/Arch: Windows/macOS/Linux on x64 and ARM64.
- Rust: Rust 1.90.x (pinned via `rust-toolchain.toml`).
- Nix: optional dev shell; see `flake.nix`.
- Containers: `ghcr.io/t3hw00t/arw-server:{latest, <semver>}` (multi-arch: linux/amd64, linux/arm64).
- Binaries: portable per‑user installs; Windows (x64/ARM64), macOS (x64/ARM64), Linux (x64/ARM64).

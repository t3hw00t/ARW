# CLI Reference
Updated: 2025-09-15
Type: Reference

Microsummary: Commands, subcommands, and flags for `arw-cli` with pointers to tutorials. Beta.

- Install: built with the workspace; see `just dev` and `nix develop` in `README.md`.
- Common flows: see Tutorials (Quickstart) and How‑to pages.

Commands (summary)
- `arw-cli` — prints version, hello, and effective paths
- `arw-cli paths [--pretty]` — effective runtime/cache/logs paths
- `arw-cli tools [--pretty]` — list registered tools
- `arw-cli gate keys` — list known gating keys
- `arw-cli capsule template [--pretty|--compact]` — print a minimal capsule template
- `arw-cli capsule gen-ed25519 [--issuer <name>] [--out-pub <file>] [--out-priv <file>]` — generate keys
- `arw-cli capsule sign-ed25519 <sk_b64> <capsule.json> [--out <file>]` — sign capsule
- `arw-cli capsule verify-ed25519 <pk_b64> <capsule.json> <sig_b64>` — verify signature

See the CLI Guide (guide/cli.md) for examples. Use `--help` on any command for details.

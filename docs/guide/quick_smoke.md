---
title: Quick Smoke
---

# Quick Smoke

Updated: 2025-10-30
Type: Tutorial

Microsummary: Run the smallest reproducible guardrail (`verify --fast`) with minimal prerequisites so new contributors can confirm the toolchain before installing optional stacks.

## When to Use This Path
- You want a sanity pass before committing to the full docs/UI toolchain.
- You are on a fresh machine and only have time for Rust + basics.
- You are validating a pull request that touches a narrow slice of Rust code.

Once the quick smoke succeeds, graduate to the full [Quickstart](quickstart.md) or `scripts/dev.sh verify` to cover docs, smokes, and UI checks.

## Requirements
- Rust 1.90+ with `cargo`, `rustfmt`, and `clippy` (install via [rustup](https://rustup.rs)).
- `cargo-nextest` (optional but recommended):
  ```bash
  cargo install cargo-nextest
  ```
- `just` or [mise](https://mise.jdx.dev) (optional convenience wrappers). The commands below use the raw scripts so you can copy/paste without either tool.

No Python, Node.js, or MkDocs dependencies are needed for this smoke.
If you later add prompt compression (llmlingua), the repo uses a local `.venv` by default; no pip user/site installs are needed.

## One-Time Setup
```bash
# Headless bootstrap tuned for fast iteration
bash scripts/dev.sh setup-agent --skip-build

# Refresh the environment mode so target/venv directories match the host
bash scripts/env/switch.sh linux       # or windows-host / windows-wsl / mac
```

> **Windows tip:** use PowerShell variants (`scripts\dev.ps1 setup-agent -SkipBuild`, `scripts\env\switch.ps1 windows-host`) if Bash is unavailable.

## Run the Quick Smoke
=== "Bash"
```bash
bash scripts/dev.sh verify --fast
```

=== "just"
```bash
just quick-smoke
```

=== "mise"
```bash
mise run quick:smoke
```

What runs:
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo nextest run --workspace --test-threads=1` (falls back to `cargo test` when nextest is missing)

Expected outcome:
- Success prints `verify complete` with zero skips.
- Missing toolchains will stop the run early with a clear error message (install the missing tool and rerun).

## If You Hit Failures
!!! tip "Common fixes"
    - `bash: cargo: command not found` → install Rust via `rustup` (`curl https://sh.rustup.rs -sSf | sh`, or download the Windows installer) and restart the shell.
    - `error: toolchain 'stable-x86_64-unknown-linux-gnu' does not contain component 'clippy'` → run `rustup component add clippy`.
    - `error: toolchain ... does not contain component 'rustfmt'` → run `rustup component add rustfmt`.
    - `error: no such subcommand: nextest` → install Nextest (`cargo install cargo-nextest`) or let the smoke fall back to `cargo test`.
- Format errors: run `cargo fmt --all` and rerun the smoke.
- Clippy warnings: apply the suggested fixes or annotate intentionally noisy cases.
- Test failures: re-run the failing crate with `cargo nextest run -p <crate>` for faster feedback.
- Environment mismatch: rerun `scripts/env/switch.sh` (or the PowerShell twin) to realign the target directory, then retry `verify --fast`.

Once the fast guardrail is green, you are ready to install docs/UI dependencies and use the full `bash scripts/dev.sh verify` or task wrappers (`just verify`, `mise run verify`) for comprehensive coverage.

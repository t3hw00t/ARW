---
title: Build Efficiency Playbook
---

# Build Efficiency Playbook

Updated: 2025-09-17
Type: Reference

> Tips for keeping local and CI builds fast without sacrificing correctness.

## Caching & Incremental Builds

- Install and enable [`sccache`](https://github.com/mozilla/sccache); export `RUSTC_WRAPPER=$(command -v sccache)` in your shell profile to reuse compiled artifacts across workspaces and CI runners.
- Keep `CARGO_INCREMENTAL=1` for iterative work. Disable it (`CARGO_INCREMENTAL=0 cargo build --release`) only for final size-sensitive artifacts.
- Use `cargo check --workspace --all-targets` for tight edit/compile loops; it skips code generation but validates borrow checking and type inference.

## Faster Linking

- Install a faster linker (`mold` or `lld`) and point Cargo at it via `.cargo/config.toml`:
  ```toml
  [target.x86_64-unknown-linux-gnu]
  linker = "clang"
  rustflags = ["-C", "link-arg=-fuse-ld=mold"]
  ```
- For local debug builds, prefer `cargo build -Zshare-generics=y` (nightly) to reuse monomorphized code across crates when available.

## Command Shortcuts

- `just dev-build` – workspace debug build without `--locked` (fast feedback, matches Rust analyzer behaviour).
- `just test-fast` – uses `cargo nextest` when available to parallelize test execution and minimize recompilation.
- `just docgen` – regenerates docs without triggering `mkdocs`; pair with `mkdocs serve` for live previews.

## Targeted Profiles

Define custom profiles in `Cargo.toml` to trim unneeded optimizations during development:

```toml
[profile.dev.package."*"]
opt-level = 0
panic = "abort"
debug = true
codegen-units = 256
```

Override per-crate settings when a crate benefits from extra optimization (e.g., WebAssembly plugins).

## Dependency Hygiene

- Regularly run `cargo update -p <crate>` to refresh patch-level releases that may contain compilation speed improvements.
- Use sparse registries (`CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse`) for faster dependency resolution when cloning fresh.
- Keep feature flags minimal; disable optional heavy dependencies in leaf crates where possible.

## CI & Remote Builds

- Cache `target/` and `~/.cargo/git`/`registry` directories between CI runs; most providers offer cache actions or volume mounts.
- Configure CI to run `cargo fetch` up front so later steps reuse the same dependency graph without redundant network access.
- For remote development, use `cargo build -Z timings` to identify hot crates and plan targeted refactors.

## Doc Generation

- `scripts/docgen.sh` skips optional OpenAPI generation when PyYAML is unavailable—install it into `.venv` to avoid repeated failures.
- Use `ls interfaces/system_components.json | entr python3 scripts/gen_system_components.py` to regenerate only the affected reference instead of the full docs suite when editing metadata.

## Troubleshooting Slow Builds

- Inspect incremental stats with `cargo build -Zincremental-verify-ich` when diagnosing stale cache issues.
- Drop `target/` if the compiler version changes or `rustc` reports metadata mismatches; stale metadata can add minutes of unnecessary recompilation.
- Profile linkers with `perf record --call-graph=dwarf -- cargo build` (Linux) to ensure no unexpected tooling regressions creep in.

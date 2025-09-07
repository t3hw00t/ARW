# Contributing to ARW

Thank you for helping shape a calm, capable agent platform.

Principles
- Beauty and harmony: keep UI and code clean and understated.
- Local-first safety: predictable behavior, clear policies.
- Rolling optimizations: make it a little faster and clearer each time.

Workflow
1. Build and test locally.
2. Run format and clippy checks.
3. Update docs and regenerate the workspace status page.
4. Keep commits focused and messages descriptive.

Commands
```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
scripts/test.ps1   # or ./scripts/test.sh
scripts/docgen.ps1 # or ./scripts/docgen.sh
```

Rolling optimization checklist
- Hot path review: any obvious allocations, clones, or locks to reduce?
- Async boundaries: spawn wisely, avoid unnecessary blocking.
- Logging: keep context-rich but not noisy; use tracing spans.
- Data shapes: reuse types across API/schema/runtime when possible.
- Build profile: prefer thin LTO; keep codegen-units low for release.

Docs style
- User docs: short, friendly, mildly technical.
- Developer docs: precise, with file paths and commands.
- Use callouts sparingly and let whitespace breathe.


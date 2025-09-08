# Agents Running Wild (ARW)

Local‑first Rust workspace for building and running personal AI agents. ARW
bundles a lightweight service, command‑line tools, and optional tray UI so you
can experiment without cloud lock‑in.

## Highlights

- **User‑mode service** with HTTP endpoints and a simple debug UI.
- **Macro‑driven tool registration** with automatic schema generation.
- **Event stream and tracing** hooks for observability.
- **Portable packaging** scripts for sharing or deployment.

## Component Overview

- **System / Host**: underlying OS, hardware, and runtime paths resolved via environment variables.
- **Core Project**: crates `arw-core`, `arw-protocol`, `arw-events`, `arw-otel`, and helper macros (`arw-macros`), plus binaries `arw-cli`, `arw-svc`, and optional `arw-tray`.
- **External Dependencies**: primary third-party crates such as Tokio, Axum, and Serde.
- **Core Plugins**: none bundled yet; future built-ins will live under `crates/plugins`.
- **Plugin Extensions**: community adapters and optional integrations may live under `crates/adapters`.

All installers and services compute effective paths at startup and write logs/state under the derived directories, keeping deployments portable across machines.

## Quick Start

```bash
# build, generate docs, and create a release package
scripts/setup.sh      # on Linux/macOS (GTK required for arw-tray)
powershell -File scripts/setup.ps1  # on Windows

# start the local service
scripts/start.sh --debug --port 8090
```

The service listens on `http://127.0.0.1:8090`; open `/debug` for a minimal UI.

## Documentation

- [Guide and API docs](docs/)
- [Roadmap](docs/ROADMAP.md)
- [Contributing](CONTRIBUTING.md)

## Community & Support

Questions, ideas, or issues? Open a discussion or file an issue in this
repository. See the [project instructions](docs/PROJECT_INSTRUCTIONS.md) for
background and the [FAQ](docs/guide/FAQ.md) for common questions.

---

ARW is released under the MIT OR Apache‑2.0 license.


set shell := ["bash", "-cu"]

# Build
build:
  cargo build --workspace --release --locked

dev-build:
  cargo build --workspace

# Lint & format
fmt:
  cargo fmt --all

lint:
  cargo clippy --workspace --all-targets -- -D warnings

# Test
test:
  cargo test --workspace --locked

# Package
package:
  bash scripts/package.sh --no-build

# Docs
docgen:
  bash scripts/docgen.sh

docs-build: docgen
  mkdocs build --strict

# Service
start port=8090 debug=1:
  ARW_NO_TRAY=1 bash scripts/start.sh {{ if debug == "1" { "--debug" } else { "" } }} --port {{port}}

# Tasks
task-add title desc="":
  bash scripts/tasks.sh add {{title}} {{ if desc != "" { print("--desc \"" + desc + "\"") }}}

task-start id:
  bash scripts/tasks.sh start {{id}}

task-pause id:
  bash scripts/tasks.sh pause {{id}}

task-done id:
  bash scripts/tasks.sh done {{id}}

task-note id text:
  bash scripts/tasks.sh note {{id}} {{text}}


---
title: Orchestrator CLI
---

# Orchestrator CLI
Updated: 2025-10-22  
Type: How‑to

The `orchestrator` command group inside `arw-cli` helps operators explore the mini-agent catalog, launch persona-aware training runs, and inspect job status directly from the terminal. It mirrors the Training Park launcher flow, including persona tagging and hint propagation, so that CLI, UI, and automation stay in sync.

## Subcommands

### `catalog`

```bash
arw-cli orchestrator catalog \
  --base http://127.0.0.1:8091 \
  --status beta \
  --category governor
```

Fetches `/orchestrator/mini_agents`, applies optional status/category filters, and renders a compact summary (id, preset, runtime estimates, required leases). Use `--json` (and optionally `--pretty`) to dump the raw response.

### `start`

```bash
arw-cli orchestrator start \
  "Improve summarisation quality" \
  --persona-id persona.alpha \
  --preset balanced \
  --diversity 0.35 \
  --recency 0.6 \
  --project demo \
  --topic summarisation \
  --topic empathy \
  --follow
```

Starts a training run via `POST /orchestrator/mini_agents/start_training`. Key points:

- Persona IDs are resolved from `--persona-id` or `ARW_PERSONA_ID`, and propagated through the request so memories, job metadata, and modular actions are tagged consistently.
- Training hints (preset/mode/diversity/recency/compression/budget/episodes) are normalised and merged into both the request payload and the resulting orchestrator hints bundle.
- `--data-json` or `--data-file` allow structured overrides; the CLI validates that overrides are JSON objects and merges them before submission.
- `--follow` polls `/state/orchestrator/jobs` until the run completes, printing status transitions, progress percentages, persona, training hints, and the final result payload. Use Ctrl‑C to stop following without cancelling the job.
- `--json`/`--pretty` return the raw HTTP response without additional formatting (useful for scripting when not following).

### `jobs`

```bash
arw-cli orchestrator jobs --limit 100
```

Lists recent jobs from `/state/orchestrator/jobs`, including persona, training hints, progress, and timestamps. Combine with `--json`/`--pretty` for machine parsing.

### Automation shortcuts

The workspace Justfile provides wrappers that compile/run the CLI with sensible defaults:

- `just orchestrator-start "Improve summarisation" persona.alpha preset=balanced` — launches a training run, exports `ARW_PERSONA_ID`, and follows progress until completion (set `ARW_ADMIN_TOKEN` beforehand).
- `just orchestrator-jobs json=true limit=100` — prints recent job summaries, emitting pretty JSON when `json=true`.
- `just orchestrator-catalog status=beta category=governor json=true` — fetches the mini-agent catalog with filters applied.

## Environment Variables

- `ARW_ADMIN_TOKEN` — token used for admin-gated endpoints; respected by all subcommands via `--admin-token` overrides.
- `ARW_PERSONA_ID` — default persona for `orchestrator start` when `--persona-id` is omitted.

## Exit Codes

- `0` on success.
- `1` when an HTTP request fails, the server is disabled for the requested feature, or when input validation fails (e.g., clamp violations on hint parameters).

## Integration Tips

- Pair `--follow` with `--json` omitted to stream readable status to operators in long-running automation (CI jobs, smoke harness).
- When integrating into scripts, submit with `--json` and parse the `job_id` field before calling `arw-cli orchestrator jobs --json` to monitor.
- Combine with `arw-cli context telemetry --watch` to keep an eye on live working-set and retriever metrics while training jobs run.

---
title: Interactive Performance Bench
---

# Interactive Performance Bench
Updated: 2025-10-16
Type: How‑to

Status: **Ready.** The unified bench lives at `apps/snappy-bench` and ships as a
workspace binary (dev: `cargo run -p snappy-bench`). It drives `/actions` while
listening to `/events` so queue wait, execution time, and end-to-end latency are
measured from a single client.

## Quick start

Run the bench against a local server with defaults (100 actions, concurrency 8). Ensure `ARW_ADMIN_TOKEN` matches the server configuration (export one as shown in the Quickstart guides if you have not already):

```bash
just bench-snappy -- --admin-token "$ARW_ADMIN_TOKEN"
```

> Tip: `export ARW_ADMIN_TOKEN="${ARW_ADMIN_TOKEN:-$(openssl rand -hex 32)}"` matches the examples in the Docker guides. Use any equivalent generator if `openssl` is unavailable, and keep the value aligned with the running server.

Or invoke it directly (debug build):

```bash
cargo run -p snappy-bench -- \
  --base http://127.0.0.1:8091 \
  --admin-token "$ARW_ADMIN_TOKEN" \
  --requests 200 \
  --concurrency 16
```

For production-like numbers, build release binaries once and invoke the compiled tool:

```bash
cargo build --release -p arw-server -p snappy-bench
target/release/snappy-bench \
  --base http://127.0.0.1:8091 \
  --admin-token "$ARW_ADMIN_TOKEN" \
  --requests 200 \
  --concurrency 16
```

The CLI exits non-zero when any action fails or when observed p95 totals exceed
the configured budget (defaults align with `configs/snappy.yaml`). This makes it
safe to drop into CI once a health target is available.

## CLI options

- `--base`: server origin (default `http://127.0.0.1:8091`).
- `--admin-token`: bearer token or `X-ARW-Admin`; falls back to
  `ARW_ADMIN_TOKEN`.
- `--requests`: number of actions to submit (default `100`).
- `--concurrency`: number of in-flight workers (default `8`).
- `--kind`: action kind (default `demo.echo`).
- `--payload` / `--payload-file`: override the default input body.
- `--budget-full-ms`: override `ARW_SNAPPY_FULL_RESULT_P95_MS` (default
  `2000`).
- `--budget-queue-ms`: override `ARW_SNAPPY_I2F_P95_MS` (default `50`).
- `--wait-timeout-secs`: maximum time to wait for completions (default `60`).
- `--json-out`: path to write a JSON summary (latency stats, throughput, failures);
  helpful for CI and regression dashboards.

The tool prints aggregate stats (avg, p50, p95, max) for total latency, queue
wait, simulated run time, and HTTP acknowledgement time. Failures are listed
with the associated action id and a truncated reason.

## Sample output

```
Snappy Bench Summary
- Requests: 100
- Completed: 100
- Failed: 0
- Elapsed: 2.41s
- Throughput: 41.45 actions/s
- Total latency (ms): avg 92.4 | p50 88.0 | p95 132.5 | max 151.9
- Queue wait latency (ms): avg 8.2 | p50 7.4 | p95 15.7 | max 19.3
- Run latency (ms): avg 74.6 | p50 72.8 | p95 110.1 | max 124.8
- HTTP ack latency (ms): avg 16.7 | p50 15.6 | p95 27.3 | max 33.5
```

Budgets that are breached are called out explicitly. For example:

```
! Budget breach: total p95 2120.4 ms > 2000.0 ms
```

Treat these warnings as regressions: either investigate the server or adjust
budgets intentionally alongside documentation updates.

## CI integration

A lightweight sanity run executes as part of the default GitHub Actions CI
(`scripts/ci_snappy_bench.sh`). It builds release binaries if needed, starts
`arw-server`, issues 60 echo actions
with concurrency 6, and fails if p95 totals exceed the configured budgets (queue
budget defaults to 500 ms for CI) or if any request fails. Each run also emits a
machine-readable JSON summary so workflows can archive or plot the results. Tune
via `ARW_BENCH_*` env vars when invoking the script
locally (see script header). For heavier profiling, run the bench manually with
larger request counts/concurrency.

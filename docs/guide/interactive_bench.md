# Interactive Performance Bench (I2F, First Partial, Cadence)
Updated: 2025-09-14
Type: How‑to

A tiny harness to validate the interactive performance budgets against a live service.

- Binary: `arw-svc` → `snappy_bench` (built with the service)
- Measures:
- `i2f_ms`: connect to `/admin/events`; time to first SSE event (service.connected)
  - `first_partial_ms`: POST `/admin/emit/test`; time to `service.test` on SSE
  - `cadence_p95_ms`: p95 inter-arrival ms across a burst of test events
- Budgets (env; defaults match the charter):
  - `ARW_SNAPPY_I2F_P95_MS` (default `50`)
  - `ARW_SNAPPY_FIRST_PARTIAL_P95_MS` (default `150`)
  - `ARW_SNAPPY_CADENCE_MS` (default `250`)
- Strict mode: exit non‑zero on breach when `ARW_BENCH_STRICT=1`

Quickstart (local):

```
ARW_DEBUG=1 ARW_PORT=8095 cargo run -p arw-svc &
sleep 1
ARW_BENCH_BASE=http://127.0.0.1:8095 ARW_BENCH_STRICT=1 \
  cargo run -p arw-svc --bin snappy_bench
```

CI: see `.github/workflows/snappy.yml` (runs on push and PR). Budgets are enforced with strict mode.

Notes
- `/admin/emit/test` publishes a small `service.test` event used by the harness to drive bursts.
- Admin gate: CI runs with `ARW_DEBUG=1`; in production, pass `Authorization: Bearer`.
- SSE contract and resume behavior: see `docs/architecture/sse_patch_contract.md`.

Cold‑start mode

- Set `ARW_BENCH_COLD=1` and provide the service binary path with `ARW_BENCH_EXE`.
- Provide a `ARW_BENCH_BASE` with the desired port (e.g., `http://127.0.0.1:8096`).
- Budget (p95): `ARW_SNAPPY_COLD_START_MS` (default `500`).

Example:

```
cargo build -p arw-svc --bin arw-svc --bin snappy_bench
ARW_BENCH_BASE=http://127.0.0.1:8096 \
ARW_BENCH_EXE=./Agent_Hub/target/debug/arw-svc \
ARW_BENCH_COLD=1 ARW_BENCH_STRICT=1 \
  cargo run -p arw-svc --bin snappy_bench
```

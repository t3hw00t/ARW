---
title: Troubleshooting
---

# Troubleshooting

This page lists quick fixes for common issues when starting ARW locally.

Updated: 2025-09-14
Type: How‑to

## Port Already in Use

Port already in use
- Symptom: `bind` error or `curl` to `/healthz` times out.
- Fix: pick another port.
  - Windows
    ```powershell
    $env:ARW_PORT=8091; powershell -ExecutionPolicy Bypass -File scripts\start.ps1 -WaitHealth
    ```
  - Linux / macOS
    ```bash
    ARW_PORT=8091 bash scripts/start.sh --wait-health
    ```

## Unauthorized Admin Calls (401/403)
- Symptom: Admin endpoints (e.g., `/admin/*`, `/introspect/*`) return 401/403.
- Fix: set a token and send the header.
  ```bash
  export ARW_ADMIN_TOKEN=secret
  curl -sS -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" http://127.0.0.1:8090/admin/introspect/tools
  ```

## Debug UI Missing
- Symptom: `/debug` returns 404 or a minimal page.
- Fix: ensure `ARW_DEBUG=1` for local dev, or run via the Desktop Launcher.
- Tip: `GET /about` should still work without debug. It returns name/version and the `docs_url` link; if `/about` fails, check service logs and port.

## SSE Doesn’t Stream
- Symptom: `curl` returns headers but shows no lines.
- Fix: use `curl -N`, disable proxy buffering, try `?replay=10`.
  ```bash
  curl -N -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" http://127.0.0.1:8090/admin/events?replay=10
  ```

## Model Download Issues

Disk insufficient (preflight)
- Symptom: `models.download.progress` with `code: "disk-insufficient"` and a `need/available/reserve` payload.
- Fix: free space or reduce `ARW_MODELS_DISK_RESERVE_MB` (default 256). Consider GC below.

Disk insufficient (stream)
- Symptom: `code: "disk-insufficient-stream"` mid‑transfer.
- Fix: free up space; retry the same request to resume.

Size exceeds limit
- Symptom: `code: "size-limit"` or `"size-limit-stream"`.
- Fix: increase `ARW_MODELS_MAX_MB` (MiB) or choose a smaller model.

Quota exceeded
- Symptom: `code: "quota-exceeded"` with CAS totals in payload.
- Fix: increase `ARW_MODELS_QUOTA_MB` or remove unused models; see GC.

Checksum mismatch
- Symptom: `code: "checksum-mismatch"` at the end.
- Fix: verify the source and SHA‑256; retry; switch mirror.

Hung/idle
- Symptom: no chunks for a long time; idle timeout.
- Fix: set `ARW_DL_IDLE_TIMEOUT_SECS` (>0) when no hard budget; network/proxy check.

Free space via CAS GC
- Run a one‑off GC to delete unreferenced blobs older than 14 days:
  ```bash
  BASE=http://127.0.0.1:8090
  curl -sS -X POST "$BASE/admin/models/cas_gc" \
    -H 'Content-Type: application/json' \
    -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" \
    -d '{"ttl_days":14}' | jq
  ```
  Listen for `models.cas.gc` summary events.

Metrics
- Admin endpoint: `GET /admin/state/models_metrics` → `{ ewma_mbps, …counters }`.
- SSE: subscribe to `models.download.progress` for status/progress.

## Launcher Build on Linux Fails
- Symptom: errors about WebKitGTK/libsoup.
- Fix: install deps or use Nix dev shell.
  ```bash
  # Option A: project helper
  just tauri-deps-linux
  # Option B: Nix shell with all libs
  nix develop
  ```

## Local llama.cpp or OpenAI Not Used
- Symptom: `/chat/send` errors or falls back.
- Fix: set endpoint or API key.
  ```bash
  export ARW_LLAMA_URL=http://127.0.0.1:8080
  # or
  export ARW_OPENAI_API_KEY=sk-...
  ```

## Still Stuck?
- Check logs in the terminal and in `.arw/logs`.
- Use the Orchestration panel in `/debug` to probe.
- File an issue with your OS, steps, and logs.

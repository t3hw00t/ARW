---
title: Troubleshooting
---

# Troubleshooting

This page lists quick fixes for common issues when starting ARW locally.

Updated: 2025-09-12

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

## SSE Doesn’t Stream
- Symptom: `curl` returns headers but shows no lines.
- Fix: use `curl -N`, disable proxy buffering, try `?replay=10`.
  ```bash
  curl -N -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" http://127.0.0.1:8090/admin/events?replay=10
  ```

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

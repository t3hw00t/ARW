---
title: Subscribe to Events (SSE)
---

# Subscribe to Events (SSE)

Microsummary: Connect to the live Server‑Sent Events stream, filter by prefix, and replay recent events. Admin‑gated. Stable.

Overview
- Endpoint: `GET /admin/events` (text/event-stream)
- Auth: requires admin access; set `ARW_ADMIN_TOKEN` and use it as `Authorization: Bearer <token>` if configured.
- Filters: `?prefix=Models.` (or any event kind prefix)
- Replay: `?replay=N` to emit the last N events on connect (best‑effort)

Examples
- Basic subscription (Unix):
  ```bash
  curl -N http://127.0.0.1:8090/admin/events
  ```
- With admin token and filter:
  ```bash
  curl -N \
    -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
    "http://127.0.0.1:8090/admin/events?prefix=Models.&replay=10"
  ```

Event model
- Events use a compact envelope with `status` (human) and `code` (machine) conventions.
- Common kinds: `Models.DownloadProgress`, `Egress.Ledger.Appended`, `Task.Completed`, `Feedback.Suggested`.
- See Explanations → Events Vocabulary for the canonical list.

Tips
- Stitch episodes using `corr_id` on each event.
- Use `?prefix=` to scope dashboards without client‑side filtering cost.
- For production, proxy and secure `/admin/*` endpoints; do not expose publicly.


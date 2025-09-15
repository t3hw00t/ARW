---
title: Correlation & Attribution
---

# Correlation & Attribution

Updated: 2025-09-15
Type: How‑to

Use correlation headers to stitch egress decisions and events back to actions, projects, and episodes.

- Headers:
  - `X-ARW-Corr`: correlation id (e.g., action id or episode id)
  - `X-ARW-Project`: project id (optional)

When these headers are present, the egress proxy annotates ledger rows and emits `egress.ledger.appended` events that include `corr_id` and `proj`.

## Example
```
curl -x http://127.0.0.1:9080 \
  -H 'X-ARW-Corr: act_1234' \
  -H 'X-ARW-Project: proj-demo' \
  https://api.github.com
```

SSE clients will see:
```
event: egress.ledger.appended
data: { "id": 42, "decision": "allow", "dest_host": "api.github.com", "dest_port": 443, "protocol": "https", "bytes_in": 12345, "corr_id": "act_1234", "proj": "proj-demo", "posture": "standard" }
```

`GET /state/egress` returns recent decisions with these fields as well.

## Automatic Tagging (Local Worker)
- Actions of kind `net.http.get` executed by the local worker automatically add `X-ARW-Corr: <action-id>` when calling the built‑in `http.fetch` (and route via the egress proxy when enabled). This ensures proxy‑side correlation without manual header wiring for built‑in flows.

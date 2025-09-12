---
title: Observability (OTel)
---

# Observability Standard (OpenTelemetry)

Map the episode timeline to trace semantics and expose correlated metrics/logs.

Signals
- Traces: span per observation/intent/action/tool; corr_id as trace id.
- Metrics: route histograms, tool/model/runtime counters, budgets.
- Logs: structured events with trace/span ids.

UI
- Episode timeline overlays spans and budget meters; link to trace explorer.

Notes
- Keep payloads privacy‑respecting by default; redact according to Data Governance.

See also: Metrics & Insights, Events Vocabulary, Replay & Time Travel.


---
title: Observability (OTel)
---

# Observability (OTel)
Updated: 2025-10-09
Type: Explanation

Map the episode timeline to trace semantics and expose correlated metrics/logs.

Signals
- Traces: span per observation/intent/action/tool; corr_id as trace id.
- Metrics: route histograms, tool/model/runtime counters, budgets, cascade freshness gauges (`arw_context_cascade_processed_last`, `arw_context_cascade_skipped_last`, `arw_context_cascade_last_event_id`, `arw_context_cascade_last_event_age_ms`).
- Logs: structured events with trace/span ids.

UI
- Episode timeline overlays spans and budget meters; link to trace explorer.

Notes
- Keep payloads privacy‑respecting by default; redact according to Data Governance.

See also: Metrics & Insights, Events Vocabulary, Replay & Time Travel.

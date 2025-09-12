---
title: Metrics & Insights
---

# Metrics & Insights
{ .topic-trio style="--exp:.6; --complex:.7; --complicated:.6" data-exp=".6" data-complex=".7" data-complicated=".6" }

Updated: 2025-09-07

## Overview
- ARW collects lightweight, privacy‑respecting metrics locally to help you tune and understand behavior.
- Route metrics: hits, errors, EWMA latency, p95 latency, last/max latency, last status.
- Event counters: totals by event kind from the in‑process event bus.

## Endpoints
- GET `/introspect/stats` → `{ events, routes }` where `routes.by_path["/path"]` has `hits`, `errors`, `ewma_ms`, `p95_ms`, `last_ms`, `max_ms`, `last_status`.

## UI
- Open `/debug` and toggle “Insights”.
- See Event totals and the top 3 routes by p95 latency (also shows EWMA and error counts).
- Copy the JSON snapshot via “Copy stats”.

## Security
- `/introspect/*` surfaces are gated by default; see Developer Security Notes.

## Tuning Tips
- Use p95 to find outliers; EWMA helps watch short‑term drift.
- Send a “latency” signal in the Self‑Learning panel targeting a hot route; Analyze; consider applying the suggested `http_timeout_secs`.
- Consider switching to the “balanced” profile during high error periods.

## Observability Discipline
- Four golden signals: latency, traffic, errors, saturation — at tool/model/runtime granularity.
- Per‑episode timelines: obs → belief → intent → action; include streamed tokens and tool I/O.
- Per‑project aggregates: success rates, retrieval diversity, cost, and error classes over time.
- Exportable traces: correlation id and spans attach to problem details and event envelopes.

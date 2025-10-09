---
title: Grafana — Quick Panels (CPU/Mem/GPU)
---

# Grafana — Quick Panels (CPU/Mem/GPU)

Updated: 2025-10-09
Type: How‑to

A minimal Grafana dashboard with quick panels: CPU avg %, Mem usage %, GPU mem usage %, and CPU per-core time series. The GPU panels require the upcoming GPU telemetry pack (`arw_gpu_*` metrics); omit those queries if the pack is not enabled yet.

## Import JSON

```json
{
  "__inputs": [
    {
      "name": "DS_PROMETHEUS",
      "label": "Prometheus",
      "description": "Prometheus datasource",
      "type": "datasource",
      "pluginId": "prometheus",
      "pluginName": "Prometheus"
    }
  ],
  "__requires": [
    { "type": "grafana", "id": "grafana", "name": "Grafana", "version": "9.5.0" },
    { "type": "panel", "id": "gauge", "name": "Gauge", "version": "8.0.0" },
    { "type": "datasource", "id": "prometheus", "name": "Prometheus", "version": "1.0.0" }
  ],
  "title": "ARW — Quick Panels",
  "timezone": "browser",
  "schemaVersion": 36,
  "version": 1,
  "refresh": "15s",
  "tags": ["arw","quick"],
  "templating": { "list": [{ "name": "DS_PROMETHEUS", "type": "datasource", "query": "prometheus", "label": "Prometheus" }] },
  "panels": [
    {
      "type": "gauge",
      "title": "CPU avg % (5m)",
      "gridPos": {"x": 0, "y": 0, "w": 8, "h": 6},
      "fieldConfig": {"defaults": {"unit": "percent", "min": 0, "max": 100}},
      "datasource": {"type": "prometheus", "uid": "${DS_PROMETHEUS}"},
      "targets": [ {"expr": "arw:cpu_percent_avg:5m OR arw_cpu_percent_avg", "legendFormat": "cpu %"} ]
    },
    {
      "type": "gauge",
      "title": "Mem usage %",
      "gridPos": {"x": 8, "y": 0, "w": 8, "h": 6},
      "fieldConfig": {"defaults": {"unit": "percent", "min": 0, "max": 100}},
      "datasource": {"type": "prometheus", "uid": "${DS_PROMETHEUS}"},
      "targets": [ {"expr": "arw:mem_usage_percent OR (100 * arw_mem_bytes_used / arw_mem_bytes_total)", "legendFormat": "mem %"} ]
    },
    {
      "type": "gauge",
      "title": "GPU mem usage %",
      "gridPos": {"x": 16, "y": 0, "w": 8, "h": 6},
      "fieldConfig": {"defaults": {"unit": "percent", "min": 0, "max": 100}},
      "datasource": {"type": "prometheus", "uid": "${DS_PROMETHEUS}"},
      "targets": [ {"expr": "arw:gpu_mem_usage_percent OR (100 * sum(arw_gpu_adapter_memory_bytes{kind=\"used\"}) / sum(arw_gpu_adapter_memory_bytes{kind=\"total\"}))", "legendFormat": "gpu %"} ]
    },
    {
      "type": "stat",
      "title": "Cascade freshness (minutes)",
      "gridPos": {"x": 12, "y": 5, "w": 6, "h": 5},
      "fieldConfig": {
        "defaults": {
          "unit": "m",
          "mappings": [],
          "thresholds": {
            "mode": "absolute",
            "steps": [
              {"value": null, "color": "green"},
              {"value": 10, "color": "orange"},
              {"value": 20, "color": "red"}
            ]
          }
        }
      },
      "datasource": {"type": "prometheus", "uid": "${DS_PROMETHEUS}"},
      "targets": [
        {
          "expr": "arw_context_cascade_last_event_age_ms / 60000",
          "legendFormat": "age"
        }
      ]
    },
    {
      "type": "timeseries",
      "title": "CPU % per core",
      "gridPos": {"x": 0, "y": 10, "w": 24, "h": 6},
      "fieldConfig": {"defaults": {"unit": "percent"}},
      "options": {"legend": {"displayMode": "table"}},
      "datasource": {"type": "prometheus", "uid": "${DS_PROMETHEUS}"},
      "targets": [ {"expr": "arw_cpu_percent_core", "legendFormat": "core {{core}}"} ]
    }
  ]
}
```

Import: Grafana → Dashboards → Import → paste JSON → select Prometheus datasource.

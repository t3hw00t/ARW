---
title: Grafana — GPU/NPU Dashboard (Prometheus)
---

# Grafana — GPU/NPU Dashboard (Prometheus)

Updated: 2025-09-15
Type: How‑to

This dashboard visualizes the GPU/NPU metrics exported by ARW at `/metrics`. Import the JSON below into Grafana and select your Prometheus data source when prompted.

Notes
- Requires ARW to be running; the service exposes Prometheus metrics at `GET /metrics`.
- Metrics used: `arw_gpu_*`, `arw_npu_*`, and `arw_gpu_adapter_info`.
- The dashboard defines a datasource input named `DS_PROMETHEUS`.

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
    { "type": "panel", "id": "stat", "name": "Stat", "version": "8.0.0" },
    { "type": "panel", "id": "gauge", "name": "Gauge", "version": "8.0.0" },
    { "type": "panel", "id": "timeseries", "name": "Time series", "version": "8.0.0" },
    { "type": "panel", "id": "bargauge", "name": "Bar gauge", "version": "8.0.0" },
    { "type": "datasource", "id": "prometheus", "name": "Prometheus", "version": "1.0.0" }
  ],
  "title": "ARW — GPU/NPU Overview",
  "timezone": "browser",
  "schemaVersion": 36,
  "version": 1,
  "refresh": "30s",
  "tags": ["arw","gpu"],
  "templating": {
    "list": [
      {
        "name": "DS_PROMETHEUS",
        "type": "datasource",
        "query": "prometheus",
        "label": "Prometheus",
        "hide": 0
      }
    ]
  },
  "panels": [
    {
      "type": "stat",
      "title": "GPU adapters",
      "gridPos": {"x": 0, "y": 0, "w": 6, "h": 4},
      "datasource": {"type": "prometheus", "uid": "${DS_PROMETHEUS}"},
      "targets": [ {"expr": "arw_gpu_adapters", "legendFormat": "adapters"} ]
    },
    {
      "type": "stat",
      "title": "NPU adapters",
      "gridPos": {"x": 6, "y": 0, "w": 6, "h": 4},
      "datasource": {"type": "prometheus", "uid": "${DS_PROMETHEUS}"},
      "targets": [ {"expr": "arw_npu_adapters", "legendFormat": "adapters"} ]
    },
    {
      "type": "gauge",
      "title": "GPU mem usage %",
      "gridPos": {"x": 12, "y": 0, "w": 12, "h": 8},
      "fieldConfig": {"defaults": {"unit": "percent", "min": 0, "max": 100}},
      "datasource": {"type": "prometheus", "uid": "${DS_PROMETHEUS}"},
      "targets": [
        {"expr": "100 * sum(arw_gpu_adapter_memory_bytes{kind=\"used\"}) / sum(arw_gpu_adapter_memory_bytes{kind=\"total\"})", "legendFormat": "mem %"}
      ]
    },
    {
      "type": "bargauge",
      "title": "VRAM by vendor (GiB)",
      "gridPos": {"x": 0, "y": 4, "w": 12, "h": 8},
      "fieldConfig": {"defaults": {"unit": "gibibytes"}},
      "datasource": {"type": "prometheus", "uid": "${DS_PROMETHEUS}"},
      "targets": [
        {
          "expr": "sum by (vendor) (arw_gpu_adapter_memory_bytes{kind=\"total\"} * on (index) group_left(vendor) arw_gpu_adapter_info) / (1024*1024*1024)",
          "legendFormat": "{{vendor}}"
        }
      ]
    },
    {
      "type": "timeseries",
      "title": "GPU busy % by adapter",
      "gridPos": {"x": 0, "y": 12, "w": 24, "h": 8},
      "fieldConfig": {"defaults": {"unit": "percent"}},
      "options": {"legend": {"displayMode": "table"}},
      "datasource": {"type": "prometheus", "uid": "${DS_PROMETHEUS}"},
      "targets": [
        {
          "expr": "arw_gpu_adapter_busy_percent",
          "legendFormat": "{{index}}"
        }
      ]
    }
  ]
}
```

Import: Grafana → Dashboards → Import → paste JSON → select Prometheus datasource.


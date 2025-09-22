---
title: Interface Release Notes
---

# Interface Release Notes
Updated: 2025-09-22
Type: Reference

Base: `origin/main`

## OpenAPI (REST)

```diff
--- openapi.base.yaml
+++ openapi.head.yaml
@@ -2389,181 +2389,6 @@
           - 'null'
           format: int64
           minimum: 0
-    ModelsConcurrencySnapshot:
-      type: object
-      required:
-      - configured_max
-      - available_permits
-      - held_permits
-      properties:
-        configured_max:
-          type: integer
-          format: int64
-          minimum: 0
-        available_permits:
-          type: integer
-          format: int64
-          minimum: 0
-        held_permits:
-          type: integer
-          format: int64
-          minimum: 0
-        hard_cap:
-          type:
-          - integer
-          - 'null'
-          format: int64
-          minimum: 0
-        pending_shrink:
-          type:
-          - integer
-          - 'null'
-          format: int64
-          minimum: 0
-    ModelsJobDestination:
-      type: object
-      required:
-      - host
-      - port
-      - protocol
-      properties:
-        host:
-          type: string
-        port:
-          type: integer
-          format: int32
-          minimum: 0
-        protocol:
-          type: string
-    ModelsJobSnapshot:
-      type: object
-      required:
-      - model_id
-      - job_id
-      - url
-      - corr_id
-      - dest
-      - started_at
-      properties:
-        model_id:
-          type: string
-        job_id:
-          type: string
-        url:
-          type: string
-        corr_id:
-          type: string
-        dest:
-          $ref: '#/components/schemas/ModelsJobDestination'
-        started_at:
-          type: integer
-          format: int64
-          minimum: 0
-    ModelsInflightEntry:
-      type: object
-      required:
-      - sha256
-      - primary
-      - count
-      properties:
-        sha256:
-          type: string
-        primary:
-          type: string
-        followers:
-          type: array
-          items:
-            type: string
-        count:
-          type: integer
-          format: int64
-          minimum: 0
-    ModelsMetricsResponse:
-      type: object
-      required:
-      - started
-      - queued
-      - admitted
-      - resumed
-      - canceled
-      - completed
-      - completed_cached
-      - errors
-      - bytes_total
-      - preflight_ok
-      - preflight_denied
-      - preflight_skipped
-      - coalesced
-      - inflight
-      - concurrency
-      - jobs
-      properties:
-        started:
-          type: integer
-          format: int64
-          minimum: 0
-        queued:
-          type: integer
-          format: int64
-          minimum: 0
-        admitted:
-          type: integer
-          format: int64
-          minimum: 0
-        resumed:
-          type: integer
-          format: int64
-          minimum: 0
-        canceled:
-          type: integer
-          format: int64
-          minimum: 0
-        completed:
-          type: integer
-          format: int64
-          minimum: 0
-        completed_cached:
-          type: integer
-          format: int64
-          minimum: 0
-        errors:
-          type: integer
-          format: int64
-          minimum: 0
-        bytes_total:
-          type: integer
-          format: int64
-          minimum: 0
-        ewma_mbps:
-          type:
-          - number
-          - 'null'
-        preflight_ok:
-          type: integer
-          format: int64
-          minimum: 0
-        preflight_denied:
-          type: integer
-          format: int64
-          minimum: 0
-        preflight_skipped:
-          type: integer
-          format: int64
-          minimum: 0
-        coalesced:
-          type: integer
-          format: int64
-          minimum: 0
-        inflight:
-          type: array
-          items:
-            $ref: '#/components/schemas/ModelsInflightEntry'
-        concurrency:
-          $ref: '#/components/schemas/ModelsConcurrencySnapshot'
-        jobs:
-          type: array
-          items:
-            $ref: '#/components/schemas/ModelsJobSnapshot'
     CoreAccept:
       type: object
       required:
@@ -3321,6 +3146,180 @@
       properties:
         id:
           type: string
+    ModelsConcurrencySnapshot:
+      type: object
+      required:
+      - configured_max
+      - available_permits
+      - held_permits
+      properties:
+        available_permits:
+          type: integer
+          format: int64
+          minimum: 0
+        configured_max:
+          type: integer
+          format: int64
+          minimum: 0
+        hard_cap:
+          type:
+          - integer
+          - 'null'
+          format: int64
+          minimum: 0
+        held_permits:
+          type: integer
+          format: int64
+          minimum: 0
+        pending_shrink:
+          type:
+          - integer
+          - 'null'
+          format: int64
+          minimum: 0
+    ModelsInflightEntry:
+      type: object
+      required:
+      - sha256
+      - primary
+      - count
+      properties:
+        count:
+          type: integer
+          format: int64
+          minimum: 0
+        followers:
+          type: array
+          items:
+            type: string
+        primary:
+          type: string
+        sha256:
+          type: string
+    ModelsJobDestination:
+      type: object
+      required:
+      - host
+      - port
+      - protocol
+      properties:
+        host:
+          type: string
+        port:
+          type: integer
+          format: int32
+          minimum: 0
+        protocol:
+          type: string
+    ModelsJobSnapshot:
+      type: object
+      required:
+      - model_id
+      - job_id
+      - url
+      - corr_id
+      - dest
+      - started_at
+      properties:
+        corr_id:
+          type: string
+        dest:
+          $ref: '#/components/schemas/ModelsJobDestination'
+        job_id:
+          type: string
+        model_id:
+          type: string
+        started_at:
+          type: integer
+          format: int64
+          minimum: 0
+        url:
+          type: string
+    ModelsMetricsResponse:
+      type: object
+      required:
+      - started
+      - queued
+      - admitted
+      - resumed
+      - canceled
+      - completed
+      - completed_cached
+      - errors
+      - bytes_total
+      - preflight_ok
+      - preflight_denied
+      - preflight_skipped
+      - coalesced
+      - concurrency
+      properties:
+        admitted:
+          type: integer
+          format: int64
+          minimum: 0
+        bytes_total:
+          type: integer
+          format: int64
+          minimum: 0
+        canceled:
+          type: integer
+          format: int64
+          minimum: 0
+        coalesced:
+          type: integer
+          format: int64
+          minimum: 0
+        completed:
+          type: integer
+          format: int64
+          minimum: 0
+        completed_cached:
+          type: integer
+          format: int64
+          minimum: 0
+        concurrency:
+          $ref: '#/components/schemas/ModelsConcurrencySnapshot'
+        errors:
+          type: integer
+          format: int64
+          minimum: 0
+        ewma_mbps:
+          type:
+          - number
+          - 'null'
+          format: double
+        inflight:
+          type: array
+          items:
+            $ref: '#/components/schemas/ModelsInflightEntry'
+        jobs:
+          type: array
+          items:
+            $ref: '#/components/schemas/ModelsJobSnapshot'
+        preflight_denied:
+          type: integer
+          format: int64
+          minimum: 0
+        preflight_ok:
+          type: integer
+          format: int64
+          minimum: 0
+        preflight_skipped:
+          type: integer
+          format: int64
+          minimum: 0
+        queued:
+          type: integer
+          format: int64
+          minimum: 0
+        resumed:
+          type: integer
+          format: int64
+          minimum: 0
+        started:
+          type: integer
+          format: int64
+          minimum: 0
     OrchestratorStartReq:
       type: object
       required:
```

## AsyncAPI (Events)

No changes


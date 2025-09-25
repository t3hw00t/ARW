---
title: Interface Release Notes
---

# Interface Release Notes
Updated: 2025-09-25
Type: Reference

Base: `origin/main`

## OpenAPI (REST)

```diff
--- openapi.base.yaml
+++ openapi.head.yaml
@@ -3479,6 +3479,14 @@
       - failed
       - avg_latency_ms
       properties:
+        avg_ctx_items:
+          type: integer
+          format: int64
+          minimum: 0
+        avg_ctx_tokens:
+          type: integer
+          format: int64
+          minimum: 0
         avg_latency_ms:
           type: integer
           format: int64
```

## AsyncAPI (Events)

```diff
--- asyncapi.base.yaml
+++ asyncapi.head.yaml
@@ -552,7 +552,10 @@
       operationId: models_download_progress_event
       summary: "models.download.progress event"
       message:
-        $ref: '#/components/messages/ModelsDownloadProgress'
+        name: 'models.download.progress'
+        payload:
+          type: object
+          additionalProperties: true
   'models.manifest.written':
     subscribe:
       operationId: models_manifest_written_event
@@ -967,85 +970,3 @@
         payload:
           type: object
           additionalProperties: true
-
-components:
-  messages:
-    ModelsDownloadProgress:
-      name: models.download.progress
-      title: Models Download Progress
-      summary: Structured progress updates emitted while models download jobs run.
-      contentType: application/json
-      payload:
-        type: object
-        required:
-          - id
-        additionalProperties: true
-        properties:
-          id:
-            type: string
-            description: Model identifier or follower key that the progress update applies to.
-          status:
-            type: string
-            description: High-level lifecycle status for the download job.
-            enum:
-              - canceled
-              - coalesced
-              - complete
-              - degraded
-              - downloading
-              - error
-              - no-active-job
-              - preflight
-              - resumed
-              - started
-          code:
-            type: string
-            description: Machine-actionable code providing additional context for the status (especially errors).
-            enum:
-              - disk_insufficient
-              - hard-budget
-              - hash-guard
-              - http
-              - idle-timeout
-              - io
-              - quota_exceeded
-              - request-timeout
-              - resume-content-range
-              - resume-http-status
-              - resumed
-              - sha256_mismatch
-              - size_limit
-              - skipped
-              - soft-budget
-          error_code:
-            type: string
-            description: Legacy alias for `code`; retained for backwards compatibility.
-          corr_id:
-            type: string
-            description: Correlation identifier propagated across related progress/update events.
-          downloaded:
-            type: integer
-            format: int64
-            minimum: 0
-            description: Bytes downloaded so far (if known).
-          total:
-            type: integer
-            format: int64
-            minimum: 0
-            description: Total bytes expected (if known).
-          percent:
-            type: number
-            format: float
-            description: Percentage complete (0-100) when the service can compute it.
-          cached:
-            type: boolean
-            description: Indicates the artifact was satisfied from cache/coalesced follower output.
-          source:
-            type: string
-            description: Origin of the progress event (e.g., `coalesced`).
-          budget:
-            type: object
-            description: Snapshot of budget utilisation when progress hints are enabled.
-          disk:
-            type: object
-            description: Snapshot of disk utilisation when progress hints are enabled.
```


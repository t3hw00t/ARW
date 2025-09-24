---
title: Interface Release Notes
---

# Interface Release Notes
Updated: 2025-09-23
Type: Reference

Base: `origin/main`

## OpenAPI (REST)

```diff
--- openapi.base.yaml
+++ openapi.head.yaml
@@ -1947,38 +1947,10 @@
       operationId: state_projects_list_doc
       responses:
         '200':
-          description: Projects read-model snapshot
+          description: Projects list
           content:
             application/json:
-              schema:
-                type: object
-                properties:
-                  generated:
-                    type: string
-                    description: RFC3339 timestamp when the snapshot was generated.
-                  items:
-                    type: array
-                    items:
-                      type: object
-                      properties:
-                        name:
-                          type: string
-                        notes:
-                          type: object
-                          additionalProperties: true
-                        tree:
-                          type: object
-                          properties:
-                            digest:
-                              type: string
-                            paths:
-                              type: object
-                              additionalProperties:
-                                type: array
-                                items:
-                                  type: object
-                        generated:
-                          type: string
+              schema: {}
       description: 'Projects: GET /state/projects.'
   /state/projects/{proj}/file:
     get:
@@ -2363,6 +2335,17 @@
       properties:
         prompt:
           type: string
+        temperature:
+          type:
+          - number
+          - 'null'
+          format: double
+        vote_k:
+          type:
+          - integer
+          - 'null'
+          format: int64
+          minimum: 0
     ChatSendResp:
       type: object
       required:
@@ -3344,10 +3327,40 @@
           type: integer
           format: int64
           minimum: 0
+        runtime:
+          $ref: '#/components/schemas/ModelsRuntimeConfig'
         started:
           type: integer
           format: int64
           minimum: 0
+    ModelsRuntimeConfig:
+      type: object
+      required:
+      - send_retries
+      - stream_retries
+      - retry_backoff_ms
+      - preflight_enabled
+      properties:
+        idle_timeout_secs:
+          type:
+          - integer
+          - 'null'
+          format: int64
+          minimum: 0
+        preflight_enabled:
+          type: boolean
+        retry_backoff_ms:
+          type: integer
+          format: int64
+          minimum: 0
+        send_retries:
+          type: integer
+          format: int32
+          minimum: 0
+        stream_retries:
+          type: integer
+          format: int32
+          minimum: 0
     OrchestratorStartReq:
       type: object
       required:
```

## AsyncAPI (Events)

No changes


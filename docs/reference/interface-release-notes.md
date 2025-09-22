---
title: Interface Release Notes
---

# Interface Release Notes
Updated: 2025-09-21
Type: Reference

Base: `origin/main`

## OpenAPI (REST)

No changes

## AsyncAPI (Events)

```diff
--- asyncapi.base.yaml
+++ asyncapi.head.yaml
@@ -403,6 +403,15 @@
         payload:
           type: object
           additionalProperties: true
+  'leases.created':
+    subscribe:
+      operationId: leases_created_event
+      summary: "leases.created event"
+      message:
+        name: 'leases.created'
+        payload:
+          type: object
+          additionalProperties: true
   'logic.unit.applied':
     subscribe:
       operationId: logic_unit_applied_event
```


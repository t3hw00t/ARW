---
title: Interface Release Notes
---

# Interface Release Notes

Base: `origin/main`

## OpenAPI (REST)

No changes

## AsyncAPI (Events)

```diff
--- /tmp/tmpb44tzi7a/asyncapi.base.yaml	2025-09-16 14:34:04.902615625 +0200
+++ /tmp/tmpb44tzi7a/asyncapi.head.yaml	2025-09-16 14:34:04.902615625 +0200
@@ -150,8 +150,71 @@
         type: object
         properties:
           id: { type: string }
-          status: { type: string }
-          code: { type: string }
+          status:
+            type: string
+            enum:
+              - started
+              - queued
+              - admitted
+              - downloading
+              - resumed
+              - resync
+              - degraded
+              - canceled
+              - complete
+              - cancel-requested
+              - no-active-job
+              - cache-mismatch
+              - error
+          code:
+            type: string
+            enum:
+              # lifecycle & progress codes
+              - started
+              - queued
+              - admitted
+              - downloading
+              - progress
+              - resumed
+              - resync
+              - degraded
+              - canceled-by-user
+              - complete
+              - cached
+              - already-in-progress
+              - already-in-progress-hash
+              - cache-mismatch
+              - soft-exhausted
+              - cancel-requested
+              - no-active-job
+              # error/guard codes
+              - request-failed
+              - concurrency-closed
+              - downstream-http-status
+              - upstream-changed
+              - resume-no-content-range
+              - resume-http-status
+              - resume-failed
+              - resync-failed
+              - quota-exceeded
+              - size-limit
+              - idle-timeout
+              - hard-exhausted
+              - io-read
+              - io-write
+              - flush-failed
+              - mkdir-failed
+              - open-failed
+              - create-failed
+              - verify-open-failed
+              - verify-read-failed
+              - checksum-mismatch
+              - size-mismatch
+              - finalize-failed
+              - admission-denied
+              - disk-insufficient
+              - size-limit-stream
+              - disk-insufficient-stream
           error: { type: string }
           progress: { type: integer }
           downloaded: { type: integer }

```


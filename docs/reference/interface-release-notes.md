---
title: Interface Release Notes
---

# Interface Release Notes
Updated: 2025-09-21

Base: `origin/main`

## OpenAPI (REST)

No changes

## AsyncAPI (Events)

```diff
--- /tmp/tmpv6cd2vfu/asyncapi.base.yaml	2025-09-21 00:12:02.773027926 +0200
+++ /tmp/tmpv6cd2vfu/asyncapi.head.yaml	2025-09-21 00:12:02.773027926 +0200
@@ -153,25 +153,75 @@
           status:
             type: string
             enum:
-              - started
+              - queued
+              - admitted
               - downloading
+              - resumed
+              - resync
               - degraded
               - canceled
               - complete
+              - cancel-requested
               - no-active-job
+              - cache-mismatch
               - error
           code:
             type: string
             enum:
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
               - soft-budget
-              - hard-budget
+              - soft-exhausted
+              - cancel-requested
+              - no-active-job
+              # error/guard codes
               - request-timeout
-              - http
-              - io
-              - size_limit
+              - request-failed
+              - concurrency-closed
+              - downstream-http-status
+              - upstream-changed
+              - resume-no-content-range
+              - resume-http-status
+              - resume-failed
+              - resync-failed
+              - quota-exceeded
               - quota_exceeded
-              - disk_insufficient
+              - size-limit
+              - size_limit
+              - idle-timeout
+              - hard-budget
+              - hard-exhausted
+              - io
+              - io-read
+              - io-write
+              - flush-failed
+              - mkdir-failed
+              - open-failed
+              - create-failed
+              - verify-open-failed
+              - verify-read-failed
+              - checksum-mismatch
               - sha256_mismatch
+              - size-mismatch
+              - finalize-failed
+              - admission-denied
+              - disk-insufficient
+              - disk_insufficient
+              - size-limit-stream
+              - disk-insufficient-stream
           error: { type: string }
           progress: { type: integer }
           downloaded: { type: integer }

```


---
title: Interface Release Notes
---

# Interface Release Notes

Base: `origin/main` vs Head: `e8f16ef3`

## OpenAPI (REST)

```diff
--- /tmp/tmppj634u0_/openapi.base.yaml	2025-09-13 20:43:12.858379760 +0200
+++ /tmp/tmppj634u0_/openapi.head.yaml	2025-09-13 20:43:12.858379760 +0200
@@ -1,10 +1,34 @@
 openapi: 3.1.0
 info:
-  title: arw-svc
-  description: ''
+  title: Agent Hub (ARW) Service API
+  description: |
+    Your private AI control room that can scale and share when you choose.
+
+    In plain terms: Agent Hub (ARW) lets you run your own team of AI “helpers” on your computer to research, plan, write, and build—while you stay in charge. It is local‑first and privacy‑first by default, with the option to securely pool computing power with trusted peers when a project needs more muscle.
   license:
     name: ''
   version: 0.1.0
+tags:
+  - name: Public
+    description: Public, unauthenticated endpoints
+  - name: Public/Specs
+    description: Public endpoints serving raw interface specifications and index
+  - name: Admin/Core
+    description: Administrative core operations (SSE, emit tests, shutdown)
+  - name: Admin/Chat
+    description: Debug chat endpoints (deprecated)
+  - name: Admin/Feedback
+    description: Feedback signals, suggestions, policy
+  - name: Admin/Governor
+    description: Runtime governor knobs and hints
+  - name: Admin/Hierarchy
+    description: Hierarchy state and control plane
+  - name: Admin/Projects
+    description: Projects admin endpoints
+  - name: Admin/State
+    description: Read‑models and state views
+  - name: Admin/Tools
+    description: Tools registry and execution
 paths:
   /about:
     get:
@@ -14,10 +38,17 @@
       responses:
         '200':
           description: About service
+          content:
+            application/json:
+              schema:
+                $ref: '#/components/schemas/About'
   /admin/chat:
     get:
       tags:
       - Admin/Chat
+      summary: "Deprecated: Chat history"
+      deprecated: true
+      x-sunset: '2026-01-01T00:00:00Z'
       operationId: chat_get_doc
       responses:
         '200':
@@ -32,6 +63,9 @@
     post:
       tags:
       - Admin/Chat
+      summary: "Deprecated: Clear chat history"
+      deprecated: true
+      x-sunset: '2026-01-01T00:00:00Z'
       operationId: chat_clear_doc
       responses:
         '200':
@@ -50,6 +84,9 @@
     post:
       tags:
       - Admin/Chat
+      summary: "Deprecated: Send chat message"
+      deprecated: true
+      x-sunset: '2026-01-01T00:00:00Z'
       operationId: chat_send_doc
       requestBody:
         content:
@@ -1181,6 +1218,39 @@
           description: Service version
 components:
   schemas:
+    About:
+      type: object
+      description: Service and branding information.
+      properties:
+        name:
+          type: string
+          example: Agent Hub (ARW)
+        tagline:
+          type: string
+          example: Your private AI control room that can scale and share when you choose.
+        description:
+          type: string
+        service:
+          type: string
+          example: arw-svc
+        version:
+          type: string
+          example: 0.1.0
+        role:
+          type: string
+          example: Home
+        docs_url:
+          type: string
+          format: uri
+          example: https://t3hw00t.github.io/ARW/
+        endpoints:
+          type: array
+          items:
+            type: string
+      required:
+        - service
+        - version
+        - endpoints
     ApplyMemory:
       type: object
       required:

```

## AsyncAPI (Events)

```diff
--- /tmp/tmppj634u0_/asyncapi.base.yaml	2025-09-13 20:43:17.610259211 +0200
+++ /tmp/tmppj634u0_/asyncapi.head.yaml	2025-09-13 20:43:17.610259211 +0200
@@ -3,6 +3,9 @@
   title: "arw-svc events"
   version: "0.1.0"
 defaultContentType: application/json
+tags:
+  - name: CloudEvents
+    description: Events include CloudEvents 1.0 metadata under `ce` (see components.schemas.EventEnvelope)
 channels:
   Service.Start:
     subscribe:
@@ -185,6 +188,31 @@
           bytes_out: { type: integer }
           bytes_in: { type: integer }
           duration_ms: { type: integer }
+  schemas:
+    EventEnvelope:
+      type: object
+      description: Envelope delivered over SSE; includes CloudEvents metadata under `ce`.
+      properties:
+        time:
+          type: string
+          format: date-time
+        kind:
+          type: string
+          description: Event type (also mapped to CloudEvents `type`)
+        payload:
+          description: Event payload (varies by channel)
+        policy:
+          description: Optional gating capsule
+        ce:
+          type: object
+          description: CloudEvents 1.0 metadata
+          properties:
+            specversion: { type: string, enum: ["1.0"] }
+            type: { type: string }
+            source: { type: string }
+            id: { type: string }
+            time: { type: string, format: date-time }
+            datacontenttype: { type: string }
     ToolRan:
       name: Tool.Ran
       payload:

```


---
title: Interface Release Notes
---

# Interface Release Notes

Base: `origin/main` vs Head: `5f09743d`

## OpenAPI (REST)

```diff
--- /tmp/tmphrg03xwl/openapi.base.yaml	2025-09-13 22:49:09.731111020 +0200
+++ /tmp/tmphrg03xwl/openapi.head.yaml	2025-09-13 22:49:09.731111020 +0200
@@ -1,23 +1,67 @@
 openapi: 3.1.0
 info:
-  title: arw-svc
-  description: ''
+  title: Agent Hub (ARW) Service API
+  description: "Your private AI control room that can scale and share when you choose.\n\
+    \nIn plain terms: Agent Hub (ARW) lets you run your own team of AI \u201Chelpers\u201D\
+    \ on your computer to research, plan, write, and build\u2014while you stay in\
+    \ charge. It is local\u2011first and privacy\u2011first by default, with the option\
+    \ to securely pool computing power with trusted peers when a project needs more\
+    \ muscle.\n"
   license:
     name: ''
   version: 0.1.0
+tags:
+- name: Admin/Chat
+  description: Admin/Chat endpoints
+- name: Admin/Core
+  description: Admin/Core endpoints
+- name: Admin/Feedback
+  description: Admin/Feedback endpoints
+- name: Admin/Governor
+  description: Admin/Governor endpoints
+- name: Admin/Hierarchy
+  description: Admin/Hierarchy endpoints
+- name: Admin/Introspect
+  description: Admin/Introspect endpoints
+- name: Admin/Memory
+  description: Admin/Memory endpoints
+- name: Admin/Models
+  description: Admin/Models endpoints
+- name: Admin/Projects
+  description: Admin/Projects endpoints
+- name: Admin/State
+  description: Admin/State endpoints
+- name: Admin/Tasks
+  description: Admin/Tasks endpoints
+- name: Admin/Tools
+  description: Admin/Tools endpoints
+- name: Public
+  description: Public endpoints
+- name: Public/Specs
+  description: Public/Specs endpoints
 paths:
   /about:
     get:
       tags:
       - Public
+      description: Return service metadata and branding for the running instance.
       operationId: about_doc
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
+      summary: 'Deprecated: Chat history'
+      deprecated: true
+      x-sunset: "2026-01-01T00:00:00Z"
+      description: Deprecated dev chat history used by the debug UI; scheduled for
+        removal after sunset.
       operationId: chat_get_doc
       responses:
         '200':
@@ -32,6 +76,10 @@
     post:
       tags:
       - Admin/Chat
+      summary: 'Deprecated: Clear chat history'
+      deprecated: true
+      x-sunset: "2026-01-01T00:00:00Z"
+      description: Deprecated dev helper to clear in-memory chat history.
       operationId: chat_clear_doc
       responses:
         '200':
@@ -50,6 +98,10 @@
     post:
       tags:
       - Admin/Chat
+      summary: 'Deprecated: Send chat message'
+      deprecated: true
+      x-sunset: "2026-01-01T00:00:00Z"
+      description: Deprecated dev helper to send a message to the synthetic chat backend.
       operationId: chat_send_doc
       requestBody:
         content:
@@ -74,7 +126,8 @@
     get:
       tags:
       - Admin/Core
-      operationId: emit_test
+      description: Emit a test event onto the internal event bus (for verification).
+      operationId: emit_test_doc
       responses:
         '200':
           description: Emit test event
@@ -92,7 +145,9 @@
     get:
       tags:
       - Admin/Core
-      operationId: events
+      description: Server-Sent Events stream; emits JSON envelopes with CloudEvents
+        metadata; supports Last-Event-ID resume and ?replay=N.
+      operationId: events_doc
       responses:
         '200':
           description: SSE event stream
@@ -106,6 +161,7 @@
     post:
       tags:
       - Admin/Feedback
+      description: Analyze recorded signals and stats to propose suggestions.
       operationId: feedback_analyze_post_doc
       responses:
         '200':
@@ -124,6 +180,7 @@
     post:
       tags:
       - Admin/Feedback
+      description: Apply a suggestion by id (policy-gated).
       operationId: feedback_apply_post_doc
       requestBody:
         content:
@@ -148,6 +205,7 @@
     post:
       tags:
       - Admin/Feedback
+      description: Enable or disable automatic application of accepted suggestions.
       operationId: feedback_auto_post_doc
       requestBody:
         content:
@@ -172,6 +230,7 @@
     get:
       tags:
       - Admin/Feedback
+      description: Return the effective feedback policy and tunables.
       operationId: feedback_policy_doc
       responses:
         '200':
@@ -186,6 +245,7 @@
     post:
       tags:
       - Admin/Feedback
+      description: Clear feedback signals and suggestions.
       operationId: feedback_reset_post_doc
       responses:
         '200':
@@ -204,6 +264,7 @@
     post:
       tags:
       - Admin/Feedback
+      description: Record a feedback signal (kind/target/confidence/severity/note).
       operationId: feedback_signal_post_doc
       requestBody:
         content:
@@ -228,6 +289,7 @@
     get:
       tags:
       - Admin/Feedback
+      description: Return the current feedback engine state.
       operationId: feedback_state_get_doc
       responses:
         '200':
@@ -242,6 +304,7 @@
     get:
       tags:
       - Admin/Feedback
+      description: List current suggestions with rationale and confidence.
       operationId: feedback_suggestions_doc
       responses:
         '200':
@@ -256,6 +319,7 @@
     get:
       tags:
       - Admin/Feedback
+      description: Poll for suggestion/version updates since the given version.
       operationId: feedback_updates_doc
       parameters:
       - name: since
@@ -281,6 +345,8 @@
     get:
       tags:
       - Admin/Governor
+      description: Get current governor hints (concurrency, timeouts, retrieval, context,
+        etc.).
       operationId: governor_hints_get_doc
       responses:
         '200':
@@ -294,6 +360,7 @@
     post:
       tags:
       - Admin/Governor
+      description: Set governor hints (validated and persisted).
       operationId: governor_hints_set_doc
       requestBody:
         content:
@@ -318,6 +385,7 @@
     get:
       tags:
       - Admin/Governor
+      description: Get the active governor profile (performance/balanced/power-saver).
       operationId: governor_profile_get_doc
       responses:
         '200':
@@ -331,6 +399,7 @@
     post:
       tags:
       - Admin/Governor
+      description: Set the active governor profile.
       operationId: governor_profile_set_doc
       requestBody:
         content:
@@ -375,6 +444,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Hierarchy: POST /admin/hierarchy/accept.'
   /admin/hierarchy/hello:
     post:
       tags:
@@ -399,6 +469,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Hierarchy: POST /admin/hierarchy/hello.'
   /admin/hierarchy/offer:
     post:
       tags:
@@ -423,6 +494,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Hierarchy: POST /admin/hierarchy/offer.'
   /admin/hierarchy/role:
     post:
       tags:
@@ -447,6 +519,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Hierarchy: POST /admin/hierarchy/role.'
   /admin/hierarchy/state:
     get:
       tags:
@@ -461,11 +534,12 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Hierarchy: GET /admin/hierarchy/state.'
   /admin/introspect/schemas/{id}:
     get:
       tags:
       - Admin/Introspect
-      operationId: introspect_schema
+      operationId: introspect_schema_doc
       parameters:
       - name: id
         in: path
@@ -484,11 +558,12 @@
                 $ref: '#/components/schemas/ProblemDetails'
         '404':
           description: Unknown tool id
+      description: 'Introspect: GET /admin/introspect/schemas/{id}.'
   /admin/introspect/tools:
     get:
       tags:
       - Admin/Introspect
-      operationId: introspect_tools
+      operationId: introspect_tools_doc
       responses:
         '200':
           description: List available tools
@@ -498,6 +573,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Introspect: GET /admin/introspect/tools.'
   /admin/memory:
     get:
       tags:
@@ -512,6 +588,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Memory: GET /admin/memory.'
   /admin/memory/apply:
     post:
       tags:
@@ -542,6 +619,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Memory: POST /admin/memory/apply.'
   /admin/memory/limit:
     get:
       tags:
@@ -556,6 +634,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Memory: GET /admin/memory/limit.'
     post:
       tags:
       - Admin/Memory
@@ -579,6 +658,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Memory: POST /admin/memory/limit.'
   /admin/memory/load:
     post:
       tags:
@@ -599,6 +679,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Memory: POST /admin/memory/load.'
   /admin/memory/save:
     post:
       tags:
@@ -623,6 +704,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Memory: POST /admin/memory/save.'
   /admin/models:
     get:
       tags:
@@ -637,6 +719,20 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Models: GET /admin/models.'
+  /admin/models/summary:
+    get:
+      tags:
+      - Admin/Models
+      operationId: models_summary_doc
+      responses:
+        '200':
+          description: Models summary
+          content:
+            application/json:
+              schema:
+                $ref: '#/components/schemas/ModelsSummary'
+      description: 'Models: GET /admin/models/summary.'
   /admin/models/add:
     post:
       tags:
@@ -667,6 +763,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Models: POST /admin/models/add.'
   /admin/models/concurrency:
     get:
       tags:
@@ -681,6 +778,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Models: GET /admin/models/concurrency.'
     post:
       tags:
       - Admin/Models
@@ -704,6 +802,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Models: POST /admin/models/concurrency.'
   /admin/models/default:
     get:
       tags:
@@ -718,6 +817,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Models: GET /admin/models/default.'
     post:
       tags:
       - Admin/Models
@@ -741,6 +841,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Models: POST /admin/models/default.'
   /admin/models/delete:
     post:
       tags:
@@ -765,6 +866,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Models: POST /admin/models/delete.'
   /admin/models/download:
     post:
       tags:
@@ -801,6 +903,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Models: POST /admin/models/download.'
   /admin/models/download/cancel:
     post:
       tags:
@@ -825,6 +928,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Models: POST /admin/models/download/cancel.'
   /admin/models/jobs:
     get:
       tags:
@@ -839,6 +943,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Models: GET /admin/models/jobs.'
   /admin/models/load:
     post:
       tags:
@@ -857,6 +962,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Models: POST /admin/models/load.'
   /admin/models/refresh:
     post:
       tags:
@@ -875,6 +981,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Models: POST /admin/models/refresh.'
   /admin/models/save:
     post:
       tags:
@@ -893,11 +1000,12 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Models: POST /admin/models/save.'
   /admin/probe:
     get:
       tags:
       - Admin/Introspect
-      operationId: probe
+      operationId: probe_doc
       responses:
         '200':
           description: Returns effective memory paths
@@ -907,6 +1015,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Introspect: GET /admin/probe.'
   /admin/projects/create:
     post:
       tags:
@@ -937,6 +1046,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Projects: POST /admin/projects/create.'
   /admin/projects/list:
     get:
       tags:
@@ -951,6 +1061,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Projects: GET /admin/projects/list.'
   /admin/projects/notes:
     post:
       tags:
@@ -982,11 +1093,12 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Projects: POST /admin/projects/notes.'
   /admin/shutdown:
     get:
       tags:
       - Admin/Core
-      operationId: shutdown
+      operationId: shutdown_doc
       responses:
         '200':
           description: Shutdown service
@@ -1000,6 +1112,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Core: GET /admin/shutdown.'
   /admin/state/actions:
     get:
       tags:
@@ -1014,6 +1127,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'State: GET /admin/state/actions.'
   /admin/state/beliefs:
     get:
       tags:
@@ -1028,6 +1142,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'State: GET /admin/state/beliefs.'
   /admin/state/intents:
     get:
       tags:
@@ -1042,6 +1157,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'State: GET /admin/state/intents.'
   /admin/state/observations:
     get:
       tags:
@@ -1056,6 +1172,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'State: GET /admin/state/observations.'
   /admin/tasks/enqueue:
     post:
       tags:
@@ -1080,6 +1197,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Tasks: POST /admin/tasks/enqueue.'
   /admin/tools:
     get:
       tags:
@@ -1094,6 +1212,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Tools: GET /admin/tools.'
   /admin/tools/run:
     post:
       tags:
@@ -1114,10 +1233,12 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Tools: POST /admin/tools/run.'
   /healthz:
     get:
-      tags: []
-      operationId: healthz
+      tags:
+      - Public
+      operationId: healthz_doc
       responses:
         '200':
           description: Service health
@@ -1125,6 +1246,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/OkResponse'
+      description: GET /healthz.
   /metrics:
     get:
       tags:
@@ -1133,6 +1255,7 @@
       responses:
         '200':
           description: Prometheus metrics
+      description: 'Public: GET /metrics.'
   /spec:
     get:
       tags:
@@ -1141,6 +1264,27 @@
       responses:
         '200':
           description: Spec index
+      description: 'Specs: GET /spec.'
+  /catalog/index:
+    get:
+      tags:
+      - Public/Specs
+      operationId: catalog_index_doc
+      responses:
+        '200':
+          description: Interface catalog index (YAML)
+        '404':
+          description: Missing
+      description: 'Specs: GET /catalog/index.'
+  /catalog/health:
+    get:
+      tags:
+      - Public/Specs
+      operationId: catalog_health_doc
+      responses:
+        '200':
+          description: Catalog health
+      description: 'Specs: GET /catalog/health.'
   /spec/asyncapi.yaml:
     get:
       tags:
@@ -1151,6 +1295,7 @@
           description: AsyncAPI YAML
         '404':
           description: Missing
+      description: 'Specs: GET /spec/asyncapi.yaml.'
   /spec/mcp-tools.json:
     get:
       tags:
@@ -1161,6 +1306,7 @@
           description: MCP tools JSON
         '404':
           description: Missing
+      description: 'Specs: GET /spec/mcp-tools.json.'
   /spec/openapi.yaml:
     get:
       tags:
@@ -1171,6 +1317,7 @@
           description: OpenAPI YAML
         '404':
           description: Missing
+      description: 'Specs: GET /spec/openapi.yaml.'
   /version:
     get:
       tags:
@@ -1179,8 +1326,96 @@
       responses:
         '200':
           description: Service version
+      description: 'Public: GET /version.'
 components:
   schemas:
+    ModelsConcurrency:
+      type: object
+      required: [configured_max, available_permits, held_permits]
+      properties:
+        configured_max: { type: integer, format: int64, minimum: 0 }
+        available_permits: { type: integer, format: int64, minimum: 0 }
+        held_permits: { type: integer, format: int64, minimum: 0 }
+        hard_cap:
+          type: [integer, 'null']
+          format: int64
+          minimum: 0
+    ModelsMetrics:
+      type: object
+      required: [started, queued, admitted, resumed, canceled, completed, completed_cached, errors, bytes_total]
+      properties:
+        started: { type: integer, format: int64, minimum: 0 }
+        queued: { type: integer, format: int64, minimum: 0 }
+        admitted: { type: integer, format: int64, minimum: 0 }
+        resumed: { type: integer, format: int64, minimum: 0 }
+        canceled: { type: integer, format: int64, minimum: 0 }
+        completed: { type: integer, format: int64, minimum: 0 }
+        completed_cached: { type: integer, format: int64, minimum: 0 }
+        errors: { type: integer, format: int64, minimum: 0 }
+        bytes_total: { type: integer, format: int64, minimum: 0 }
+        ewma_mbps:
+          type: [number, 'null']
+          format: double
+          minimum: 0
+    ModelsSummary:
+      type: object
+      required: [items, concurrency, metrics]
+      properties:
+        items:
+          type: array
+          items:
+            $ref: '#/components/schemas/ModelItem'
+        default:
+          type: string
+        concurrency:
+          $ref: '#/components/schemas/ModelsConcurrency'
+        metrics:
+          $ref: '#/components/schemas/ModelsMetrics'
+    ModelItem:
+      type: object
+      required: [id]
+      properties:
+        id: { type: string }
+        provider: { type: [string, 'null'] }
+        path: { type: [string, 'null'] }
+        sha256: { type: [string, 'null'] }
+        bytes: { type: [integer, 'null'], format: int64, minimum: 0 }
+        status: { type: [string, 'null'] }
+        error_code: { type: [string, 'null'] }
+    About:
+      type: object
+      description: Service and branding information.
+      properties:
+        name:
+          type: string
+          example: Agent Hub (ARW)
+        tagline:
+          type: string
+          example: Your private AI control room that can scale and share when you
+            choose.
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
+      - service
+      - version
+      - endpoints
     ApplyMemory:
       type: object
       required:
@@ -1526,5 +1761,3 @@
         id:
           type: string
         input: {}
-tags:
-- name: arw-svc

```

## AsyncAPI (Events)

```diff
--- /tmp/tmphrg03xwl/asyncapi.base.yaml	2025-09-13 22:49:10.619772526 +0200
+++ /tmp/tmphrg03xwl/asyncapi.head.yaml	2025-09-13 22:49:10.619772526 +0200
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


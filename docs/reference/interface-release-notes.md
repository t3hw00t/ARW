---
title: Interface Release Notes
---

# Interface Release Notes

Base: `origin/main` vs Head: `4f323c7c`

## OpenAPI (REST)

```diff
--- /tmp/tmpu1u89pyu/openapi.base.yaml	2025-09-14 05:56:57.210740995 +0200
+++ /tmp/tmpu1u89pyu/openapi.head.yaml	2025-09-14 05:56:57.221211677 +0200
@@ -1,23 +1,106 @@
 openapi: 3.1.0
+jsonSchemaDialect: https://json-schema.org/draft/2020-12/schema
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
+- name: Public/State
+  description: Public read-models and state
 paths:
+  /models/blob/{sha256}:
+    get:
+      tags:
+      - Public
+      description: |
+        Return a CAS blob by SHA-256 digest. Strong validators are provided with
+        ETag (hex digest) and Last-Modified. Clients may send If-None-Match to
+        receive 304 Not Modified. Cache-Control is set to long-lived immutable.
+      operationId: models_blob_get_doc
+      parameters:
+      - name: sha256
+        in: path
+        description: Hex lowercase SHA-256 (64 chars)
+        required: true
+        schema:
+          type: string
+      responses:
+        '200':
+          description: Blob bytes
+          content:
+            application/octet-stream:
+              schema:
+                type: string
+                format: binary
+        '206':
+          description: Partial Content (Range)
+        '304':
+          description: Not Modified
+        '400':
+          description: Invalid sha256
+          content:
+            application/json:
+              schema:
+                $ref: '#/components/schemas/ProblemDetails'
+        '404':
+          description: Not found
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
@@ -32,6 +115,10 @@
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
@@ -50,6 +137,10 @@
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
@@ -74,7 +165,8 @@
     get:
       tags:
       - Admin/Core
-      operationId: emit_test
+      description: Emit a test event onto the internal event bus (for verification).
+      operationId: emit_test_doc
       responses:
         '200':
           description: Emit test event
@@ -92,7 +184,9 @@
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
@@ -106,6 +200,7 @@
     post:
       tags:
       - Admin/Feedback
+      description: Analyze recorded signals and stats to propose suggestions.
       operationId: feedback_analyze_post_doc
       responses:
         '200':
@@ -124,6 +219,7 @@
     post:
       tags:
       - Admin/Feedback
+      description: Apply a suggestion by id (policy-gated).
       operationId: feedback_apply_post_doc
       requestBody:
         content:
@@ -148,6 +244,7 @@
     post:
       tags:
       - Admin/Feedback
+      description: Enable or disable automatic application of accepted suggestions.
       operationId: feedback_auto_post_doc
       requestBody:
         content:
@@ -172,6 +269,7 @@
     get:
       tags:
       - Admin/Feedback
+      description: Return the effective feedback policy and tunables.
       operationId: feedback_policy_doc
       responses:
         '200':
@@ -186,6 +284,7 @@
     post:
       tags:
       - Admin/Feedback
+      description: Clear feedback signals and suggestions.
       operationId: feedback_reset_post_doc
       responses:
         '200':
@@ -204,6 +303,7 @@
     post:
       tags:
       - Admin/Feedback
+      description: Record a feedback signal (kind/target/confidence/severity/note).
       operationId: feedback_signal_post_doc
       requestBody:
         content:
@@ -228,6 +328,7 @@
     get:
       tags:
       - Admin/Feedback
+      description: Return the current feedback engine state.
       operationId: feedback_state_get_doc
       responses:
         '200':
@@ -242,6 +343,7 @@
     get:
       tags:
       - Admin/Feedback
+      description: List current suggestions with rationale and confidence.
       operationId: feedback_suggestions_doc
       responses:
         '200':
@@ -256,6 +358,7 @@
     get:
       tags:
       - Admin/Feedback
+      description: Poll for suggestion/version updates since the given version.
       operationId: feedback_updates_doc
       parameters:
       - name: since
@@ -281,6 +384,8 @@
     get:
       tags:
       - Admin/Governor
+      description: Get current governor hints (concurrency, timeouts, retrieval, context,
+        etc.).
       operationId: governor_hints_get_doc
       responses:
         '200':
@@ -294,6 +399,7 @@
     post:
       tags:
       - Admin/Governor
+      description: Set governor hints (validated and persisted).
       operationId: governor_hints_set_doc
       requestBody:
         content:
@@ -318,6 +424,7 @@
     get:
       tags:
       - Admin/Governor
+      description: Get the active governor profile (performance/balanced/power-saver).
       operationId: governor_profile_get_doc
       responses:
         '200':
@@ -331,6 +438,7 @@
     post:
       tags:
       - Admin/Governor
+      description: Set the active governor profile.
       operationId: governor_profile_set_doc
       requestBody:
         content:
@@ -375,6 +483,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Hierarchy: POST /admin/hierarchy/accept.'
   /admin/hierarchy/hello:
     post:
       tags:
@@ -399,6 +508,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Hierarchy: POST /admin/hierarchy/hello.'
   /admin/hierarchy/offer:
     post:
       tags:
@@ -423,6 +533,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Hierarchy: POST /admin/hierarchy/offer.'
   /admin/hierarchy/role:
     post:
       tags:
@@ -447,6 +558,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Hierarchy: POST /admin/hierarchy/role.'
   /admin/hierarchy/state:
     get:
       tags:
@@ -461,11 +573,12 @@
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
@@ -484,11 +597,12 @@
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
@@ -498,6 +612,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Introspect: GET /admin/introspect/tools.'
   /admin/memory:
     get:
       tags:
@@ -512,6 +627,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Memory: GET /admin/memory.'
   /admin/memory/apply:
     post:
       tags:
@@ -542,6 +658,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Memory: POST /admin/memory/apply.'
   /admin/memory/limit:
     get:
       tags:
@@ -556,6 +673,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Memory: GET /admin/memory/limit.'
     post:
       tags:
       - Admin/Memory
@@ -579,6 +697,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Memory: POST /admin/memory/limit.'
   /admin/memory/load:
     post:
       tags:
@@ -599,6 +718,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Memory: POST /admin/memory/load.'
   /admin/memory/save:
     post:
       tags:
@@ -623,6 +743,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Memory: POST /admin/memory/save.'
   /admin/models:
     get:
       tags:
@@ -631,12 +752,77 @@
       responses:
         '200':
           description: Models list
+          content:
+            application/json:
+              schema:
+                type: array
+                items:
+                  $ref: '#/components/schemas/ModelItem'
+              examples:
+                basic:
+                  summary: Two models
+                  value:
+                    - id: llama3:8b
+                      provider: ollama
+                      bytes: 5347737600
+                      status: ready
+                    - id: qwen2:7b
+                      provider: hf
+                      bytes: 4855592960
+                      status: downloading
         '403':
           description: Forbidden
           content:
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
+              examples:
+                basic:
+                  summary: Typical models summary
+                  value:
+                    items:
+                      - id: llama3:8b
+                        provider: ollama
+                        path: /models/llama3/8b.bin
+                        sha256: 0123abcd...
+                        bytes: 5347737600
+                        status: ready
+                      - id: qwen2:7b
+                        provider: hf
+                        path: /models/qwen2/7b.bin
+                        sha256: 89ab0123...
+                        bytes: 4855592960
+                        status: downloading
+                    concurrency:
+                      configured_max: 2
+                      available_permits: 2
+                      held_permits: 0
+                      hard_cap: null
+                    metrics:
+                      started: 4
+                      queued: 1
+                      admitted: 3
+                      resumed: 0
+                      canceled: 0
+                      completed: 2
+                      completed_cached: 0
+                      errors: 0
+                      bytes_total: 10245591040
+                      ewma_mbps: 18.2
+      description: 'Models: GET /admin/models/summary.'
   /admin/models/add:
     post:
       tags:
@@ -667,6 +853,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Models: POST /admin/models/add.'
   /admin/models/concurrency:
     get:
       tags:
@@ -675,12 +862,24 @@
       responses:
         '200':
           description: Concurrency settings
+          content:
+            application/json:
+              schema:
+                $ref: '#/components/schemas/ModelsConcurrency'
+              examples:
+                typical:
+                  value:
+                    configured_max: 2
+                    available_permits: 2
+                    held_permits: 0
+                    hard_cap: null
         '403':
           description: Forbidden
           content:
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Models: GET /admin/models/concurrency.'
     post:
       tags:
       - Admin/Models
@@ -704,6 +903,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Models: POST /admin/models/concurrency.'
   /admin/models/default:
     get:
       tags:
@@ -718,6 +918,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Models: GET /admin/models/default.'
     post:
       tags:
       - Admin/Models
@@ -741,6 +942,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Models: POST /admin/models/default.'
   /admin/models/delete:
     post:
       tags:
@@ -765,6 +967,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Models: POST /admin/models/delete.'
   /admin/models/download:
     post:
       tags:
@@ -801,6 +1004,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Models: POST /admin/models/download.'
   /admin/models/download/cancel:
     post:
       tags:
@@ -825,6 +1029,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Models: POST /admin/models/download/cancel.'
   /admin/models/jobs:
     get:
       tags:
@@ -839,6 +1044,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Models: GET /admin/models/jobs.'
   /admin/models/load:
     post:
       tags:
@@ -857,6 +1063,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Models: POST /admin/models/load.'
   /admin/models/refresh:
     post:
       tags:
@@ -875,6 +1082,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Models: POST /admin/models/refresh.'
   /admin/models/save:
     post:
       tags:
@@ -893,11 +1101,12 @@
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
@@ -907,6 +1116,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Introspect: GET /admin/probe.'
   /admin/projects/create:
     post:
       tags:
@@ -937,6 +1147,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Projects: POST /admin/projects/create.'
   /admin/projects/list:
     get:
       tags:
@@ -951,6 +1162,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Projects: GET /admin/projects/list.'
   /admin/projects/notes:
     post:
       tags:
@@ -982,11 +1194,12 @@
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
@@ -1000,6 +1213,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Core: GET /admin/shutdown.'
   /admin/state/actions:
     get:
       tags:
@@ -1014,6 +1228,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'State: GET /admin/state/actions.'
   /admin/state/beliefs:
     get:
       tags:
@@ -1028,6 +1243,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'State: GET /admin/state/beliefs.'
   /admin/state/intents:
     get:
       tags:
@@ -1042,6 +1258,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'State: GET /admin/state/intents.'
   /admin/state/observations:
     get:
       tags:
@@ -1056,6 +1273,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'State: GET /admin/state/observations.'
   /admin/tasks/enqueue:
     post:
       tags:
@@ -1080,6 +1298,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Tasks: POST /admin/tasks/enqueue.'
   /admin/tools:
     get:
       tags:
@@ -1094,6 +1313,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
+      description: 'Tools: GET /admin/tools.'
   /admin/tools/run:
     post:
       tags:
@@ -1114,10 +1334,12 @@
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
@@ -1125,6 +1347,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/OkResponse'
+      description: GET /healthz.
   /metrics:
     get:
       tags:
@@ -1133,6 +1356,7 @@
       responses:
         '200':
           description: Prometheus metrics
+      description: 'Public: GET /metrics.'
   /spec:
     get:
       tags:
@@ -1141,6 +1365,27 @@
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
@@ -1151,6 +1396,7 @@
           description: AsyncAPI YAML
         '404':
           description: Missing
+      description: 'Specs: GET /spec/asyncapi.yaml.'
   /spec/mcp-tools.json:
     get:
       tags:
@@ -1161,6 +1407,7 @@
           description: MCP tools JSON
         '404':
           description: Missing
+      description: 'Specs: GET /spec/mcp-tools.json.'
   /spec/openapi.yaml:
     get:
       tags:
@@ -1171,6 +1418,7 @@
           description: OpenAPI YAML
         '404':
           description: Missing
+      description: 'Specs: GET /spec/openapi.yaml.'
   /version:
     get:
       tags:
@@ -1179,8 +1427,147 @@
       responses:
         '200':
           description: Service version
+      description: 'Public: GET /version.'
+  /state/models:
+    get:
+      description: Public read-only models list
+      tags:
+      - Public/State
+      operationId: state_models_doc
+      responses:
+        '200':
+          description: Models list
+          content:
+            application/json:
+              schema:
+                type: array
+                items:
+                  $ref: '#/components/schemas/ModelItem'
+              examples:
+                basic:
+                  value:
+                    - id: llama3:8b
+                      provider: ollama
+                      bytes: 5347737600
+                      status: ready
+                    - id: qwen2:7b
+                      provider: hf
+                      bytes: 4855592960
+                      status: downloading
+  # (no additional entries)
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
+      example:
+        configured_max: 2
+        available_permits: 2
+        held_permits: 0
+        hard_cap: null
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
+      example:
+        started: 4
+        queued: 1
+        admitted: 3
+        resumed: 0
+        canceled: 0
+        completed: 2
+        completed_cached: 0
+        errors: 0
+        bytes_total: 10245591040
+        ewma_mbps: 18.2
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
+      # example omitted due to tooling quirks with a property named "default"
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
+      example:
+        id: llama3:8b
+        provider: ollama
+        path: /models/llama3/8b.bin
+        sha256: 0123abcd...
+        bytes: 5347737600
+        status: ready
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
@@ -1526,5 +1913,4 @@
         id:
           type: string
         input: {}
-tags:
-- name: arw-svc
+  #

```

## AsyncAPI (Events)

```diff
--- /tmp/tmpu1u89pyu/asyncapi.base.yaml	2025-09-14 05:56:58.037924859 +0200
+++ /tmp/tmpu1u89pyu/asyncapi.head.yaml	2025-09-14 05:56:58.037924859 +0200
@@ -2,110 +2,158 @@
 info:
   title: "arw-svc events"
   version: "0.1.0"
+  description: "Normalized dot.case event channels for arw-svc."
+  license:
+    name: "MIT OR Apache-2.0"
+  contact:
+    name: "ARW"
+    url: "https://github.com/t3hw00t/ARW"
+    email: "noreply@example.com"
 defaultContentType: application/json
+tags:
+  - name: CloudEvents
+    description: Events include CloudEvents 1.0 metadata under `ce`.
 channels:
-  Service.Start:
+  service.start:
     subscribe:
+      operationId: service_start
+      description: Service emitted start event
       message:
         $ref: '#/components/messages/ServiceStart'
-  Service.Health:
+  service.health:
     subscribe:
+      operationId: service_health
+      description: Periodic health heartbeat
       message:
         $ref: '#/components/messages/ServiceHealth'
-  Service.Test:
+  service.test:
     subscribe:
+      operationId: service_test
+      description: Test event emission
       message:
         $ref: '#/components/messages/ServiceTest'
-  Governor.Changed:
+  governor.changed:
     subscribe:
+      operationId: governor_changed
+      description: Governor profile changed
       message:
         $ref: '#/components/messages/GovernorChanged'
-  Memory.Applied:
+  memory.applied:
     subscribe:
+      operationId: memory_applied
+      description: Memory applied to working set
       message:
         $ref: '#/components/messages/MemoryApplied'
-  Models.Changed:
+  models.changed:
     subscribe:
+      operationId: models_changed
+      description: Models list/default changed
       message:
         $ref: '#/components/messages/ModelsChanged'
-  Models.DownloadProgress:
+  models.download.progress:
     subscribe:
+      operationId: models_download_progress
+      description: Download progress, status codes, metrics snapshots
       message:
         $ref: '#/components/messages/ModelsDownloadProgress'
-  Models.ManifestWritten:
+  models.manifest.written:
     subscribe:
+      operationId: models_manifest_written
+      description: A model manifest has been written
       message:
         $ref: '#/components/messages/ModelsManifestWritten'
-  Models.CasGc:
+  models.cas.gc:
     subscribe:
+      operationId: models_cas_gc
+      description: CAS GC run summary
       message:
         $ref: '#/components/messages/ModelsCasGc'
-  Egress.Preview:
+  egress.preview:
     subscribe:
+      operationId: egress_preview
+      description: Egress preflight summary
       message:
         $ref: '#/components/messages/EgressPreview'
-  Egress.Ledger.Appended:
+  egress.ledger.appended:
     subscribe:
+      operationId: egress_ledger_appended
+      description: Egress decision appended to ledger
       message:
         $ref: '#/components/messages/EgressLedgerAppended'
-  Tool.Ran:
+  tool.ran:
     subscribe:
+      operationId: tool_ran
+      description: Tool execution completed
       message:
         $ref: '#/components/messages/ToolRan'
-  Feedback.Signal:
+  feedback.signal:
     subscribe:
+      operationId: feedback_signal
+      description: Feedback signal recorded
       message:
         $ref: '#/components/messages/FeedbackSignal'
-  Feedback.Suggested:
+  feedback.suggested:
     subscribe:
+      operationId: feedback_suggested
+      description: Feedback suggestion produced
       message:
         $ref: '#/components/messages/FeedbackSuggested'
-  Feedback.Applied:
+  feedback.applied:
     subscribe:
+      operationId: feedback_applied
+      description: Feedback suggestion applied
       message:
         $ref: '#/components/messages/FeedbackApplied'
 components:
   messages:
     ServiceStart:
-      name: Service.Start
+      name: service.start
       payload:
         type: object
         additionalProperties: true
     ServiceHealth:
-      name: Service.Health
+      name: service.health
       payload:
         type: object
         properties:
           ok: { type: boolean }
     ServiceTest:
-      name: Service.Test
+      name: service.test
       payload:
         type: object
         additionalProperties: true
     GovernorChanged:
-      name: Governor.Changed
+      name: governor.changed
       payload:
         type: object
         properties:
           profile: { type: string }
     MemoryApplied:
-      name: Memory.Applied
+      name: memory.applied
       payload:
         type: object
         additionalProperties: true
     ModelsChanged:
-      name: Models.Changed
+      name: models.changed
       payload:
         type: object
         additionalProperties: true
     ModelsDownloadProgress:
-      name: Models.DownloadProgress
+      name: models.download.progress
+      correlationId:
+        description: Correlates download episodes and related events
+        location: "$message.payload#/corr_id"
       payload:
         type: object
         properties:
           id: { type: string }
-          status: { type: string }
-          code: { type: string }
+          corr_id: { type: string }
+          status:
+            type: string
+            enum: [started, queued, admitted, downloading, resumed, resync, degraded, canceled, complete, cancel-requested, no-active-job, cache-mismatch]
+          code:
+            type: string
+            enum: [started, queued, admitted, downloading, progress, resumed, resync, degraded, canceled-by-user, complete, cached, already-in-progress, already-in-progress-hash, cache-mismatch, soft-exhausted, cancel-requested, no-active-job, request-failed, concurrency-closed, downstream-http-status, upstream-changed, resume-no-content-range, resume-http-status, resume-failed, resync-failed, quota-exceeded, size-limit, idle-timeout, hard-exhausted, io-read, io-write, flush-failed, mkdir-failed, open-failed, create-failed, verify-open-failed, verify-read-failed, checksum-mismatch, size-mismatch, finalize-failed, admission-denied, disk-insufficient, size-limit-stream, disk-insufficient-stream]
           error: { type: string }
           progress: { type: integer }
           downloaded: { type: integer }
@@ -121,15 +169,18 @@
               spent_ms: { type: integer }
               remaining_soft_ms: { type: integer }
               remaining_hard_ms: { type: integer }
+              class: { type: string, enum: [interactive, batch] }
+            additionalProperties: false
           disk:
             type: object
             properties:
               available: { type: integer }
               total: { type: integer }
               reserve: { type: integer }
+            additionalProperties: false
         additionalProperties: true
     ModelsManifestWritten:
-      name: Models.ManifestWritten
+      name: models.manifest.written
       payload:
         type: object
         properties:
@@ -137,8 +188,10 @@
           manifest_path: { type: string }
           sha256: { type: ["string","null"] }
           cas: { type: ["string","null"] }
+          corr_id: { type: string }
+        additionalProperties: false
     ModelsCasGc:
-      name: Models.CasGc
+      name: models.cas.gc
       payload:
         type: object
         properties:
@@ -147,8 +200,10 @@
           deleted: { type: integer }
           deleted_bytes: { type: integer }
           ttl_days: { type: integer }
+          corr_id: { type: string }
+        additionalProperties: false
     EgressPreview:
-      name: Egress.Preview
+      name: egress.preview
       payload:
         type: object
         properties:
@@ -164,7 +219,7 @@
           corr_id: { type: string }
         additionalProperties: true
     EgressLedgerAppended:
-      name: Egress.Ledger.Appended
+      name: egress.ledger.appended
       payload:
         type: object
         properties:
@@ -186,14 +241,14 @@
           bytes_in: { type: integer }
           duration_ms: { type: integer }
     ToolRan:
-      name: Tool.Ran
+      name: tool.ran
       payload:
         type: object
         properties:
           id: { type: string }
           output: { type: object }
     FeedbackSignal:
-      name: Feedback.Signal
+      name: feedback.signal
       payload:
         type: object
         properties:
@@ -206,7 +261,7 @@
               confidence: { type: number }
               severity: { type: integer }
     FeedbackSuggested:
-      name: Feedback.Suggested
+      name: feedback.suggested
       payload:
         type: object
         properties:
@@ -222,7 +277,7 @@
                 rationale: { type: string }
                 confidence: { type: number }
     FeedbackApplied:
-      name: Feedback.Applied
+      name: feedback.applied
       payload:
         type: object
         properties:

```


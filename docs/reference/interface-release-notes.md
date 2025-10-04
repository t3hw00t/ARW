---
title: Interface Release Notes
---

# Interface Release Notes
Updated: 2025-10-02
Type: Reference

Base: `origin/main`

## OpenAPI (REST)

```diff
--- openapi.base.yaml
+++ openapi.head.yaml
@@ -11,9 +11,8 @@
     get:
       tags:
       - Meta
-      summary: Inspect service metadata
-      description: Return version, build information, endpoint counts, and the enumerated
-        public/admin routes announced by the server.
+      summary: Service metadata and endpoints index.
+      description: Service metadata, endpoints index, and performance preset.
       operationId: about_doc
       responses:
         '200':
@@ -40,9 +39,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
-      description: Retrieve the active admin chat transcript, including message history
-        and tool metadata.
-      summary: Fetch chat transcript
+      description: 'Chat: GET /admin/chat.'
   /admin/chat/clear:
     post:
       tags:
@@ -60,9 +57,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
-      description: Wipe the stored admin chat transcript and reset associated tool
-        context for a fresh session.
-      summary: Clear chat transcript
+      description: 'Chat: POST /admin/chat/clear.'
   /admin/chat/send:
     post:
       tags:
@@ -87,9 +82,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
-      description: Submit a message to the admin chat lane and receive the synthesized
-        assistant response.
-      summary: Send chat message
+      description: 'Chat: POST /admin/chat/send.'
   /admin/chat/status:
     get:
       tags:
@@ -115,14 +108,12 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
-      description: Report chat lane health, including the most recent latency probe;
-        optionally trigger a new probe.
-      summary: Query chat status
+      description: 'Chat: GET /admin/chat/status.'
   /admin/distill:
     post:
       tags:
       - Distill
-      summary: Run distillation pass
+      summary: Trigger a manual distillation pass.
       operationId: distill_run_doc
       responses:
         '200':
@@ -136,8 +127,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
-      description: Trigger the manual distillation pipeline to snapshot playbooks,
-        refresh beliefs, and regenerate derived notebooks.
+      description: Trigger a manual distillation pass.
   /admin/events/journal:
     get:
       tags:
@@ -204,9 +194,7 @@
           description: Unauthorized
         '404':
           description: Unknown experiment
-      description: Mark a variant as the active choice for the experiment and persist
-        the rollout decision.
-      summary: Activate experiment variant
+      description: 'Experiments: POST /admin/experiments/activate.'
   /admin/experiments/assign:
     post:
       tags:
@@ -226,9 +214,7 @@
               schema: {}
         '401':
           description: Unauthorized
-      description: Record or override an experiment assignment for a participant or
-        agent and broadcast the decision.
-      summary: Assign experiment variant
+      description: 'Experiments: POST /admin/experiments/assign.'
   /admin/experiments/define:
     post:
       tags:
@@ -248,9 +234,7 @@
               schema: {}
         '401':
           description: Unauthorized
-      description: Create or update an experiment definition with the supplied variants
-        and configuration payload.
-      summary: Define experiment
+      description: 'Experiments: POST /admin/experiments/define.'
   /admin/experiments/list:
     get:
       tags:
@@ -265,9 +249,7 @@
                 $ref: '#/components/schemas/ExperimentsListResponse'
         '401':
           description: Unauthorized
-      description: Return all experiment definitions currently registered with their
-        variant metadata.
-      summary: List experiments
+      description: 'Experiments: GET /admin/experiments/list.'
   /admin/experiments/run:
     post:
       tags:
@@ -288,9 +270,7 @@
                 $ref: '#/components/schemas/RunOutcome'
         '401':
           description: Unauthorized
-      description: Execute the requested experiment variants against the chosen golden
-        project and return the evaluation outcome.
-      summary: Run experiment on goldens
+      description: 'Experiments: POST /admin/experiments/run.'
   /admin/experiments/scoreboard:
     get:
       tags:
@@ -305,9 +285,7 @@
                 $ref: '#/components/schemas/ExperimentsScoreboardResponse'
         '401':
           description: Unauthorized
-      description: Provide aggregated performance metrics for each experiment variant
-        to compare recent runs.
-      summary: Fetch experiment scoreboard
+      description: 'Experiments: GET /admin/experiments/scoreboard.'
   /admin/experiments/start:
     post:
       tags:
@@ -327,9 +305,7 @@
               schema: {}
         '401':
           description: Unauthorized
-      description: Publish a start event for a new experiment with optional assignment,
-        budget, and variant hints.
-      summary: Start experiment run
+      description: 'Experiments: POST /admin/experiments/start.'
   /admin/experiments/stop:
     post:
       tags:
@@ -349,9 +325,7 @@
               schema: {}
         '401':
           description: Unauthorized
-      description: Halt an experiment run by emitting a stop event for the provided
-        experiment identifier.
-      summary: Stop experiment
+      description: 'Experiments: POST /admin/experiments/stop.'
   /admin/experiments/winners:
     get:
       tags:
@@ -366,9 +340,7 @@
                 $ref: '#/components/schemas/ExperimentsWinnersResponse'
         '401':
           description: Unauthorized
-      description: Return the top-performing variants for experiments based on the
-        latest evaluation data.
-      summary: List experiment winners
+      description: 'Experiments: GET /admin/experiments/winners.'
   /admin/feedback/analyze:
     post:
       tags:
@@ -383,9 +355,7 @@
                 $ref: '#/components/schemas/FeedbackState'
         '401':
           description: Unauthorized
-      description: Trigger an immediate feedback analysis pass to refresh suggestions
-        from the latest signals.
-      summary: Recompute feedback suggestions
+      description: 'Feedback: POST /admin/feedback/analyze.'
   /admin/feedback/apply:
     post:
       tags:
@@ -411,9 +381,7 @@
           description: Policy denied
         '404':
           description: Unknown suggestion
-      description: Apply the identified suggestion and let the engine reconcile policy
-        and audit outcomes.
-      summary: Apply feedback suggestion
+      description: 'Feedback: POST /admin/feedback/apply.'
   /admin/feedback/auto:
     post:
       tags:
@@ -433,8 +401,7 @@
               schema: {}
         '401':
           description: Unauthorized
-      description: Enable or disable automatic application of approved feedback suggestions.
-      summary: Toggle automatic feedback application
+      description: 'Feedback: POST /admin/feedback/auto.'
   /admin/feedback/policy:
     get:
       tags:
@@ -448,9 +415,7 @@
               schema: {}
         '401':
           description: Unauthorized
-      description: Return the effective feedback application policy after merging
-        defaults and overrides.
-      summary: Fetch feedback policy
+      description: 'Feedback: GET /admin/feedback/policy.'
   /admin/feedback/reset:
     post:
       tags:
@@ -464,9 +429,7 @@
               schema: {}
         '401':
           description: Unauthorized
-      description: Clear feedback signals, suggestions, and cached state for a cold
-        start.
-      summary: Reset feedback engine
+      description: 'Feedback: POST /admin/feedback/reset.'
   /admin/feedback/rollback:
     post:
       tags:
@@ -490,9 +453,7 @@
           description: Unauthorized
         '404':
           description: Snapshot not found
-      description: Restore feedback state to the requested snapshot version and return
-        the resulting suggestion set.
-      summary: Roll back feedback snapshot
+      description: 'Feedback: POST /admin/feedback/rollback.'
   /admin/feedback/signal:
     post:
       tags:
@@ -513,9 +474,7 @@
                 $ref: '#/components/schemas/FeedbackState'
         '401':
           description: Unauthorized
-      description: Submit a feedback signal with confidence and severity so it influences
-        subsequent analysis.
-      summary: Record feedback signal
+      description: 'Feedback: POST /admin/feedback/signal.'
   /admin/feedback/state:
     get:
       tags:
@@ -530,9 +489,7 @@
                 $ref: '#/components/schemas/FeedbackState'
         '401':
           description: Unauthorized
-      description: Return the current feedback engine snapshot, including signals,
-        suggestions, and configuration.
-      summary: Inspect feedback state
+      description: 'Feedback: GET /admin/feedback/state.'
   /admin/feedback/suggestions:
     get:
       tags:
@@ -546,9 +503,7 @@
               schema: {}
         '401':
           description: Unauthorized
-      description: Return the current queue of actionable feedback suggestions with
-        their metadata.
-      summary: List feedback suggestions
+      description: 'Feedback: GET /admin/feedback/suggestions.'
   /admin/feedback/updates:
     get:
       tags:
@@ -572,9 +527,7 @@
           description: No changes since provided version
         '401':
           description: Unauthorized
-      description: Retrieve feedback suggestions updated since a provided version
-        cursor, enabling incremental refresh.
-      summary: Fetch feedback updates
+      description: 'Feedback: GET /admin/feedback/updates.'
   /admin/feedback/versions:
     get:
       tags:
@@ -588,9 +541,7 @@
               schema: {}
         '401':
           description: Unauthorized
-      description: Enumerate available feedback snapshots that can be inspected or
-        rolled back to.
-      summary: List feedback snapshots
+      description: 'Feedback: GET /admin/feedback/versions.'
   /admin/goldens/add:
     post:
       tags:
@@ -612,9 +563,7 @@
           description: Persist failed
         '401':
           description: Unauthorized
-      description: Append a golden item to the specified project collection and persist
-        the updated set.
-      summary: Add golden record
+      description: 'Experiments: POST /admin/goldens/add.'
   /admin/goldens/list:
     get:
       tags:
@@ -635,9 +584,7 @@
                 $ref: '#/components/schemas/GoldensListResponse'
         '401':
           description: Unauthorized
-      description: Return the golden dataset for the requested project, including
-        individual test cases.
-      summary: List golden records
+      description: 'Experiments: GET /admin/goldens/list.'
   /admin/goldens/run:
     post:
       tags:
@@ -658,9 +605,7 @@
                 $ref: '#/components/schemas/EvalSummary'
         '401':
           description: Unauthorized
-      description: Run the supplied golden dataset against the current chat runtime
-        and report evaluation metrics.
-      summary: Evaluate golden set
+      description: 'Experiments: POST /admin/goldens/run.'
   /admin/governor/hints:
     get:
       tags:
@@ -675,9 +620,7 @@
                 $ref: '#/components/schemas/Hints'
         '401':
           description: Unauthorized
-      description: Return the currently effective governor hints that shape scheduling
-        and retrieval behaviour.
-      summary: Inspect governor hints
+      description: 'Introspect: GET /admin/governor/hints.'
     post:
       tags:
       - Admin/Introspect
@@ -696,9 +639,7 @@
               schema: {}
         '401':
           description: Unauthorized
-      description: Apply new governor hints to adjust scheduling, retrieval, and context
-        construction parameters.
-      summary: Update governor hints
+      description: 'Introspect: POST /admin/governor/hints.'
   /admin/governor/profile:
     get:
       tags:
@@ -713,8 +654,7 @@
                 $ref: '#/components/schemas/GovernorProfileResponse'
         '401':
           description: Unauthorized
-      description: Return the active governor profile name configured for the node.
-      summary: Get governor profile
+      description: 'Introspect: GET /admin/governor/profile.'
     post:
       tags:
       - Admin/Introspect
@@ -733,9 +673,7 @@
               schema: {}
         '401':
           description: Unauthorized
-      description: Switch the governor to the requested profile and broadcast the
-        change.
-      summary: Set governor profile
+      description: 'Introspect: POST /admin/governor/profile.'
   /admin/hierarchy/accept:
     post:
       tags:
@@ -755,9 +693,7 @@
               schema: {}
         '401':
           description: Unauthorized
-      description: Accept a hierarchy offer message to finalize a connection with
-        the given participant.
-      summary: Accept hierarchy offer
+      description: 'Hierarchy: POST /admin/hierarchy/accept.'
   /admin/hierarchy/hello:
     post:
       tags:
@@ -777,9 +713,7 @@
               schema: {}
         '401':
           description: Unauthorized
-      description: Emit the initial hello message in the hierarchy handshake with
-        another participant.
-      summary: Send hierarchy hello
+      description: 'Hierarchy: POST /admin/hierarchy/hello.'
   /admin/hierarchy/offer:
     post:
       tags:
@@ -799,9 +733,7 @@
               schema: {}
         '401':
           description: Unauthorized
-      description: Publish a hierarchy offer to negotiate roles and capabilities with
-        a peer.
-      summary: Offer hierarchy connection
+      description: 'Hierarchy: POST /admin/hierarchy/offer.'
   /admin/hierarchy/role:
     post:
       tags:
@@ -821,9 +753,7 @@
               schema: {}
         '401':
           description: Unauthorized
-      description: Update hierarchy role assignments for a participant and persist
-        the change.
-      summary: Set hierarchy roles
+      description: 'Hierarchy: POST /admin/hierarchy/role.'
   /admin/hierarchy/state:
     get:
       tags:
@@ -837,14 +767,12 @@
               schema: {}
         '401':
           description: Unauthorized
-      description: Return the current hierarchy session map, including offers, participants,
-        and active roles.
-      summary: Inspect hierarchy state
+      description: 'Hierarchy: GET /admin/hierarchy/state.'
   /admin/memory:
     get:
       tags:
       - Admin/Memory
-      summary: List recent memory records
+      summary: List recent memory items (admin helper).
       operationId: admin_memory_list_doc
       parameters:
       - name: lane
@@ -876,13 +804,12 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
-      description: Return the latest memory items for inspection; supports lane and
-        limit filters for debugging.
+      description: List recent memory items (admin helper).
   /admin/memory/apply:
     post:
       tags:
       - Admin/Memory
-      summary: Upsert memory record
+      summary: Insert a memory item (admin helper).
       operationId: admin_memory_apply_doc
       requestBody:
         content:
@@ -908,8 +835,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
-      description: Insert or update a memory item via the admin helper and emit the
-        associated memory events for auditing.
+      description: Insert a memory item (admin helper).
   /admin/memory/quarantine:
     get:
       tags:
@@ -923,8 +849,7 @@
               schema: {}
         '401':
           description: Unauthorized
-      description: Return entries awaiting review in the memory quarantine queue.
-      summary: List quarantined memories
+      description: 'Review: GET /admin/memory/quarantine.'
     post:
       tags:
       - Review
@@ -945,9 +870,7 @@
           description: Unauthorized
         '500':
           description: Storage error
-      description: Enqueue a memory item for quarantine review and emit the appropriate
-        audit event.
-      summary: Queue memory for review
+      description: 'Review: POST /admin/memory/quarantine.'
   /admin/memory/quarantine/admit:
     post:
       tags:
@@ -969,9 +892,7 @@
           description: Unauthorized
         '500':
           description: Storage error
-      description: Remove a memory from quarantine, optionally admitting or rejecting
-        it, and report the outcome.
-      summary: Admit quarantined memory
+      description: 'Review: POST /admin/memory/quarantine/admit.'
   /admin/models:
     get:
       tags:
@@ -987,8 +908,7 @@
                 items: {}
         '401':
           description: Unauthorized
-      description: Return the configured model entries including provider metadata.
-      summary: List models
+      description: 'Models: GET /admin/models.'
   /admin/models/add:
     post:
       tags:
@@ -1008,9 +928,7 @@
               schema: {}
         '400':
           description: Invalid input
-      description: Register a model entry with optional provider, path, and status
-        metadata.
-      summary: Add model entry
+      description: 'Models: POST /admin/models/add.'
   /admin/models/by-hash/{sha256}:
     get:
       tags:
@@ -1047,9 +965,7 @@
           content:
             application/json:
               schema: {}
-      description: Stream a CAS-stored model blob by SHA-256 with caching headers
-        and partial range support.
-      summary: Download model blob
+      description: 'Models: GET /admin/models/by-hash/{sha256}.'
   /admin/models/cas_gc:
     post:
       tags:
@@ -1069,9 +985,7 @@
               schema: {}
         '501':
           description: CAS GC unavailable
-      description: Execute a content-addressed store cleanup pass and report removed
-        blobs.
-      summary: Run CAS garbage collection
+      description: 'Models: POST /admin/models/cas_gc.'
   /admin/models/concurrency:
     get:
       tags:
@@ -1084,8 +998,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ModelsConcurrencySnapshot'
-      description: Return the current model concurrency settings and snapshot telemetry.
-      summary: Inspect model concurrency
+      description: 'Models: GET /admin/models/concurrency.'
     post:
       tags:
       - Models
@@ -1103,8 +1016,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ModelsConcurrencySnapshot'
-      description: Apply new concurrency limits or blocking behaviour for model execution.
-      summary: Update model concurrency
+      description: 'Models: POST /admin/models/concurrency.'
   /admin/models/default:
     get:
       tags:
@@ -1116,8 +1028,7 @@
           content:
             application/json:
               schema: {}
-      description: Return the identifier of the default model selection.
-      summary: Get default model
+      description: 'Models: GET /admin/models/default.'
     post:
       tags:
       - Models
@@ -1136,8 +1047,7 @@
               schema: {}
         '400':
           description: Unknown model
-      description: Select the default model to be used for future requests.
-      summary: Set default model
+      description: 'Models: POST /admin/models/default.'
   /admin/models/download:
     post:
       tags:
@@ -1157,9 +1067,7 @@
               schema: {}
         '501':
           description: Download unavailable
-      description: Request download or import of a model artifact and enqueue the
-        job if supported.
-      summary: Start model download
+      description: 'Models: POST /admin/models/download.'
   /admin/models/download/cancel:
     post:
       tags:
@@ -1179,9 +1087,7 @@
               schema: {}
         '501':
           description: Cancel unavailable
-      description: Cancel an in-flight model download job when the backend supports
-        it.
-      summary: Cancel model download
+      description: 'Models: POST /admin/models/download/cancel.'
   /admin/models/jobs:
     get:
       tags:
@@ -1193,9 +1099,7 @@
           content:
             application/json:
               schema: {}
-      description: Return the current queue of model download and load jobs with their
-        statuses.
-      summary: Inspect model jobs
+      description: 'Models: GET /admin/models/jobs.'
   /admin/models/load:
     post:
       tags:
@@ -1211,8 +1115,7 @@
                 items: {}
         '404':
           description: Missing models.json
-      description: Load model entries from the persisted manifest on disk.
-      summary: Load model manifest
+      description: 'Models: POST /admin/models/load.'
   /admin/models/refresh:
     post:
       tags:
@@ -1226,9 +1129,7 @@
               schema:
                 type: array
                 items: {}
-      description: Refresh the live model list from runtime state and return the updated
-        entries.
-      summary: Refresh models list
+      description: 'Models: POST /admin/models/refresh.'
   /admin/models/remove:
     post:
       tags:
@@ -1246,8 +1147,7 @@
           content:
             application/json:
               schema: {}
-      description: Remove a model entry by identifier and report whether it existed.
-      summary: Remove model entry
+      description: 'Models: POST /admin/models/remove.'
   /admin/models/save:
     post:
       tags:
@@ -1259,8 +1159,7 @@
           content:
             application/json:
               schema: {}
-      description: Persist the current model registry to the on-disk manifest.
-      summary: Save model manifest
+      description: 'Models: POST /admin/models/save.'
   /admin/models/summary:
     get:
       tags:
@@ -1274,14 +1173,12 @@
               schema: {}
         '401':
           description: Unauthorized
-      description: Return aggregate statistics about installed models, storage usage,
-        and capabilities.
-      summary: Summarize model catalog
+      description: 'Models: GET /admin/models/summary.'
   /admin/probe:
     get:
       tags:
       - Admin/Introspect
-      summary: Inspect effective paths
+      summary: Effective path probe (successor to `/admin/probe`).
       operationId: probe_effective_paths_doc
       responses:
         '200':
@@ -1291,13 +1188,12 @@
               schema: {}
         '401':
           description: Unauthorized
-      description: Return the resolved state, cache, and config directories plus runtime
-        metadata so operators can confirm filesystem layout.
+      description: Effective path probe (successor to `/admin/probe`).
   /admin/probe/hw:
     get:
       tags:
       - Admin/Introspect
-      summary: Probe runtime hardware
+      summary: Hardware/software probe (`/admin/probe/hw`).
       operationId: probe_hw_doc
       responses:
         '200':
@@ -1307,13 +1203,12 @@
               schema: {}
         '401':
           description: Unauthorized
-      description: "Report detected hardware and OS capabilities\u2014including CPU,\
-        \ GPU, and accelerators\u2014to confirm what the node can access."
+      description: Hardware/software probe (`/admin/probe/hw`).
   /admin/probe/metrics:
     get:
       tags:
       - Admin/Introspect
-      summary: Probe metrics snapshot
+      summary: Metrics snapshot probe (`/admin/probe/metrics`).
       operationId: probe_metrics_doc
       responses:
         '200':
@@ -1323,8 +1218,7 @@
               schema: {}
         '401':
           description: Unauthorized
-      description: Return the current metrics summary (Prometheus-style counters and
-        histograms) for quick diagnostics.
+      description: Metrics snapshot probe (`/admin/probe/metrics`).
   /admin/self_model/apply:
     post:
       tags:
@@ -1357,8 +1251,7 @@
           content:
             application/json:
               schema: {}
-      description: Apply a previously proposed self-model change and notify subscribers.
-      summary: Apply self-model proposal
+      description: 'SelfModel: POST /admin/self_model/apply.'
   /admin/self_model/propose:
     post:
       tags:
@@ -1386,9 +1279,7 @@
           content:
             application/json:
               schema: {}
-      description: Submit a self-model patch proposal for an agent and emit the proposal
-        event.
-      summary: Propose self-model update
+      description: 'SelfModel: POST /admin/self_model/propose.'
   /admin/tools:
     get:
       tags:
@@ -1402,9 +1293,7 @@
               schema: {}
         '401':
           description: Unauthorized
-      description: Return the catalog of available tools with stability and capability
-        metadata.
-      summary: List registered tools
+      description: 'Tools: GET /admin/tools.'
   /admin/tools/cache_stats:
     get:
       tags:
@@ -1418,8 +1307,7 @@
               schema: {}
         '401':
           description: Unauthorized
-      description: Return cache utilisation metrics for the shared tool cache.
-      summary: Inspect tool cache statistics
+      description: 'Tools: GET /admin/tools/cache_stats.'
   /admin/tools/run:
     post:
       tags:
@@ -1447,9 +1335,7 @@
           description: Unknown tool
         '500':
           description: Tool runtime error
-      description: Execute a registered tool with the provided input payload and return
-        its output.
-      summary: Run tool
+      description: 'Tools: POST /admin/tools/run.'
   /admin/world_diffs:
     get:
       tags:
@@ -1463,8 +1349,7 @@
               schema: {}
         '401':
           description: Unauthorized
-      description: Return the queue of pending world diffs awaiting review.
-      summary: List world diffs
+      description: 'Review: GET /admin/world_diffs.'
   /admin/world_diffs/decision:
     post:
       tags:
@@ -1488,8 +1373,7 @@
           description: Diff not found
         '500':
           description: Storage error
-      description: Accept or reject a queued world diff and persist the decision outcome.
-      summary: Record world diff decision
+      description: 'Review: POST /admin/world_diffs/decision.'
   /admin/world_diffs/queue:
     post:
       tags:
@@ -1511,15 +1395,14 @@
           description: Unauthorized
         '500':
           description: Storage error
-      description: Enqueue a world diff for review with the supplied metadata.
-      summary: Queue world diff
+      description: 'Review: POST /admin/world_diffs/queue.'
   /events:
     get:
       tags:
       - Events
-      summary: Stream event feed
-      description: Open the Server-Sent Events stream of normalized envelopes; supports
-        prefix filtering and Last-Event-ID replay.
+      summary: "Server\u2011Sent Events stream of envelopes."
+      description: "Server\u2011Sent Events stream of envelopes; supports replay and\
+        \ prefix filters."
       operationId: events_sse_doc
       parameters:
       - name: after
@@ -1571,9 +1454,8 @@
     get:
       tags:
       - Meta
-      summary: Readiness probe
-      description: 'Return a simple readiness payload (`{"ok": true}`) suitable for
-        health checks and load balancers.'
+      summary: Health probe.
+      description: Service readiness probe.
       operationId: healthz_doc
       responses:
         '200':
@@ -1594,14 +1476,12 @@
             text/plain:
               schema:
                 type: string
-      description: Serve Prometheus-formatted metrics for the unified server, including
-        tool cache counters.
-      summary: Export Prometheus metrics
+      description: 'Public: GET /metrics.'
   /orchestrator/mini_agents:
     get:
       tags:
       - Orchestrator
-      summary: List mini-agent templates
+      summary: List available mini-agents (placeholder).
       operationId: orchestrator_mini_agents_doc
       responses:
         '200':
@@ -1615,13 +1495,12 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
-      description: Return placeholder metadata about available mini-agents while the
-        orchestrator capability is incubating.
+      description: List available mini-agents (placeholder).
   /orchestrator/mini_agents/start_training:
     post:
       tags:
       - Orchestrator
-      summary: Start mini-agent training
+      summary: Start a training job that results in a suggested Logic Unit (admin).
       operationId: orchestrator_start_training_doc
       requestBody:
         content:
@@ -1647,8 +1526,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
-      description: Kick off a training job that will propose a Logic Unit configuration
-        once complete; returns an async job handle when accepted.
+      description: Start a training job that results in a suggested Logic Unit (admin).
   /projects:
     post:
       tags:
@@ -1672,9 +1550,7 @@
           description: Unauthorized
         '500':
           description: Error
-      description: Create a new project directory, seed default notes, and emit the
-        creation event.
-      summary: Create project
+      description: 'Projects: POST /projects.'
   /projects/{proj}/file:
     put:
       tags:
@@ -1713,9 +1589,7 @@
           description: Conflict
         '500':
           description: Error
-      description: Create or replace a project file at the given path, enforcing optimistic
-        concurrency and quotas.
-      summary: Write project file
+      description: 'Projects: PUT /projects/{proj}/file.'
     patch:
       tags:
       - Projects
@@ -1753,9 +1627,7 @@
           description: Conflict
         '500':
           description: Error
-      description: Apply a JSON patch or diff patch to an existing project file while
-        checking version guards.
-      summary: Patch project file
+      description: 'Projects: PATCH /projects/{proj}/file.'
   /projects/{proj}/import:
     post:
       tags:
@@ -1788,9 +1660,7 @@
           description: Forbidden
         '500':
           description: Error
-      description: Copy or move a file from the staging area into the project workspace
-        and emit audit events.
-      summary: Import project asset
+      description: 'Projects: POST /projects/{proj}/import.'
   /projects/{proj}/notes:
     put:
       tags:
@@ -1822,9 +1692,7 @@
           description: Unauthorized
         '500':
           description: Error
-      description: Replace the project notes document and return metadata for the
-        updated file.
-      summary: Save project notes
+      description: 'Projects: PUT /projects/{proj}/notes.'
   /research_watcher/{id}/approve:
     post:
       tags:
@@ -1855,9 +1723,7 @@
           description: Not found
         '500':
           description: Error
-      description: Mark a research watcher entry as approved, optionally attaching
-        an operator note.
-      summary: Approve research watcher item
+      description: 'Research: POST /research_watcher/{id}/approve.'
   /research_watcher/{id}/archive:
     post:
       tags:
@@ -1888,14 +1754,12 @@
           description: Not found
         '500':
           description: Error
-      description: Archive a research watcher entry to remove it from the active queue
-        while preserving audit history.
-      summary: Archive research watcher item
+      description: 'Research: POST /research_watcher/{id}/archive.'
   /spec/health:
     get:
       tags:
       - Specs
-      summary: Inspect spec artifacts
+      summary: Health summary for spec artifacts (presence/size).
       operationId: spec_health_doc
       responses:
         '200':
@@ -1903,8 +1767,7 @@
           content:
             application/json:
               schema: {}
-      description: Report presence, size, and checksum information for bundled OpenAPI,
-        AsyncAPI, and schema artifacts.
+      description: Health summary for spec artifacts (presence/size).
   /staging/actions/{id}/approve:
     post:
       tags:
@@ -1935,9 +1798,7 @@
           description: Not found
         '500':
           description: Error
-      description: Approve a staged action so it can execute and emit the resulting
-        workflow job.
-      summary: Approve staging action
+      description: 'Staging: POST /staging/actions/{id}/approve.'
   /staging/actions/{id}/deny:
     post:
       tags:
@@ -1968,16 +1829,13 @@
           description: Not found
         '500':
           description: Error
-      description: Deny a staged action with an optional reason, preventing it from
-        executing.
-      summary: Deny staging action
+      description: 'Staging: POST /staging/actions/{id}/deny.'
   /state/actions:
     get:
       tags:
       - State
-      summary: List recent actions
-      description: Return the rolling window of actions emitted by the kernel, ordered
-        from newest to oldest.
+      summary: Recent actions list.
+      description: Recent actions list (most recent first).
       operationId: state_actions_doc
       parameters:
       - name: limit
@@ -2029,9 +1887,8 @@
     get:
       tags:
       - State
-      summary: Inspect belief store
-      description: Return the current belief entries derived from events so clients
-        can reason over world facts.
+      summary: Current beliefs snapshot derived from events.
+      description: Current beliefs snapshot derived from events.
       operationId: state_beliefs_doc
       responses:
         '200':
@@ -2048,9 +1905,8 @@
     get:
       tags:
       - State
-      summary: Inspect cluster nodes
-      description: Return the snapshot of known cluster nodes, their roles, and health
-        metadata.
+      summary: Cluster nodes snapshot.
+      description: Cluster nodes snapshot (admin-only).
       operationId: state_cluster_doc
       responses:
         '200':
@@ -2067,7 +1923,7 @@
     get:
       tags:
       - State
-      summary: List experiment events
+      summary: Experiment events snapshot (public read-model).
       operationId: state_experiments_doc
       responses:
         '200':
@@ -2075,13 +1931,12 @@
           content:
             application/json:
               schema: {}
-      description: Expose the experiment read-model summarizing variants, assignments,
-        and recent outcomes.
+      description: Experiment events snapshot (public read-model).
   /state/guardrails_metrics:
     get:
       tags:
       - State
-      summary: Inspect guardrail metrics
+      summary: Guardrails circuit-breaker metrics snapshot.
       operationId: state_guardrails_metrics_doc
       responses:
         '200':
@@ -2094,14 +1949,14 @@
           content:
             application/json:
               schema: {}
-      description: Return guardrail circuit-breaker counters and latency measurements
-        for monitoring automation health.
+      description: Guardrails circuit-breaker metrics snapshot.
   /state/intents:
     get:
       tags:
       - State
-      summary: List recent intents
-      description: Return the rolling window of intent events emitted by the kernel.
+      summary: Recent intents stream (rolling window) with a monotonic version counter.
+      description: Recent intents stream (rolling window) with a monotonic version
+        counter.
       operationId: state_intents_doc
       responses:
         '200':
@@ -2118,9 +1973,8 @@
     get:
       tags:
       - State
-      summary: Inspect model catalog
-      description: Return the derived model catalog with provider metadata, install
-        status, and version details.
+      summary: Model catalog read-model.
+      description: Model catalog read-model.
       operationId: state_models_doc
       responses:
         '200':
@@ -2180,15 +2034,12 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/HashPage'
-      description: Return a paginated view of installed model blobs with filters for
-        provider, size, and hash.
-      summary: List installed model hashes
+      description: 'State: GET /state/models_hashes.'
   /state/models_metrics:
     get:
       tags:
       - State
-      description: Return model runtime metrics, including cache hits and latency
-        data, for observability dashboards.
+      description: Models metrics snapshot.
       operationId: state_models_metrics_doc
       responses:
         '200':
@@ -2202,7 +2053,6 @@
           content:
             application/json:
               schema: {}
-      summary: Inspect model metrics
   /state/observations:
     get:
       tags:
@@ -2252,7 +2102,7 @@
     get:
       tags:
       - Orchestrator
-      summary: List orchestrator jobs
+      summary: Orchestrator jobs snapshot.
       operationId: state_orchestrator_jobs_doc
       parameters:
       - name: limit
@@ -2273,8 +2123,7 @@
             application/json:
               schema:
                 $ref: '#/components/schemas/ProblemDetails'
-      description: Return the current orchestrator job queue including statuses, runners,
-        and progress metadata.
+      description: Orchestrator jobs snapshot.
   /state/projects:
     get:
       tags:
@@ -2286,9 +2135,7 @@
           content:
             application/json:
               schema: {}
-      description: Return the cached project snapshot with file tree, notes, and metadata
-        for quick reads.
-      summary: Snapshot project catalog
+      description: 'Projects: GET /state/projects.'
   /state/projects/{proj}/file:
     get:
       tags:
@@ -2319,9 +2166,7 @@
           description: Unauthorized
         '404':
           description: Not found
-      description: Return the latest stored contents for a project file identified
-        by project and relative path.
-      summary: Fetch project file snapshot
+      description: 'Projects: GET /state/projects/{proj}/file.'
   /state/projects/{proj}/notes:
     get:
       tags:
@@ -2345,9 +2190,7 @@
           description: Invalid request
         '401':
           description: Unauthorized
-      description: Return the current project notes document with metadata such as
-        checksum and size.
-      summary: Fetch project notes
+      description: 'Projects: GET /state/projects/{proj}/notes.'
   /state/projects/{proj}/tree:
     get:
       tags:
@@ -2378,14 +2221,12 @@
           description: Unauthorized
         '404':
           description: Not found
-      description: Return a directory listing for a project path to help clients explore
-        workspace structure.
-      summary: Browse project tree
+      description: 'Projects: GET /state/projects/{proj}/tree.'
   /state/research_watcher:
     get:
       tags:
       - State
-      summary: Inspect research watcher
+      summary: Research watcher queue snapshot.
       operationId: state_research_watcher_doc
       parameters:
       - name: status
@@ -2414,13 +2255,12 @@
           content:
             application/json:
               schema: {}
-      description: Return the research watcher queue snapshot with pending items,
-        statuses, and telemetry.
+      description: Research watcher queue snapshot.
   /state/route_stats:
     get:
       tags:
       - State
-      summary: Inspect route metrics
+      summary: Bus and per-route counters snapshot.
       operationId: state_route_stats_doc
       responses:
         '200':
@@ -2428,13 +2268,12 @@
           content:
             application/json:
               schema: {}
-      description: Return per-route counters, durations, and cache statistics aggregated
-        by the server.
+      description: Bus and per-route counters snapshot.
   /state/runtime_matrix:
     get:
       tags:
       - State
-      summary: Inspect runtime matrix
+      summary: Runtime matrix snapshot.
       operationId: state_runtime_matrix_doc
       responses:
         '200':
@@ -2448,13 +2287,12 @@
           content:
             application/json:
               schema: {}
-      description: Return the runtime matrix covering available runtimes, capabilities,
-        and health signals.
+      description: Runtime matrix snapshot.
   /state/staging/actions:
     get:
       tags:
       - State
-      summary: Inspect staging actions
+      summary: Staging queue snapshot.
       operationId: state_staging_actions_doc
       parameters:
       - name: status
@@ -2483,13 +2321,12 @@
           content:
             application/json:
               schema: {}
-      description: Return staged actions awaiting review or execution in the staging
-        queue.
+      description: Staging queue snapshot.
   /state/tasks:
     get:
       tags:
       - State
-      summary: Inspect background tasks
+      summary: Background tasks status snapshot.
       operationId: state_tasks_doc
       responses:
         '200':
@@ -2497,13 +2334,12 @@
           content:
             application/json:
               schema: {}
-      description: Return the background task registry with progress, retry counts,
-        and assigned workers.
+      description: Background tasks status snapshot.
   /state/training/telemetry:
     get:
       tags:
       - State
-      summary: Inspect training telemetry
+      summary: Training telemetry snapshot.
       operationId: state_training_telemetry_doc
       responses:
         '200':
@@ -2513,15 +2349,13 @@
               schema: {}
         '401':
           description: Unauthorized
-      description: Return aggregated Training Park telemetry, including success ratios,
-        recall, and coverage metrics.
+      description: Training telemetry snapshot.
   /state/world:
     get:
       tags:
       - State
-      summary: Inspect world model
-      description: Return the active world graph snapshot with claims, provenance
-        metadata, and belief relationships.
+      summary: Project world model snapshot (belief graph view).
+      description: Project world model snapshot (belief graph view).
       operationId: state_world_doc
       parameters:
       - name: proj
@@ -2545,9 +2379,8 @@
     get:
       tags:
       - State
-      summary: Select world claims
-      description: Evaluate a query against the world graph and return the top-k claims
-        that match the provided filters.
+      summary: Select top-k claims for a query.
+      description: Select top-k claims for a query.
       operationId: state_world_select_doc
       parameters:
       - name: proj
```

## AsyncAPI (Events)

```diff
--- asyncapi.base.yaml
+++ asyncapi.head.yaml
@@ -124,6 +124,16 @@
         payload:
           type: object
           additionalProperties: true
+  'autonomy.budget.updated':
+    subscribe:
+      operationId: autonomy_budget_updated_event
+      summary: "autonomy.budget.updated event"
+      description: "Event published on 'autonomy.budget.updated' channel."
+      message:
+        name: 'autonomy.budget.updated'
+        payload:
+          type: object
+          additionalProperties: true
   'autonomy.interrupt':
     subscribe:
       operationId: autonomy_interrupt_event
@@ -184,6 +194,16 @@
         payload:
           type: object
           additionalProperties: true
+  'cache.policy.reloaded':
+    subscribe:
+      operationId: cache_policy_reloaded_event
+      summary: "cache.policy.reloaded event"
+      description: "Event published on 'cache.policy.reloaded' channel."
+      message:
+        name: 'cache.policy.reloaded'
+        payload:
+          type: object
+          additionalProperties: true
   'catalog.updated':
     subscribe:
       operationId: catalog_updated_event
@@ -254,6 +274,16 @@
         payload:
           type: object
           additionalProperties: true
+  'config.reloaded':
+    subscribe:
+      operationId: config_reloaded_event
+      summary: "config.reloaded event"
+      description: "Event published on 'config.reloaded' channel."
+      message:
+        name: 'config.reloaded'
+        payload:
+          type: object
+          additionalProperties: true
   'connectors.registered':
     subscribe:
       operationId: connectors_registered_event
@@ -804,6 +834,26 @@
         payload:
           type: object
           additionalProperties: true
+  'policy.gating.reloaded':
+    subscribe:
+      operationId: policy_gating_reloaded_event
+      summary: "policy.gating.reloaded event"
+      description: "Event published on 'policy.gating.reloaded' channel."
+      message:
+        name: 'policy.gating.reloaded'
+        payload:
+          type: object
+          additionalProperties: true
+  'policy.guardrails.applied':
+    subscribe:
+      operationId: policy_guardrails_applied_event
+      summary: "policy.guardrails.applied event"
+      description: "Event published on 'policy.guardrails.applied' channel."
+      message:
+        name: 'policy.guardrails.applied'
+        payload:
+          type: object
+          additionalProperties: true
   'policy.reloaded':
     subscribe:
       operationId: policy_reloaded_event
@@ -864,6 +914,26 @@
         payload:
           type: object
           additionalProperties: true
+  'projects.snapshot.created':
+    subscribe:
+      operationId: projects_snapshot_created_event
+      summary: "projects.snapshot.created event"
+      description: "Event published on 'projects.snapshot.created' channel."
+      message:
+        name: 'projects.snapshot.created'
+        payload:
+          type: object
+          additionalProperties: true
+  'projects.snapshot.restored':
+    subscribe:
+      operationId: projects_snapshot_restored_event
+      summary: "projects.snapshot.restored event"
+      description: "Event published on 'projects.snapshot.restored' channel."
+      message:
+        name: 'projects.snapshot.restored'
+        payload:
+          type: object
+          additionalProperties: true
   'research.watcher.updated':
     subscribe:
       operationId: research_watcher_updated_event
@@ -924,6 +994,26 @@
         payload:
           type: object
           additionalProperties: true
+  'runtime.restore.completed':
+    subscribe:
+      operationId: runtime_restore_completed_event
+      summary: "runtime.restore.completed event"
+      description: "Event published on 'runtime.restore.completed' channel."
+      message:
+        name: 'runtime.restore.completed'
+        payload:
+          type: object
+          additionalProperties: true
+  'runtime.restore.requested':
+    subscribe:
+      operationId: runtime_restore_requested_event
+      summary: "runtime.restore.requested event"
+      description: "Event published on 'runtime.restore.requested' channel."
+      message:
+        name: 'runtime.restore.requested'
+        payload:
+          type: object
+          additionalProperties: true
   'runtime.state.changed':
     subscribe:
       operationId: runtime_state_changed_event
@@ -1224,6 +1314,16 @@
         payload:
           type: object
           additionalProperties: true
+  'world.telemetry':
+    subscribe:
+      operationId: world_telemetry_event
+      summary: "world.telemetry event"
+      description: "Event published on 'world.telemetry' channel."
+      message:
+        name: 'world.telemetry'
+        payload:
+          type: object
+          additionalProperties: true
   'world.updated':
     subscribe:
       operationId: world_updated_event
```


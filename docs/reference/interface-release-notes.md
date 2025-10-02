---
title: Interface Release Notes
---

# Interface Release Notes
Updated: 2025-09-30
Type: Reference

Base: `origin/main`

## OpenAPI (REST)

### New Endpoints: None
-----------------------

### Deleted Endpoints: None
---------------------------

### Modified Endpoints: 108
---------------------------
GET /about
- Summary changed from 'Inspect service metadata' to 'Service metadata and endpoints index.'
- Description changed from 'Return version, build information, endpoint counts, and the enumerated public/admin routes announced by the server.' to 'Service metadata, endpoints index, and performance preset.'

GET /admin/chat
- Summary changed from 'Fetch chat transcript' to ''
- Description changed from 'Retrieve the active admin chat transcript, including message history and tool metadata.' to 'Chat: GET /admin/chat.'

POST /admin/chat/clear
- Summary changed from 'Clear chat transcript' to ''
- Description changed from 'Wipe the stored admin chat transcript and reset associated tool context for a fresh session.' to 'Chat: POST /admin/chat/clear.'

POST /admin/chat/send
- Summary changed from 'Send chat message' to ''
- Description changed from 'Submit a message to the admin chat lane and receive the synthesized assistant response.' to 'Chat: POST /admin/chat/send.'

GET /admin/chat/status
- Summary changed from 'Query chat status' to ''
- Description changed from 'Report chat lane health, including the most recent latency probe; optionally trigger a new probe.' to 'Chat: GET /admin/chat/status.'

POST /admin/distill
- Summary changed from 'Run distillation pass' to 'Trigger a manual distillation pass.'
- Description changed from 'Trigger the manual distillation pipeline to snapshot playbooks, refresh beliefs, and regenerate derived notebooks.' to 'Trigger a manual distillation pass.'

POST /admin/experiments/activate
- Summary changed from 'Activate experiment variant' to ''
- Description changed from 'Mark a variant as the active choice for the experiment and persist the rollout decision.' to 'Experiments: POST /admin/experiments/activate.'

POST /admin/experiments/assign
- Summary changed from 'Assign experiment variant' to ''
- Description changed from 'Record or override an experiment assignment for a participant or agent and broadcast the decision.' to 'Experiments: POST /admin/experiments/assign.'

POST /admin/experiments/define
- Summary changed from 'Define experiment' to ''
- Description changed from 'Create or update an experiment definition with the supplied variants and configuration payload.' to 'Experiments: POST /admin/experiments/define.'

GET /admin/experiments/list
- Summary changed from 'List experiments' to ''
- Description changed from 'Return all experiment definitions currently registered with their variant metadata.' to 'Experiments: GET /admin/experiments/list.'

POST /admin/experiments/run
- Summary changed from 'Run experiment on goldens' to ''
- Description changed from 'Execute the requested experiment variants against the chosen golden project and return the evaluation outcome.' to 'Experiments: POST /admin/experiments/run.'

GET /admin/experiments/scoreboard
- Summary changed from 'Fetch experiment scoreboard' to ''
- Description changed from 'Provide aggregated performance metrics for each experiment variant to compare recent runs.' to 'Experiments: GET /admin/experiments/scoreboard.'

POST /admin/experiments/start
- Summary changed from 'Start experiment run' to ''
- Description changed from 'Publish a start event for a new experiment with optional assignment, budget, and variant hints.' to 'Experiments: POST /admin/experiments/start.'

POST /admin/experiments/stop
- Summary changed from 'Stop experiment' to ''
- Description changed from 'Halt an experiment run by emitting a stop event for the provided experiment identifier.' to 'Experiments: POST /admin/experiments/stop.'

GET /admin/experiments/winners
- Summary changed from 'List experiment winners' to ''
- Description changed from 'Return the top-performing variants for experiments based on the latest evaluation data.' to 'Experiments: GET /admin/experiments/winners.'

POST /admin/feedback/analyze
- Summary changed from 'Recompute feedback suggestions' to ''
- Description changed from 'Trigger an immediate feedback analysis pass to refresh suggestions from the latest signals.' to 'Feedback: POST /admin/feedback/analyze.'

POST /admin/feedback/apply
- Summary changed from 'Apply feedback suggestion' to ''
- Description changed from 'Apply the identified suggestion and let the engine reconcile policy and audit outcomes.' to 'Feedback: POST /admin/feedback/apply.'

POST /admin/feedback/auto
- Summary changed from 'Toggle automatic feedback application' to ''
- Description changed from 'Enable or disable automatic application of approved feedback suggestions.' to 'Feedback: POST /admin/feedback/auto.'

GET /admin/feedback/policy
- Summary changed from 'Fetch feedback policy' to ''
- Description changed from 'Return the effective feedback application policy after merging defaults and overrides.' to 'Feedback: GET /admin/feedback/policy.'

POST /admin/feedback/reset
- Summary changed from 'Reset feedback engine' to ''
- Description changed from 'Clear feedback signals, suggestions, and cached state for a cold start.' to 'Feedback: POST /admin/feedback/reset.'

POST /admin/feedback/rollback
- Summary changed from 'Roll back feedback snapshot' to ''
- Description changed from 'Restore feedback state to the requested snapshot version and return the resulting suggestion set.' to 'Feedback: POST /admin/feedback/rollback.'

POST /admin/feedback/signal
- Summary changed from 'Record feedback signal' to ''
- Description changed from 'Submit a feedback signal with confidence and severity so it influences subsequent analysis.' to 'Feedback: POST /admin/feedback/signal.'

GET /admin/feedback/state
- Summary changed from 'Inspect feedback state' to ''
- Description changed from 'Return the current feedback engine snapshot, including signals, suggestions, and configuration.' to 'Feedback: GET /admin/feedback/state.'

GET /admin/feedback/suggestions
- Summary changed from 'List feedback suggestions' to ''
- Description changed from 'Return the current queue of actionable feedback suggestions with their metadata.' to 'Feedback: GET /admin/feedback/suggestions.'

GET /admin/feedback/updates
- Summary changed from 'Fetch feedback updates' to ''
- Description changed from 'Retrieve feedback suggestions updated since a provided version cursor, enabling incremental refresh.' to 'Feedback: GET /admin/feedback/updates.'

GET /admin/feedback/versions
- Summary changed from 'List feedback snapshots' to ''
- Description changed from 'Enumerate available feedback snapshots that can be inspected or rolled back to.' to 'Feedback: GET /admin/feedback/versions.'

POST /admin/goldens/add
- Summary changed from 'Add golden record' to ''
- Description changed from 'Append a golden item to the specified project collection and persist the updated set.' to 'Experiments: POST /admin/goldens/add.'

GET /admin/goldens/list
- Summary changed from 'List golden records' to ''
- Description changed from 'Return the golden dataset for the requested project, including individual test cases.' to 'Experiments: GET /admin/goldens/list.'

POST /admin/goldens/run
- Summary changed from 'Evaluate golden set' to ''
- Description changed from 'Run the supplied golden dataset against the current chat runtime and report evaluation metrics.' to 'Experiments: POST /admin/goldens/run.'

GET /admin/governor/hints
- Summary changed from 'Inspect governor hints' to ''
- Description changed from 'Return the currently effective governor hints that shape scheduling and retrieval behaviour.' to 'Introspect: GET /admin/governor/hints.'

POST /admin/governor/hints
- Summary changed from 'Update governor hints' to ''
- Description changed from 'Apply new governor hints to adjust scheduling, retrieval, and context construction parameters.' to 'Introspect: POST /admin/governor/hints.'

GET /admin/governor/profile
- Summary changed from 'Get governor profile' to ''
- Description changed from 'Return the active governor profile name configured for the node.' to 'Introspect: GET /admin/governor/profile.'

POST /admin/governor/profile
- Summary changed from 'Set governor profile' to ''
- Description changed from 'Switch the governor to the requested profile and broadcast the change.' to 'Introspect: POST /admin/governor/profile.'

POST /admin/hierarchy/accept
- Summary changed from 'Accept hierarchy offer' to ''
- Description changed from 'Accept a hierarchy offer message to finalize a connection with the given participant.' to 'Hierarchy: POST /admin/hierarchy/accept.'

POST /admin/hierarchy/hello
- Summary changed from 'Send hierarchy hello' to ''
- Description changed from 'Emit the initial hello message in the hierarchy handshake with another participant.' to 'Hierarchy: POST /admin/hierarchy/hello.'

POST /admin/hierarchy/offer
- Summary changed from 'Offer hierarchy connection' to ''
- Description changed from 'Publish a hierarchy offer to negotiate roles and capabilities with a peer.' to 'Hierarchy: POST /admin/hierarchy/offer.'

POST /admin/hierarchy/role
- Summary changed from 'Set hierarchy roles' to ''
- Description changed from 'Update hierarchy role assignments for a participant and persist the change.' to 'Hierarchy: POST /admin/hierarchy/role.'

GET /admin/hierarchy/state
- Summary changed from 'Inspect hierarchy state' to ''
- Description changed from 'Return the current hierarchy session map, including offers, participants, and active roles.' to 'Hierarchy: GET /admin/hierarchy/state.'

GET /admin/memory
- Summary changed from 'List recent memory records' to 'List recent memory items (admin helper).'
- Description changed from 'Return the latest memory items for inspection; supports lane and limit filters for debugging.' to 'List recent memory items (admin helper).'

POST /admin/memory/apply
- Summary changed from 'Upsert memory record' to 'Insert a memory item (admin helper).'
- Description changed from 'Insert or update a memory item via the admin helper and emit the associated memory events for auditing.' to 'Insert a memory item (admin helper).'

GET /admin/memory/quarantine
- Summary changed from 'List quarantined memories' to ''
- Description changed from 'Return entries awaiting review in the memory quarantine queue.' to 'Review: GET /admin/memory/quarantine.'

POST /admin/memory/quarantine
- Summary changed from 'Queue memory for review' to ''
- Description changed from 'Enqueue a memory item for quarantine review and emit the appropriate audit event.' to 'Review: POST /admin/memory/quarantine.'

POST /admin/memory/quarantine/admit
- Summary changed from 'Admit quarantined memory' to ''
- Description changed from 'Remove a memory from quarantine, optionally admitting or rejecting it, and report the outcome.' to 'Review: POST /admin/memory/quarantine/admit.'

GET /admin/models
- Summary changed from 'List models' to ''
- Description changed from 'Return the configured model entries including provider metadata.' to 'Models: GET /admin/models.'

POST /admin/models/add
- Summary changed from 'Add model entry' to ''
- Description changed from 'Register a model entry with optional provider, path, and status metadata.' to 'Models: POST /admin/models/add.'

GET /admin/models/by-hash/{sha256}
- Summary changed from 'Download model blob' to ''
- Description changed from 'Stream a CAS-stored model blob by SHA-256 with caching headers and partial range support.' to 'Models: GET /admin/models/by-hash/{sha256}.'

POST /admin/models/cas_gc
- Summary changed from 'Run CAS garbage collection' to ''
- Description changed from 'Execute a content-addressed store cleanup pass and report removed blobs.' to 'Models: POST /admin/models/cas_gc.'

GET /admin/models/concurrency
- Summary changed from 'Inspect model concurrency' to ''
- Description changed from 'Return the current model concurrency settings and snapshot telemetry.' to 'Models: GET /admin/models/concurrency.'

POST /admin/models/concurrency
- Summary changed from 'Update model concurrency' to ''
- Description changed from 'Apply new concurrency limits or blocking behaviour for model execution.' to 'Models: POST /admin/models/concurrency.'

GET /admin/models/default
- Summary changed from 'Get default model' to ''
- Description changed from 'Return the identifier of the default model selection.' to 'Models: GET /admin/models/default.'

POST /admin/models/default
- Summary changed from 'Set default model' to ''
- Description changed from 'Select the default model to be used for future requests.' to 'Models: POST /admin/models/default.'

POST /admin/models/download
- Summary changed from 'Start model download' to ''
- Description changed from 'Request download or import of a model artifact and enqueue the job if supported.' to 'Models: POST /admin/models/download.'

POST /admin/models/download/cancel
- Summary changed from 'Cancel model download' to ''
- Description changed from 'Cancel an in-flight model download job when the backend supports it.' to 'Models: POST /admin/models/download/cancel.'

GET /admin/models/jobs
- Summary changed from 'Inspect model jobs' to ''
- Description changed from 'Return the current queue of model download and load jobs with their statuses.' to 'Models: GET /admin/models/jobs.'

POST /admin/models/load
- Summary changed from 'Load model manifest' to ''
- Description changed from 'Load model entries from the persisted manifest on disk.' to 'Models: POST /admin/models/load.'

POST /admin/models/refresh
- Summary changed from 'Refresh models list' to ''
- Description changed from 'Refresh the live model list from runtime state and return the updated entries.' to 'Models: POST /admin/models/refresh.'

POST /admin/models/remove
- Summary changed from 'Remove model entry' to ''
- Description changed from 'Remove a model entry by identifier and report whether it existed.' to 'Models: POST /admin/models/remove.'

POST /admin/models/save
- Summary changed from 'Save model manifest' to ''
- Description changed from 'Persist the current model registry to the on-disk manifest.' to 'Models: POST /admin/models/save.'

GET /admin/models/summary
- Summary changed from 'Summarize model catalog' to ''
- Description changed from 'Return aggregate statistics about installed models, storage usage, and capabilities.' to 'Models: GET /admin/models/summary.'

GET /admin/probe
- Summary changed from 'Inspect effective paths' to 'Effective path probe (successor to `/admin/probe`).'
- Description changed from 'Return the resolved state, cache, and config directories plus runtime metadata so operators can confirm filesystem layout.' to 'Effective path probe (successor to `/admin/probe`).'

GET /admin/probe/hw
- Summary changed from 'Probe runtime hardware' to 'Hardware/software probe (`/admin/probe/hw`).'
- Description changed from 'Report detected hardware and OS capabilities—including CPU, GPU, and accelerators—to confirm what the node can access.' to 'Hardware/software probe (`/admin/probe/hw`).'

GET /admin/probe/metrics
- Summary changed from 'Probe metrics snapshot' to 'Metrics snapshot probe (`/admin/probe/metrics`).'
- Description changed from 'Return the current metrics summary (Prometheus-style counters and histograms) for quick diagnostics.' to 'Metrics snapshot probe (`/admin/probe/metrics`).'

POST /admin/self_model/apply
- Summary changed from 'Apply self-model proposal' to ''
- Description changed from 'Apply a previously proposed self-model change and notify subscribers.' to 'SelfModel: POST /admin/self_model/apply.'

POST /admin/self_model/propose
- Summary changed from 'Propose self-model update' to ''
- Description changed from 'Submit a self-model patch proposal for an agent and emit the proposal event.' to 'SelfModel: POST /admin/self_model/propose.'

GET /admin/tools
- Summary changed from 'List registered tools' to ''
- Description changed from 'Return the catalog of available tools with stability and capability metadata.' to 'Tools: GET /admin/tools.'

GET /admin/tools/cache_stats
- Summary changed from 'Inspect tool cache statistics' to ''
- Description changed from 'Return cache utilisation metrics for the shared tool cache.' to 'Tools: GET /admin/tools/cache_stats.'

POST /admin/tools/run
- Summary changed from 'Run tool' to ''
- Description changed from 'Execute a registered tool with the provided input payload and return its output.' to 'Tools: POST /admin/tools/run.'

GET /admin/world_diffs
- Summary changed from 'List world diffs' to ''
- Description changed from 'Return the queue of pending world diffs awaiting review.' to 'Review: GET /admin/world_diffs.'

POST /admin/world_diffs/decision
- Summary changed from 'Record world diff decision' to ''
- Description changed from 'Accept or reject a queued world diff and persist the decision outcome.' to 'Review: POST /admin/world_diffs/decision.'

POST /admin/world_diffs/queue
- Summary changed from 'Queue world diff' to ''
- Description changed from 'Enqueue a world diff for review with the supplied metadata.' to 'Review: POST /admin/world_diffs/queue.'

GET /events
- Summary changed from 'Stream event feed' to 'Server‑Sent Events stream of envelopes.'
- Description changed from 'Open the Server-Sent Events stream of normalized envelopes; supports prefix filtering and Last-Event-ID replay.' to 'Server‑Sent Events stream of envelopes; supports replay and prefix filters.'

GET /healthz
- Summary changed from 'Readiness probe' to 'Health probe.'
- Description changed from 'Return a simple readiness payload (`{"ok": true}`) suitable for health checks and load balancers.' to 'Service readiness probe.'

GET /metrics
- Summary changed from 'Export Prometheus metrics' to ''
- Description changed from 'Serve Prometheus-formatted metrics for the unified server, including tool cache counters.' to 'Public: GET /metrics.'

GET /orchestrator/mini_agents
- Summary changed from 'List mini-agent templates' to 'List available mini-agents (placeholder).'
- Description changed from 'Return placeholder metadata about available mini-agents while the orchestrator capability is incubating.' to 'List available mini-agents (placeholder).'

POST /orchestrator/mini_agents/start_training
- Summary changed from 'Start mini-agent training' to 'Start a training job that results in a suggested Logic Unit (admin).'
- Description changed from 'Kick off a training job that will propose a Logic Unit configuration once complete; returns an async job handle when accepted.' to 'Start a training job that results in a suggested Logic Unit (admin).'

POST /projects
- Summary changed from 'Create project' to ''
- Description changed from 'Create a new project directory, seed default notes, and emit the creation event.' to 'Projects: POST /projects.'

PATCH /projects/{proj}/file
- Summary changed from 'Patch project file' to ''
- Description changed from 'Apply a JSON patch or diff patch to an existing project file while checking version guards.' to 'Projects: PATCH /projects/{proj}/file.'

PUT /projects/{proj}/file
- Summary changed from 'Write project file' to ''
- Description changed from 'Create or replace a project file at the given path, enforcing optimistic concurrency and quotas.' to 'Projects: PUT /projects/{proj}/file.'

POST /projects/{proj}/import
- Summary changed from 'Import project asset' to ''
- Description changed from 'Copy or move a file from the staging area into the project workspace and emit audit events.' to 'Projects: POST /projects/{proj}/import.'

PUT /projects/{proj}/notes
- Summary changed from 'Save project notes' to ''
- Description changed from 'Replace the project notes document and return metadata for the updated file.' to 'Projects: PUT /projects/{proj}/notes.'

POST /research_watcher/{id}/approve
- Summary changed from 'Approve research watcher item' to ''
- Description changed from 'Mark a research watcher entry as approved, optionally attaching an operator note.' to 'Research: POST /research_watcher/{id}/approve.'

POST /research_watcher/{id}/archive
- Summary changed from 'Archive research watcher item' to ''
- Description changed from 'Archive a research watcher entry to remove it from the active queue while preserving audit history.' to 'Research: POST /research_watcher/{id}/archive.'

GET /spec/health
- Summary changed from 'Inspect spec artifacts' to 'Health summary for spec artifacts (presence/size).'
- Description changed from 'Report presence, size, and checksum information for bundled OpenAPI, AsyncAPI, and schema artifacts.' to 'Health summary for spec artifacts (presence/size).'

POST /staging/actions/{id}/approve
- Summary changed from 'Approve staging action' to ''
- Description changed from 'Approve a staged action so it can execute and emit the resulting workflow job.' to 'Staging: POST /staging/actions/{id}/approve.'

POST /staging/actions/{id}/deny
- Summary changed from 'Deny staging action' to ''
- Description changed from 'Deny a staged action with an optional reason, preventing it from executing.' to 'Staging: POST /staging/actions/{id}/deny.'

GET /state/actions
- Summary changed from 'List recent actions' to 'Recent actions list.'
- Description changed from 'Return the rolling window of actions emitted by the kernel, ordered from newest to oldest.' to 'Recent actions list (most recent first).'
- Response now includes `version` (monotonic counter) aligned with the state observer so clients can detect updates without refetching entire histories.
- Response now sets `ETag: "state-actions-v<version>"` so caches and polling clients can reuse conditional requests.
- Response now sets `Cache-Control: private, max-age=2` for short-lived conditional polling.
- Query parameters added: `state`, `kind_prefix`, and `updated_since`, alongside the existing `limit`, enabling filtered action snapshots without client-side scanning.

GET /state/contributions
- Response now sets `version`, `ETag: "state-contributions-v<version>"`, and `Cache-Control: private, max-age=2` for conditional polling.

GET /state/episodes
- Response now sets `version`, `ETag: "state-episodes-v<version>"`, and `Cache-Control: private, max-age=2` for conditional polling.

GET /state/beliefs
- Summary changed from 'Inspect belief store' to 'Current beliefs snapshot derived from events.'
- Description changed from 'Return the current belief entries derived from events so clients can reason over world facts.' to 'Current beliefs snapshot derived from events.'
- Response now sets `version`, `ETag: "state-beliefs-v<version>"`, and `Cache-Control: private, max-age=2` for lightweight conditional polling.

GET /state/cluster
- Summary changed from 'Inspect cluster nodes' to 'Cluster nodes snapshot.'
- Description changed from 'Return the snapshot of known cluster nodes, their roles, and health metadata.' to 'Cluster nodes snapshot (admin-only).'
- Response now includes `generated` (RFC3339) and `generated_ms` (epoch milliseconds) so clients can surface the snapshot age without relying on local clocks.

GET /state/experiments
- Summary changed from 'List experiment events' to 'Experiment events snapshot (public read-model).'
- Description changed from 'Expose the experiment read-model summarizing variants, assignments, and recent outcomes.' to 'Experiment events snapshot (public read-model).'
- Response now sets `version`, `ETag: "state-experiments-v<version>"`, and `Cache-Control: private, max-age=2` for conditional polling.

GET /state/guardrails_metrics
- Summary changed from 'Inspect guardrail metrics' to 'Guardrails circuit-breaker metrics snapshot.'
- Description changed from 'Return guardrail circuit-breaker counters and latency measurements for monitoring automation health.' to 'Guardrails circuit-breaker metrics snapshot.'

GET /state/intents
- Summary changed from 'List recent intents' to 'Recent intents stream (rolling window).'
- Description changed from 'Return the rolling window of intent events emitted by the kernel.' to 'Recent intents stream (rolling window).'
- Response now includes `version` (monotonic counter) so clients can detect refreshes and wire delta polling.
- Response now sets `ETag: "state-intents-v<version>"` to support If-None-Match polling loops.
- Response now sets `Cache-Control: private, max-age=2` for short-lived conditional polling.

GET /state/models
- Summary changed from 'Inspect model catalog' to 'Model catalog read-model.'
- Description changed from 'Return the derived model catalog with provider metadata, install status, and version details.' to 'Model catalog read-model.'

GET /state/models_hashes
- Summary changed from 'List installed model hashes' to ''
- Description changed from 'Return a paginated view of installed model blobs with filters for provider, size, and hash.' to 'State: GET /state/models_hashes.'

GET /state/models_metrics
- Summary changed from 'Inspect model metrics' to ''
- Description changed from 'Return model runtime metrics, including cache hits and latency data, for observability dashboards.' to 'Models metrics snapshot.'

GET /state/observations
- Summary changed from 'List recent observations' to 'Recent observations from the event bus.'
- Description changed from 'Return the rolling window of observation events captured from the live event bus.' to 'Recent observations from the event bus.'
- Response now sets `version`, `ETag: "state-observations-v<version>"`, and `Cache-Control: private, max-age=2` to support conditional polling.
- Query parameters added: `limit` (most recent N items), `kind_prefix` (match event kind prefix), and `since` (RFC3339, include only envelopes emitted after the timestamp) for lightweight filtering. `arw-cli events observations` now also supports `--since-relative <window>` to derive the timestamp client-side (e.g., last 15 minutes).

GET /state/orchestrator/jobs
- Summary changed from 'List orchestrator jobs' to 'Orchestrator jobs snapshot.'
- Description changed from 'Return the current orchestrator job queue including statuses, runners, and progress metadata.' to 'Orchestrator jobs snapshot.'

GET /state/projects
- Summary changed from 'Snapshot project catalog' to ''
- Description changed from 'Return the cached project snapshot with file tree, notes, and metadata for quick reads.' to 'Projects: GET /state/projects.'

GET /state/projects/{proj}/file
- Summary changed from 'Fetch project file snapshot' to ''
- Description changed from 'Return the latest stored contents for a project file identified by project and relative path.' to 'Projects: GET /state/projects/{proj}/file.'

GET /state/projects/{proj}/notes
- Summary changed from 'Fetch project notes' to ''
- Description changed from 'Return the current project notes document with metadata such as checksum and size.' to 'Projects: GET /state/projects/{proj}/notes.'

GET /state/projects/{proj}/tree
- Summary changed from 'Browse project tree' to ''
- Description changed from 'Return a directory listing for a project path to help clients explore workspace structure.' to 'Projects: GET /state/projects/{proj}/tree.'

GET /state/research_watcher
- Summary changed from 'Inspect research watcher' to 'Research watcher queue snapshot.'
- Description changed from 'Return the research watcher queue snapshot with pending items, statuses, and telemetry.' to 'Research watcher queue snapshot.'

GET /state/route_stats
- Summary changed from 'Inspect route metrics' to 'Bus and per-route counters snapshot.'
- Description changed from 'Return per-route counters, durations, and cache statistics aggregated by the server.' to 'Bus and per-route counters snapshot.'
- Response now sets `version`, `ETag: "state-route-stats-v<version>"`, and `Cache-Control: private, max-age=2` for conditional polling.

GET /state/runtime_matrix
- Summary changed from 'Inspect runtime matrix' to 'Runtime matrix snapshot.'
- Description changed from 'Return the runtime matrix covering available runtimes, capabilities, and health signals.' to 'Runtime matrix snapshot.'
- Response now includes `ttl_seconds` to mirror `ARW_RUNTIME_MATRIX_TTL_SEC` and help clients decide when to refresh the snapshot proactively.

GET /state/staging/actions
- Summary changed from 'Inspect staging actions' to 'Staging queue snapshot.'
- Description changed from 'Return staged actions awaiting review or execution in the staging queue.' to 'Staging queue snapshot.'
- Response now includes `generated` (RFC3339) and `generated_ms` (epoch milliseconds) timestamps for stable freshness indicators across clients.

GET /state/tasks
- Summary changed from 'Inspect background tasks' to 'Background tasks status snapshot.'
- Description changed from 'Return the background task registry with progress, retry counts, and assigned workers.' to 'Background tasks status snapshot.'
- Response now sets `version`, `ETag: "state-tasks-v<version>"`, and `Cache-Control: private, max-age=2` for conditional polling.

GET /state/training/telemetry
- Summary changed from 'Inspect training telemetry' to 'Training telemetry snapshot.'
- Description changed from 'Return aggregated Training Park telemetry, including success ratios, recall, and coverage metrics.' to 'Training telemetry snapshot.'

GET /state/world
- Summary changed from 'Inspect world model' to 'Project world model snapshot (belief graph view).'
- Description changed from 'Return the active world graph snapshot with claims, provenance metadata, and belief relationships.' to 'Project world model snapshot (belief graph view).'
- Response now sets `version`, `ETag: "state-world-v<version>"`, and `Cache-Control: private, max-age=2` for conditional polling.

GET /state/egress
- Response now sets `version`, `ETag: "state-egress-v<version>"`, and `Cache-Control: private, max-age=2` for conditional polling.

GET /state/world/select
- Summary changed from 'Select world claims' to 'Select top-k claims for a query.'
- Description changed from 'Evaluate a query against the world graph and return the top-k claims that match the provided filters.' to 'Select top-k claims for a query.'

## AsyncAPI (Events)

```diff
--- asyncapi.base.yaml
+++ asyncapi.head.yaml
@@ -94,6 +94,86 @@
         payload:
           type: object
           additionalProperties: true
+  'autonomy.alert':
+    subscribe:
+      operationId: autonomy_alert_event
+      summary: "autonomy.alert event"
+      description: "Event published on 'autonomy.alert' channel."
+      message:
+        name: 'autonomy.alert'
+        payload:
+          type: object
+          additionalProperties: true
+  'autonomy.budget.close_to_limit':
+    subscribe:
+      operationId: autonomy_budget_close_to_limit_event
+      summary: "autonomy.budget.close_to_limit event"
+      description: "Event published on 'autonomy.budget.close_to_limit' channel."
+      message:
+        name: 'autonomy.budget.close_to_limit'
+        payload:
+          type: object
+          additionalProperties: true
+  'autonomy.budget.exhausted':
+    subscribe:
+      operationId: autonomy_budget_exhausted_event
+      summary: "autonomy.budget.exhausted event"
+      description: "Event published on 'autonomy.budget.exhausted' channel."
+      message:
+        name: 'autonomy.budget.exhausted'
+        payload:
+          type: object
+          additionalProperties: true
+  'autonomy.interrupt':
+    subscribe:
+      operationId: autonomy_interrupt_event
+      summary: "autonomy.interrupt event"
+      description: "Event published on 'autonomy.interrupt' channel."
+      message:
+        name: 'autonomy.interrupt'
+        payload:
+          type: object
+          additionalProperties: true
+  'autonomy.run.paused':
+    subscribe:
+      operationId: autonomy_run_paused_event
+      summary: "autonomy.run.paused event"
+      description: "Event published on 'autonomy.run.paused' channel."
+      message:
+        name: 'autonomy.run.paused'
+        payload:
+          type: object
+          additionalProperties: true
+  'autonomy.run.resumed':
+    subscribe:
+      operationId: autonomy_run_resumed_event
+      summary: "autonomy.run.resumed event"
+      description: "Event published on 'autonomy.run.resumed' channel."
+      message:
+        name: 'autonomy.run.resumed'
+        payload:
+          type: object
+          additionalProperties: true
+  'autonomy.run.started':
+    subscribe:
+      operationId: autonomy_run_started_event
+      summary: "autonomy.run.started event"
+      description: "Event published on 'autonomy.run.started' channel."
+      message:
+        name: 'autonomy.run.started'
+        payload:
+          type: object
+          additionalProperties: true
+  'autonomy.run.stopped':
+    subscribe:
+      operationId: autonomy_run_stopped_event
+      summary: "autonomy.run.stopped event"
+      description: "Event published on 'autonomy.run.stopped' channel."
+      message:
+        name: 'autonomy.run.stopped'
+        payload:
+          type: object
+          additionalProperties: true
   'beliefs.updated':
     subscribe:
       operationId: beliefs_updated_event
@@ -630,7 +710,10 @@
       summary: "models.download.progress event"
       description: "Event published on 'models.download.progress' channel."
       message:
-        $ref: '#/components/messages/models.download.progress'
+        name: 'models.download.progress'
+        payload:
+          type: object
+          additionalProperties: true
   'models.manifest.written':
     subscribe:
       operationId: models_manifest_written_event
@@ -861,6 +944,16 @@
         payload:
           type: object
           additionalProperties: true
+  'screenshots.ocr.completed':
+    subscribe:
+      operationId: screenshots_ocr_completed_event
+      summary: "screenshots.ocr.completed event"
+      description: "Event published on 'screenshots.ocr.completed' channel."
+      message:
+        name: 'screenshots.ocr.completed'
+        payload:
+          type: object
+          additionalProperties: true
   'self.model.proposed':
     subscribe:
       operationId: self_model_proposed_event
@@ -1141,125 +1234,3 @@
         payload:
           type: object
           additionalProperties: true
-components:
-  messages:
-    'models.download.progress':
-      name: 'models.download.progress'
-      title: "Models download progress"
-      summary: "Download lifecycle progress, errors, and budget hints."
-      contentType: application/json
-      correlationId:
-        description: "Correlation id shared across previews, progress, and ledger entries (when present)."
-        location: "$message.payload#/corr_id"
-      payload:
-        type: object
-        required:
-          - id
-        additionalProperties: true
-        properties:
-          id:
-            type: string
-            description: "Download identifier (model id or follower target)."
-          status:
-            type: string
-            description: "Lifecycle phase for this download entry."
-            enum:
-              - started
-              - preflight
-              - downloading
-              - resumed
-              - degraded
-              - complete
-              - error
-              - canceled
-              - no-active-job
-              - coalesced
-          code:
-            type: string
-            description: "Stable machine-readable code describing the current status."
-            enum:
-              - hash-guard
-              - skipped
-              - resumed
-              - http
-              - request-timeout
-              - resume-http-status
-              - resume-content-range
-              - idle-timeout
-              - io
-              - sha256_mismatch
-              - quota_exceeded
-              - disk_insufficient
-              - size_limit
-              - soft-budget
-              - hard-budget
-          error_code:
-            type: string
-            description: "When status is error, mirrors the failure code for quick filtering."
-          corr_id:
-            type: string
-            description: "Correlation id shared across previews, progress, and ledger entries."
-          downloaded:
-            type: integer
-            format: int64
-            description: "Bytes persisted so far for this download attempt."
-          bytes:
-            type: integer
-            format: int64
-            description: "Alias for downloaded bytes retained for backwards compatibility."
-          total:
-            type: integer
-            format: int64
-            description: "Total expected bytes when advertised by the source."
-          percent:
-            type: number
-            format: double
-            minimum: 0
-            maximum: 100
-            description: "Percent complete when total is known."
-          url:
-            type: string
-            description: "Source URL (redacted to drop secrets for logs/previews)."
-          mode:
-            type: string
-            description: "Preflight/coalescing mode indicator (e.g., ok, skip, coalesced)."
-          primary:
-            type: string
-            description: "Primary download id when this entry was coalesced behind it."
-          content_length:
-            type: integer
-            format: int64
-            description: "Content length advertised during preflight."
-          etag:
-            type: string
-            description: "Source ETag observed during preflight (if provided)."
-          last_modified:
-            type: string
-            description: "Last-Modified header captured during preflight."
-          reason:
-            type: string
-            description: "Reason string for skipped preflight validations."
-          offset:
-            type: integer
-            format: int64
-            description: "Resume offset applied when a partial download continued."
-          sha256:
-            type: string
-            description: "Computed SHA-256 digest once the download completes."
-          cached:
-            type: boolean
-            description: "Whether the completed download reused an existing CAS entry."
-          source:
-            type: string
-            description: "Source indicator for coalesced completions (e.g., coalesced)."
-          error:
-            type: string
-            description: "Human-readable error message when status is error."
-          budget:
-            type: object
-            description: "Soft/hard budget snapshot with elapsed timings."
-            additionalProperties: true
-          disk:
-            type: object
-            description: "Disk utilization snapshot ({reserve, available, need})."
-            additionalProperties: true
```

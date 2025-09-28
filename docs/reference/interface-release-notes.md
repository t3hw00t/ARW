---
title: Interface Release Notes
---

# Interface Release Notes
Updated: 2025-09-28
Type: Reference

Base: `origin/main`

## OpenAPI (REST)

No changes

## AsyncAPI (Events)

```diff
--- asyncapi.base.yaml
+++ asyncapi.head.yaml
@@ -3,14 +3,22 @@
   title: "arw-server events"
   version: "0.1.4"
   description: "Normalized dot.case event channels for the unified server."
+  contact:
+    name: "ARW Project"
+    url: "https://github.com/t3hw00t/ARW"
+    email: "opensource@example.com"
   license:
     name: "MIT OR Apache-2.0"
+tags:
+  - name: "Events"
+    description: "Event channels emitted by the unified server."
 defaultContentType: application/json
 channels:
   'actions.applied':
     subscribe:
       operationId: actions_applied_event
       summary: "actions.applied event"
+      description: "Event published on 'actions.applied' channel."
       message:
         name: 'actions.applied'
         payload:
@@ -20,6 +28,7 @@
     subscribe:
       operationId: actions_completed_event
       summary: "actions.completed event"
+      description: "Event published on 'actions.completed' channel."
       message:
         name: 'actions.completed'
         payload:
@@ -29,6 +38,7 @@
     subscribe:
       operationId: actions_failed_event
       summary: "actions.failed event"
+      description: "Event published on 'actions.failed' channel."
       message:
         name: 'actions.failed'
         payload:
@@ -38,6 +48,7 @@
     subscribe:
       operationId: actions_hint_applied_event
       summary: "actions.hint.applied event"
+      description: "Event published on 'actions.hint.applied' channel."
       message:
         name: 'actions.hint.applied'
         payload:
@@ -47,6 +58,7 @@
     subscribe:
       operationId: actions_running_event
       summary: "actions.running event"
+      description: "Event published on 'actions.running' channel."
       message:
         name: 'actions.running'
         payload:
@@ -56,6 +68,7 @@
     subscribe:
       operationId: actions_submitted_event
       summary: "actions.submitted event"
+      description: "Event published on 'actions.submitted' channel."
       message:
         name: 'actions.submitted'
         payload:
@@ -65,6 +78,7 @@
     subscribe:
       operationId: actions_updated_event
       summary: "actions.updated event"
+      description: "Event published on 'actions.updated' channel."
       message:
         name: 'actions.updated'
         payload:
@@ -74,6 +88,7 @@
     subscribe:
       operationId: apps_vscode_opened_event
       summary: "apps.vscode.opened event"
+      description: "Event published on 'apps.vscode.opened' channel."
       message:
         name: 'apps.vscode.opened'
         payload:
@@ -83,6 +98,7 @@
     subscribe:
       operationId: beliefs_updated_event
       summary: "beliefs.updated event"
+      description: "Event published on 'beliefs.updated' channel."
       message:
         name: 'beliefs.updated'
         payload:
@@ -92,6 +108,7 @@
     subscribe:
       operationId: catalog_updated_event
       summary: "catalog.updated event"
+      description: "Event published on 'catalog.updated' channel."
       message:
         name: 'catalog.updated'
         payload:
@@ -101,6 +118,7 @@
     subscribe:
       operationId: chat_message_event
       summary: "chat.message event"
+      description: "Event published on 'chat.message' channel."
       message:
         name: 'chat.message'
         payload:
@@ -110,6 +128,7 @@
     subscribe:
       operationId: chat_planner_event
       summary: "chat.planner event"
+      description: "Event published on 'chat.planner' channel."
       message:
         name: 'chat.planner'
         payload:
@@ -119,6 +138,7 @@
     subscribe:
       operationId: chat_probe_event
       summary: "chat.probe event"
+      description: "Event published on 'chat.probe' channel."
       message:
         name: 'chat.probe'
         payload:
@@ -128,6 +148,7 @@
     subscribe:
       operationId: cluster_node_advertise_event
       summary: "cluster.node.advertise event"
+      description: "Event published on 'cluster.node.advertise' channel."
       message:
         name: 'cluster.node.advertise'
         payload:
@@ -137,6 +158,7 @@
     subscribe:
       operationId: cluster_node_changed_event
       summary: "cluster.node.changed event"
+      description: "Event published on 'cluster.node.changed' channel."
       message:
         name: 'cluster.node.changed'
         payload:
@@ -146,6 +168,7 @@
     subscribe:
       operationId: config_patch_applied_event
       summary: "config.patch.applied event"
+      description: "Event published on 'config.patch.applied' channel."
       message:
         name: 'config.patch.applied'
         payload:
@@ -155,6 +178,7 @@
     subscribe:
       operationId: connectors_registered_event
       summary: "connectors.registered event"
+      description: "Event published on 'connectors.registered' channel."
       message:
         name: 'connectors.registered'
         payload:
@@ -164,6 +188,7 @@
     subscribe:
       operationId: connectors_token_updated_event
       summary: "connectors.token.updated event"
+      description: "Event published on 'connectors.token.updated' channel."
       message:
         name: 'connectors.token.updated'
         payload:
@@ -173,6 +198,7 @@
     subscribe:
       operationId: context_assembled_event
       summary: "context.assembled event"
+      description: "Event published on 'context.assembled' channel."
       message:
         name: 'context.assembled'
         payload:
@@ -182,15 +208,27 @@
     subscribe:
       operationId: context_coverage_event
       summary: "context.coverage event"
+      description: "Event published on 'context.coverage' channel."
       message:
         name: 'context.coverage'
         payload:
           type: object
           additionalProperties: true
+  'context.recall.risk':
+    subscribe:
+      operationId: context_recall_risk_event
+      summary: "context.recall.risk event"
+      description: "Event published on 'context.recall.risk' channel."
+      message:
+        name: 'context.recall.risk'
+        payload:
+          type: object
+          additionalProperties: true
   'distill.completed':
     subscribe:
       operationId: distill_completed_event
       summary: "distill.completed event"
+      description: "Event published on 'distill.completed' channel."
       message:
         name: 'distill.completed'
         payload:
@@ -200,6 +238,7 @@
     subscribe:
       operationId: egress_ledger_appended_event
       summary: "egress.ledger.appended event"
+      description: "Event published on 'egress.ledger.appended' channel."
       message:
         name: 'egress.ledger.appended'
         payload:
@@ -209,6 +248,7 @@
     subscribe:
       operationId: egress_preview_event
       summary: "egress.preview event"
+      description: "Event published on 'egress.preview' channel."
       message:
         name: 'egress.preview'
         payload:
@@ -218,6 +258,7 @@
     subscribe:
       operationId: egress_settings_updated_event
       summary: "egress.settings.updated event"
+      description: "Event published on 'egress.settings.updated' channel."
       message:
         name: 'egress.settings.updated'
         payload:
@@ -227,6 +268,7 @@
     subscribe:
       operationId: experiment_activated_event
       summary: "experiment.activated event"
+      description: "Event published on 'experiment.activated' channel."
       message:
         name: 'experiment.activated'
         payload:
@@ -236,6 +278,7 @@
     subscribe:
       operationId: experiment_completed_event
       summary: "experiment.completed event"
+      description: "Event published on 'experiment.completed' channel."
       message:
         name: 'experiment.completed'
         payload:
@@ -245,6 +288,7 @@
     subscribe:
       operationId: experiment_result_event
       summary: "experiment.result event"
+      description: "Event published on 'experiment.result' channel."
       message:
         name: 'experiment.result'
         payload:
@@ -254,6 +298,7 @@
     subscribe:
       operationId: experiment_started_event
       summary: "experiment.started event"
+      description: "Event published on 'experiment.started' channel."
       message:
         name: 'experiment.started'
         payload:
@@ -263,6 +308,7 @@
     subscribe:
       operationId: experiment_variant_chosen_event
       summary: "experiment.variant.chosen event"
+      description: "Event published on 'experiment.variant.chosen' channel."
       message:
         name: 'experiment.variant.chosen'
         payload:
@@ -272,6 +318,7 @@
     subscribe:
       operationId: experiment_winner_event
       summary: "experiment.winner event"
+      description: "Event published on 'experiment.winner' channel."
       message:
         name: 'experiment.winner'
         payload:
@@ -281,6 +328,7 @@
     subscribe:
       operationId: feedback_applied_event
       summary: "feedback.applied event"
+      description: "Event published on 'feedback.applied' channel."
       message:
         name: 'feedback.applied'
         payload:
@@ -290,6 +338,7 @@
     subscribe:
       operationId: feedback_signal_event
       summary: "feedback.signal event"
+      description: "Event published on 'feedback.signal' channel."
       message:
         name: 'feedback.signal'
         payload:
@@ -299,6 +348,7 @@
     subscribe:
       operationId: feedback_suggested_event
       summary: "feedback.suggested event"
+      description: "Event published on 'feedback.suggested' channel."
       message:
         name: 'feedback.suggested'
         payload:
@@ -308,6 +358,7 @@
     subscribe:
       operationId: feedback_updated_event
       summary: "feedback.updated event"
+      description: "Event published on 'feedback.updated' channel."
       message:
         name: 'feedback.updated'
         payload:
@@ -317,6 +368,7 @@
     subscribe:
       operationId: goldens_evaluated_event
       summary: "goldens.evaluated event"
+      description: "Event published on 'goldens.evaluated' channel."
       message:
         name: 'goldens.evaluated'
         payload:
@@ -326,6 +378,7 @@
     subscribe:
       operationId: governor_changed_event
       summary: "governor.changed event"
+      description: "Event published on 'governor.changed' channel."
       message:
         name: 'governor.changed'
         payload:
@@ -335,6 +388,7 @@
     subscribe:
       operationId: hierarchy_accepted_event
       summary: "hierarchy.accepted event"
+      description: "Event published on 'hierarchy.accepted' channel."
       message:
         name: 'hierarchy.accepted'
         payload:
@@ -344,6 +398,7 @@
     subscribe:
       operationId: hierarchy_hello_event
       summary: "hierarchy.hello event"
+      description: "Event published on 'hierarchy.hello' channel."
       message:
         name: 'hierarchy.hello'
         payload:
@@ -353,6 +408,7 @@
     subscribe:
       operationId: hierarchy_offer_event
       summary: "hierarchy.offer event"
+      description: "Event published on 'hierarchy.offer' channel."
       message:
         name: 'hierarchy.offer'
         payload:
@@ -362,6 +418,7 @@
     subscribe:
       operationId: hierarchy_role_changed_event
       summary: "hierarchy.role.changed event"
+      description: "Event published on 'hierarchy.role.changed' channel."
       message:
         name: 'hierarchy.role.changed'
         payload:
@@ -371,6 +428,7 @@
     subscribe:
       operationId: hierarchy_state_event
       summary: "hierarchy.state event"
+      description: "Event published on 'hierarchy.state' channel."
       message:
         name: 'hierarchy.state'
         payload:
@@ -380,6 +438,7 @@
     subscribe:
       operationId: intents_approved_event
       summary: "intents.approved event"
+      description: "Event published on 'intents.approved' channel."
       message:
         name: 'intents.approved'
         payload:
@@ -389,6 +448,7 @@
     subscribe:
       operationId: intents_proposed_event
       summary: "intents.proposed event"
+      description: "Event published on 'intents.proposed' channel."
       message:
         name: 'intents.proposed'
         payload:
@@ -398,6 +458,7 @@
     subscribe:
       operationId: intents_rejected_event
       summary: "intents.rejected event"
+      description: "Event published on 'intents.rejected' channel."
       message:
         name: 'intents.rejected'
         payload:
@@ -407,6 +468,7 @@
     subscribe:
       operationId: leases_created_event
       summary: "leases.created event"
+      description: "Event published on 'leases.created' channel."
       message:
         name: 'leases.created'
         payload:
@@ -416,6 +478,7 @@
     subscribe:
       operationId: logic_unit_applied_event
       summary: "logic.unit.applied event"
+      description: "Event published on 'logic.unit.applied' channel."
       message:
         name: 'logic.unit.applied'
         payload:
@@ -425,6 +488,7 @@
     subscribe:
       operationId: logic_unit_installed_event
       summary: "logic.unit.installed event"
+      description: "Event published on 'logic.unit.installed' channel."
       message:
         name: 'logic.unit.installed'
         payload:
@@ -434,6 +498,7 @@
     subscribe:
       operationId: logic_unit_reverted_event
       summary: "logic.unit.reverted event"
+      description: "Event published on 'logic.unit.reverted' channel."
       message:
         name: 'logic.unit.reverted'
         payload:
@@ -443,6 +508,7 @@
     subscribe:
       operationId: logic_unit_suggested_event
       summary: "logic.unit.suggested event"
+      description: "Event published on 'logic.unit.suggested' channel."
       message:
         name: 'logic.unit.suggested'
         payload:
@@ -452,6 +518,7 @@
     subscribe:
       operationId: memory_admitted_event
       summary: "memory.admitted event"
+      description: "Event published on 'memory.admitted' channel."
       message:
         name: 'memory.admitted'
         payload:
@@ -460,63 +527,18 @@
   'memory.applied':
     subscribe:
       operationId: memory_applied_event
-      summary: "memory.applied event (id, lane, key, tags[], hash, value, ptr, value_preview, value_bytes, source)"
+      summary: "memory.applied event"
+      description: "Event published on 'memory.applied' channel."
       message:
         name: 'memory.applied'
         payload:
           type: object
           additionalProperties: true
-          properties:
-            id:
-              type: string
-            lane:
-              type: string
-            kind:
-              type: string
-              nullable: true
-            key:
-              type: string
-              nullable: true
-            tags:
-              type: array
-              items:
-                type: string
-            hash:
-              type: string
-            score:
-              type: number
-              nullable: true
-            prob:
-              type: number
-              nullable: true
-            value:
-              description: Full memory payload (arbitrary JSON value)
-            value_preview:
-              type: string
-              description: Human-friendly snippet for dashboards
-            value_preview_truncated:
-              type: boolean
-            value_bytes:
-              type: integer
-              format: int64
-            ptr:
-              type: object
-              additionalProperties: true
-            source:
-              type: string
-            updated:
-              type: string
-              format: date-time
-            applied_at:
-              type: string
-              format: date-time
-          required:
-            - id
-            - lane
   'memory.item.expired':
     subscribe:
       operationId: memory_item_expired_event
       summary: "memory.item.expired event"
+      description: "Event published on 'memory.item.expired' channel."
       message:
         name: 'memory.item.expired'
         payload:
@@ -526,6 +548,7 @@
     subscribe:
       operationId: memory_item_upserted_event
       summary: "memory.item.upserted event"
+      description: "Event published on 'memory.item.upserted' channel."
       message:
         name: 'memory.item.upserted'
         payload:
@@ -535,6 +558,7 @@
     subscribe:
       operationId: memory_link_put_event
       summary: "memory.link.put event"
+      description: "Event published on 'memory.link.put' channel."
       message:
         name: 'memory.link.put'
         payload:
@@ -544,6 +568,7 @@
     subscribe:
       operationId: memory_pack_journaled_event
       summary: "memory.pack.journaled event"
+      description: "Event published on 'memory.pack.journaled' channel."
       message:
         name: 'memory.pack.journaled'
         payload:
@@ -553,6 +578,7 @@
     subscribe:
       operationId: memory_quarantined_event
       summary: "memory.quarantined event"
+      description: "Event published on 'memory.quarantined' channel."
       message:
         name: 'memory.quarantined'
         payload:
@@ -562,6 +588,7 @@
     subscribe:
       operationId: memory_record_put_event
       summary: "memory.record.put event"
+      description: "Event published on 'memory.record.put' channel."
       message:
         name: 'memory.record.put'
         payload:
@@ -571,6 +598,7 @@
     subscribe:
       operationId: models_cas_gc_event
       summary: "models.cas.gc event"
+      description: "Event published on 'models.cas.gc' channel."
       message:
         name: 'models.cas.gc'
         payload:
@@ -580,6 +608,7 @@
     subscribe:
       operationId: models_changed_event
       summary: "models.changed event"
+      description: "Event published on 'models.changed' channel."
       message:
         name: 'models.changed'
         payload:
@@ -589,6 +618,7 @@
     subscribe:
       operationId: models_concurrency_changed_event
       summary: "models.concurrency.changed event"
+      description: "Event published on 'models.concurrency.changed' channel."
       message:
         name: 'models.concurrency.changed'
         payload:
@@ -598,6 +628,7 @@
     subscribe:
       operationId: models_download_progress_event
       summary: "models.download.progress event"
+      description: "Event published on 'models.download.progress' channel."
       message:
         name: 'models.download.progress'
         payload:
@@ -607,6 +638,7 @@
     subscribe:
       operationId: models_manifest_written_event
       summary: "models.manifest.written event"
+      description: "Event published on 'models.manifest.written' channel."
       message:
         name: 'models.manifest.written'
         payload:
@@ -616,6 +648,7 @@
     subscribe:
       operationId: models_refreshed_event
       summary: "models.refreshed event"
+      description: "Event published on 'models.refreshed' channel."
       message:
         name: 'models.refreshed'
         payload:
@@ -625,6 +658,7 @@
     subscribe:
       operationId: orchestrator_job_completed_event
       summary: "orchestrator.job.completed event"
+      description: "Event published on 'orchestrator.job.completed' channel."
       message:
         name: 'orchestrator.job.completed'
         payload:
@@ -634,6 +668,7 @@
     subscribe:
       operationId: orchestrator_job_created_event
       summary: "orchestrator.job.created event"
+      description: "Event published on 'orchestrator.job.created' channel."
       message:
         name: 'orchestrator.job.created'
         payload:
@@ -643,6 +678,7 @@
     subscribe:
       operationId: orchestrator_job_progress_event
       summary: "orchestrator.job.progress event"
+      description: "Event published on 'orchestrator.job.progress' channel."
       message:
         name: 'orchestrator.job.progress'
         payload:
@@ -652,6 +688,7 @@
     subscribe:
       operationId: policy_capsule_applied_event
       summary: "policy.capsule.applied event"
+      description: "Event published on 'policy.capsule.applied' channel."
       message:
         name: 'policy.capsule.applied'
         payload:
@@ -661,6 +698,7 @@
     subscribe:
       operationId: policy_capsule_expired_event
       summary: "policy.capsule.expired event"
+      description: "Event published on 'policy.capsule.expired' channel."
       message:
         name: 'policy.capsule.expired'
         payload:
@@ -670,6 +708,7 @@
     subscribe:
       operationId: policy_capsule_failed_event
       summary: "policy.capsule.failed event"
+      description: "Event published on 'policy.capsule.failed' channel."
       message:
         name: 'policy.capsule.failed'
         payload:
@@ -679,6 +718,7 @@
     subscribe:
       operationId: policy_decision_event
       summary: "policy.decision event"
+      description: "Event published on 'policy.decision' channel."
       message:
         name: 'policy.decision'
         payload:
@@ -688,6 +728,7 @@
     subscribe:
       operationId: policy_reloaded_event
       summary: "policy.reloaded event"
+      description: "Event published on 'policy.reloaded' channel."
       message:
         name: 'policy.reloaded'
         payload:
@@ -697,6 +738,7 @@
     subscribe:
       operationId: probe_hw_event
       summary: "probe.hw event"
+      description: "Event published on 'probe.hw' channel."
       message:
         name: 'probe.hw'
         payload:
@@ -706,6 +748,7 @@
     subscribe:
       operationId: probe_metrics_event
       summary: "probe.metrics event"
+      description: "Event published on 'probe.metrics' channel."
       message:
         name: 'probe.metrics'
         payload:
@@ -715,6 +758,7 @@
     subscribe:
       operationId: projects_created_event
       summary: "projects.created event"
+      description: "Event published on 'projects.created' channel."
       message:
         name: 'projects.created'
         payload:
@@ -724,6 +768,7 @@
     subscribe:
       operationId: projects_file_written_event
       summary: "projects.file.written event"
+      description: "Event published on 'projects.file.written' channel."
       message:
         name: 'projects.file.written'
         payload:
@@ -733,6 +778,7 @@
     subscribe:
       operationId: projects_notes_saved_event
       summary: "projects.notes.saved event"
+      description: "Event published on 'projects.notes.saved' channel."
       message:
         name: 'projects.notes.saved'
         payload:
@@ -742,6 +788,7 @@
     subscribe:
       operationId: research_watcher_updated_event
       summary: "research.watcher.updated event"
+      description: "Event published on 'research.watcher.updated' channel."
       message:
         name: 'research.watcher.updated'
         payload:
@@ -751,15 +798,67 @@
     subscribe:
       operationId: rpu_trust_changed_event
       summary: "rpu.trust.changed event"
+      description: "Event published on 'rpu.trust.changed' channel."
       message:
         name: 'rpu.trust.changed'
         payload:
           type: object
           additionalProperties: true
+  'runtime.claim.acquired':
+    subscribe:
+      operationId: runtime_claim_acquired_event
+      summary: "runtime.claim.acquired event"
+      description: "Event published on 'runtime.claim.acquired' channel."
+      message:
+        name: 'runtime.claim.acquired'
+        payload:
+          type: object
+          additionalProperties: true
+  'runtime.claim.released':
+    subscribe:
+      operationId: runtime_claim_released_event
+      summary: "runtime.claim.released event"
+      description: "Event published on 'runtime.claim.released' channel."
+      message:
+        name: 'runtime.claim.released'
+        payload:
+          type: object
+          additionalProperties: true
+  'runtime.claim.request':
+    subscribe:
+      operationId: runtime_claim_request_event
+      summary: "runtime.claim.request event"
+      description: "Event published on 'runtime.claim.request' channel."
+      message:
+        name: 'runtime.claim.request'
+        payload:
+          type: object
+          additionalProperties: true
+  'runtime.health':
+    subscribe:
+      operationId: runtime_health_event
+      summary: "runtime.health event"
+      description: "Event published on 'runtime.health' channel."
+      message:
+        name: 'runtime.health'
+        payload:
+          type: object
+          additionalProperties: true
+  'runtime.state.changed':
+    subscribe:
+      operationId: runtime_state_changed_event
+      summary: "runtime.state.changed event"
+      description: "Event published on 'runtime.state.changed' channel."
+      message:
+        name: 'runtime.state.changed'
+        payload:
+          type: object
+          additionalProperties: true
   'screenshots.captured':
     subscribe:
       operationId: screenshots_captured_event
       summary: "screenshots.captured event"
+      description: "Event published on 'screenshots.captured' channel."
       message:
         name: 'screenshots.captured'
         payload:
@@ -769,6 +868,7 @@
     subscribe:
       operationId: self_model_proposed_event
       summary: "self.model.proposed event"
+      description: "Event published on 'self.model.proposed' channel."
       message:
         name: 'self.model.proposed'
         payload:
@@ -778,6 +878,7 @@
     subscribe:
       operationId: self_model_updated_event
       summary: "self.model.updated event"
+      description: "Event published on 'self.model.updated' channel."
       message:
         name: 'self.model.updated'
         payload:
@@ -787,6 +888,7 @@
     subscribe:
       operationId: service_connected_event
       summary: "service.connected event"
+      description: "Event published on 'service.connected' channel."
       message:
         name: 'service.connected'
         payload:
@@ -796,6 +898,7 @@
     subscribe:
       operationId: service_health_event
       summary: "service.health event"
+      description: "Event published on 'service.health' channel."
       message:
         name: 'service.health'
         payload:
@@ -805,6 +908,7 @@
     subscribe:
       operationId: service_start_event
       summary: "service.start event"
+      description: "Event published on 'service.start' channel."
       message:
         name: 'service.start'
         payload:
@@ -814,6 +918,7 @@
     subscribe:
       operationId: service_stop_event
       summary: "service.stop event"
+      description: "Event published on 'service.stop' channel."
       message:
         name: 'service.stop'
         payload:
@@ -823,6 +928,7 @@
     subscribe:
       operationId: service_test_event
       summary: "service.test event"
+      description: "Event published on 'service.test' channel."
       message:
         name: 'service.test'
         payload:
@@ -832,6 +938,7 @@
     subscribe:
       operationId: snappy_detail_event
       summary: "snappy.detail event"
+      description: "Event published on 'snappy.detail' channel."
       message:
         name: 'snappy.detail'
         payload:
@@ -841,6 +948,7 @@
     subscribe:
       operationId: snappy_notice_event
       summary: "snappy.notice event"
+      description: "Event published on 'snappy.notice' channel."
       message:
         name: 'snappy.notice'
         payload:
@@ -850,6 +958,7 @@
     subscribe:
       operationId: staging_decided_event
       summary: "staging.decided event"
+      description: "Event published on 'staging.decided' channel."
       message:
         name: 'staging.decided'
         payload:
@@ -859,6 +968,7 @@
     subscribe:
       operationId: staging_pending_event
       summary: "staging.pending event"
+      description: "Event published on 'staging.pending' channel."
       message:
         name: 'staging.pending'
         payload:
@@ -868,6 +978,7 @@
     subscribe:
       operationId: state_read_model_patch_event
       summary: "state.read.model.patch event"
+      description: "Event published on 'state.read.model.patch' channel."
       message:
         name: 'state.read.model.patch'
         payload:
@@ -877,6 +988,7 @@
     subscribe:
       operationId: task_completed_event
       summary: "task.completed event"
+      description: "Event published on 'task.completed' channel."
       message:
         name: 'task.completed'
         payload:
@@ -886,6 +998,7 @@
     subscribe:
       operationId: tool_cache_event
       summary: "tool.cache event"
+      description: "Event published on 'tool.cache' channel."
       message:
         name: 'tool.cache'
         payload:
@@ -895,6 +1008,7 @@
     subscribe:
       operationId: tool_ran_event
       summary: "tool.ran event"
+      description: "Event published on 'tool.ran' channel."
       message:
         name: 'tool.ran'
         payload:
@@ -904,6 +1018,7 @@
     subscribe:
       operationId: training_metrics_updated_event
       summary: "training.metrics.updated event"
+      description: "Event published on 'training.metrics.updated' channel."
       message:
         name: 'training.metrics.updated'
         payload:
@@ -913,6 +1028,7 @@
     subscribe:
       operationId: working_set_completed_event
       summary: "working_set.completed event"
+      description: "Event published on 'working_set.completed' channel."
       message:
         name: 'working_set.completed'
         payload:
@@ -922,6 +1038,7 @@
     subscribe:
       operationId: working_set_error_event
       summary: "working_set.error event"
+      description: "Event published on 'working_set.error' channel."
       message:
         name: 'working_set.error'
         payload:
@@ -931,6 +1048,7 @@
     subscribe:
       operationId: working_set_expand_query_event
       summary: "working_set.expand_query event"
+      description: "Event published on 'working_set.expand_query' channel."
       message:
         name: 'working_set.expand_query'
         payload:
@@ -940,6 +1058,7 @@
     subscribe:
       operationId: working_set_expanded_event
       summary: "working_set.expanded event"
+      description: "Event published on 'working_set.expanded' channel."
       message:
         name: 'working_set.expanded'
         payload:
@@ -949,6 +1068,7 @@
     subscribe:
       operationId: working_set_iteration_summary_event
       summary: "working_set.iteration.summary event"
+      description: "Event published on 'working_set.iteration.summary' channel."
       message:
         name: 'working_set.iteration.summary'
         payload:
@@ -958,6 +1078,7 @@
     subscribe:
       operationId: working_set_seed_event
       summary: "working_set.seed event"
+      description: "Event published on 'working_set.seed' channel."
       message:
         name: 'working_set.seed'
         payload:
@@ -967,6 +1088,7 @@
     subscribe:
       operationId: working_set_selected_event
       summary: "working_set.selected event"
+      description: "Event published on 'working_set.selected' channel."
       message:
         name: 'working_set.selected'
         payload:
@@ -976,6 +1098,7 @@
     subscribe:
       operationId: working_set_started_event
       summary: "working_set.started event"
+      description: "Event published on 'working_set.started' channel."
       message:
         name: 'working_set.started'
         payload:
@@ -985,6 +1108,7 @@
     subscribe:
       operationId: world_diff_applied_event
       summary: "world.diff.applied event"
+      description: "Event published on 'world.diff.applied' channel."
       message:
         name: 'world.diff.applied'
         payload:
@@ -994,6 +1118,7 @@
     subscribe:
       operationId: world_diff_queued_event
       summary: "world.diff.queued event"
+      description: "Event published on 'world.diff.queued' channel."
       message:
         name: 'world.diff.queued'
         payload:
@@ -1003,6 +1128,7 @@
     subscribe:
       operationId: world_diff_rejected_event
       summary: "world.diff.rejected event"
+      description: "Event published on 'world.diff.rejected' channel."
       message:
         name: 'world.diff.rejected'
         payload:
@@ -1012,6 +1138,7 @@
     subscribe:
       operationId: world_updated_event
       summary: "world.updated event"
+      description: "Event published on 'world.updated' channel."
       message:
         name: 'world.updated'
         payload:
```


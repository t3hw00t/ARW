---
title: Event Topics (Canonical)
---

# Event Topics (Canonical)
Updated: 2025-10-19
Type: Reference

Source of truth for event kinds published by the service. Generated from
[`crates/arw-topics/src/lib.rs`](https://github.com/t3hw00t/ARW/blob/main/crates/arw-topics/src/lib.rs)
so code and docs stay in sync. Every topic follows dot.case naming; use the
linked source or the Feature Matrix for detailed semantics.

Related docs:
- Explanations → [Events Vocabulary](../architecture/events_vocabulary.md)
- Architecture → [SSE + JSON Patch Contract](../architecture/sse_patch_contract.md)
- How-to → [Subscribe to Events (SSE)](../guide/events_sse.md)
- How-to → [Models Download (HTTP)](../guide/models_download.md)

## Models / downloads

| Topic | Constant |
|-------|----------|
| `models.download.progress` | `TOPIC_PROGRESS` |
| `models.changed` | `TOPIC_MODELS_CHANGED` |
| `models.manifest.written` | `TOPIC_MODELS_MANIFEST_WRITTEN` |
| `egress.preview` | `TOPIC_EGRESS_PREVIEW` |
| `egress.ledger.appended` | `TOPIC_EGRESS_LEDGER_APPENDED` |
| `models.concurrency.changed` | `TOPIC_CONCURRENCY_CHANGED` |
| `state.read.model.patch` | `TOPIC_READMODEL_PATCH` |
| `models.refreshed` | `TOPIC_MODELS_REFRESHED` |
| `models.cas.gc` | `TOPIC_MODELS_CAS_GC` |
| `research.watcher.updated` | `TOPIC_RESEARCH_WATCHER_UPDATED` |
| `training.metrics.updated` | `TOPIC_TRAINING_METRICS_UPDATED` |
| `staging.pending` | `TOPIC_STAGING_PENDING` |
| `staging.decided` | `TOPIC_STAGING_DECIDED` |

## Interactive performance (snappy)

| Topic | Constant |
|-------|----------|
| `snappy.notice` | `TOPIC_SNAPPY_NOTICE` |
| `snappy.detail` | `TOPIC_SNAPPY_DETAIL` |

## Chat / Debug chat

| Topic | Constant |
|-------|----------|
| `chat.message` | `TOPIC_CHAT_MESSAGE` |
| `chat.planner` | `TOPIC_CHAT_PLANNER` |
| `chat.probe` | `TOPIC_CHAT_PROBE` |

## Memory / Review

| Topic | Constant |
|-------|----------|
| `memory.applied` | `TOPIC_MEMORY_APPLIED` |
| `memory.quarantined` | `TOPIC_MEMORY_QUARANTINED` |
| `memory.admitted` | `TOPIC_MEMORY_ADMITTED` |
| `memory.record.put` | `TOPIC_MEMORY_RECORD_PUT` |
| `memory.link.put` | `TOPIC_MEMORY_LINK_PUT` |
| `memory.item.upserted` | `TOPIC_MEMORY_ITEM_UPSERTED` |
| `memory.item.expired` | `TOPIC_MEMORY_ITEM_EXPIRED` |
| `memory.pack.journaled` | `TOPIC_MEMORY_PACK_JOURNALED` |
| `story.thread.updated` | `TOPIC_STORY_THREAD_UPDATED` |

## Beliefs (read-model summaries)

| Topic | Constant |
|-------|----------|
| `beliefs.updated` | `TOPIC_BELIEFS_UPDATED` |

## Feedback engine

| Topic | Constant |
|-------|----------|
| `feedback.suggested` | `TOPIC_FEEDBACK_SUGGESTED` |
| `feedback.updated` | `TOPIC_FEEDBACK_UPDATED` |
| `feedback.delta` | `TOPIC_FEEDBACK_DELTA` |

## Projects

| Topic | Constant |
|-------|----------|
| `projects.created` | `TOPIC_PROJECTS_CREATED` |
| `projects.notes.saved` | `TOPIC_PROJECTS_NOTES_SAVED` |
| `projects.file.written` | `TOPIC_PROJECTS_FILE_WRITTEN` |
| `projects.snapshot.created` | `TOPIC_PROJECTS_SNAPSHOT_CREATED` |
| `projects.snapshot.restored` | `TOPIC_PROJECTS_SNAPSHOT_RESTORED` |

## Experiments

| Topic | Constant |
|-------|----------|
| `experiment.result` | `TOPIC_EXPERIMENT_RESULT` |
| `experiment.winner` | `TOPIC_EXPERIMENT_WINNER` |
| `experiment.started` | `TOPIC_EXPERIMENT_STARTED` |
| `experiment.completed` | `TOPIC_EXPERIMENT_COMPLETED` |
| `experiment.variant.chosen` | `TOPIC_EXPERIMENT_VARIANT_CHOSEN` |
| `experiment.activated` | `TOPIC_EXPERIMENT_ACTIVATED` |

## Self-model & world diffs (review)

| Topic | Constant |
|-------|----------|
| `self.model.proposed` | `TOPIC_SELFMODEL_PROPOSED` |
| `self.model.updated` | `TOPIC_SELFMODEL_UPDATED` |
| `world.diff.queued` | `TOPIC_WORLDDIFF_QUEUED` |
| `world.diff.applied` | `TOPIC_WORLDDIFF_APPLIED` |
| `world.diff.rejected` | `TOPIC_WORLDDIFF_REJECTED` |

## Hierarchy / Cluster

| Topic | Constant |
|-------|----------|
| `hierarchy.hello` | `TOPIC_HIERARCHY_HELLO` |
| `hierarchy.offer` | `TOPIC_HIERARCHY_OFFER` |
| `hierarchy.accepted` | `TOPIC_HIERARCHY_ACCEPTED` |
| `hierarchy.state` | `TOPIC_HIERARCHY_STATE` |
| `hierarchy.role.changed` | `TOPIC_HIERARCHY_ROLE_CHANGED` |
| `cluster.node.advertise` | `TOPIC_CLUSTER_NODE_ADVERTISE` |
| `cluster.node.changed` | `TOPIC_CLUSTER_NODE_CHANGED` |

## Governor / Actions

| Topic | Constant |
|-------|----------|
| `governor.changed` | `TOPIC_GOVERNOR_CHANGED` |
| `actions.hint.applied` | `TOPIC_ACTIONS_HINT_APPLIED` |
| `actions.running` | `TOPIC_ACTIONS_RUNNING` |
| `actions.completed` | `TOPIC_ACTIONS_COMPLETED` |
| `actions.failed` | `TOPIC_ACTIONS_FAILED` |
| `actions.updated` | `TOPIC_ACTIONS_UPDATED` |

## Autonomy

| Topic | Constant |
|-------|----------|
| `autonomy.run.started` | `TOPIC_AUTONOMY_RUN_STARTED` |
| `autonomy.run.paused` | `TOPIC_AUTONOMY_RUN_PAUSED` |
| `autonomy.run.resumed` | `TOPIC_AUTONOMY_RUN_RESUMED` |
| `autonomy.run.stopped` | `TOPIC_AUTONOMY_RUN_STOPPED` |
| `autonomy.interrupt` | `TOPIC_AUTONOMY_INTERRUPT` |
| `autonomy.alert` | `TOPIC_AUTONOMY_ALERT` |
| `autonomy.budget.close_to_limit` | `TOPIC_AUTONOMY_BUDGET_CLOSE` |
| `autonomy.budget.exhausted` | `TOPIC_AUTONOMY_BUDGET_EXHAUSTED` |
| `autonomy.budget.updated` | `TOPIC_AUTONOMY_BUDGET_UPDATED` |

## Service lifecycle and misc

| Topic | Constant |
|-------|----------|
| `service.connected` | `TOPIC_SERVICE_CONNECTED` |
| `service.start` | `TOPIC_SERVICE_START` |
| `service.health` | `TOPIC_SERVICE_HEALTH` |
| `service.test` | `TOPIC_SERVICE_TEST` |
| `service.stop` | `TOPIC_SERVICE_STOP` |
| `probe.hw` | `TOPIC_PROBE_HW` |
| `probe.metrics` | `TOPIC_PROBE_METRICS` |
| `catalog.updated` | `TOPIC_CATALOG_UPDATED` |
| `identity.registry.reloaded` | `TOPIC_IDENTITY_RELOADED` |
| `config.patch.applied` | `TOPIC_CONFIG_PATCH_APPLIED` |
| `config.reloaded` | `TOPIC_CONFIG_RELOADED` |
| `policy.gating.reloaded` | `TOPIC_GATING_RELOADED` |
| `cache.policy.reloaded` | `TOPIC_CACHE_POLICY_RELOADED` |
| `policy.decision` | `TOPIC_POLICY_DECISION` |
| `policy.reloaded` | `TOPIC_POLICY_RELOADED` |
| `policy.capsule.applied` | `TOPIC_POLICY_CAPSULE_APPLIED` |
| `policy.capsule.failed` | `TOPIC_POLICY_CAPSULE_FAILED` |
| `policy.capsule.expired` | `TOPIC_POLICY_CAPSULE_EXPIRED` |
| `policy.capsule.teardown` | `TOPIC_POLICY_CAPSULE_TEARDOWN` |
| `policy.guardrails.applied` | `TOPIC_POLICY_GUARDRAILS_APPLIED` |
| `leases.created` | `TOPIC_LEASES_CREATED` |

## Apps / desktop integration

| Topic | Constant |
|-------|----------|
| `apps.vscode.opened` | `TOPIC_APPS_VSCODE_OPENED` |

## Orchestrator

| Topic | Constant |
|-------|----------|
| `task.completed` | `TOPIC_TASK_COMPLETED` |
| `orchestrator.job.created` | `TOPIC_ORCHESTRATOR_JOB_CREATED` |
| `orchestrator.job.progress` | `TOPIC_ORCHESTRATOR_JOB_PROGRESS` |
| `orchestrator.job.completed` | `TOPIC_ORCHESTRATOR_JOB_COMPLETED` |

## World model

| Topic | Constant |
|-------|----------|
| `world.updated` | `TOPIC_WORLD_UPDATED` |
| `world.telemetry` | `TOPIC_WORLD_TELEMETRY` |

## Context assembly

| Topic | Constant |
|-------|----------|
| `context.assembled` | `TOPIC_CONTEXT_ASSEMBLED` |
| `context.coverage` | `TOPIC_CONTEXT_COVERAGE` |
| `context.recall.risk` | `TOPIC_CONTEXT_RECALL_RISK` |
| `context.cascade.updated` | `TOPIC_CONTEXT_CASCADE_UPDATED` |
| `working_set.started` | `TOPIC_WORKING_SET_STARTED` |
| `working_set.seed` | `TOPIC_WORKING_SET_SEED` |
| `working_set.expanded` | `TOPIC_WORKING_SET_EXPANDED` |
| `working_set.expand_query` | `TOPIC_WORKING_SET_EXPAND_QUERY` |
| `working_set.selected` | `TOPIC_WORKING_SET_SELECTED` |
| `working_set.completed` | `TOPIC_WORKING_SET_COMPLETED` |
| `working_set.iteration.summary` | `TOPIC_WORKING_SET_ITERATION_SUMMARY` |
| `working_set.error` | `TOPIC_WORKING_SET_ERROR` |

## Goldens

| Topic | Constant |
|-------|----------|
| `goldens.evaluated` | `TOPIC_GOLDENS_EVALUATED` |

## Logic Units

| Topic | Constant |
|-------|----------|
| `logic.unit.installed` | `TOPIC_LOGICUNIT_INSTALLED` |
| `logic.unit.applied` | `TOPIC_LOGICUNIT_APPLIED` |
| `logic.unit.reverted` | `TOPIC_LOGICUNIT_REVERTED` |
| `logic.unit.suggested` | `TOPIC_LOGICUNIT_SUGGESTED` |

## Intents & Actions (legacy CamelCase variants used in debug streams)

| Topic | Constant |
|-------|----------|
| `intents.proposed` | `TOPIC_INTENTS_PROPOSED` |
| `intents.approved` | `TOPIC_INTENTS_APPROVED` |
| `intents.rejected` | `TOPIC_INTENTS_REJECTED` |
| `actions.applied` | `TOPIC_ACTIONS_APPLIED` |
| `actions.submitted` | `TOPIC_ACTIONS_SUBMITTED` |

## Tools

| Topic | Constant |
|-------|----------|
| `tool.cache` | `TOPIC_TOOL_CACHE` |
| `tool.ran` | `TOPIC_TOOL_RAN` |

## Screenshots

| Topic | Constant |
|-------|----------|
| `screenshots.captured` | `TOPIC_SCREENSHOTS_CAPTURED` |
| `screenshots.ocr.completed` | `TOPIC_SCREENSHOTS_OCR_COMPLETED` |

## Modular stack

| Topic | Constant |
|-------|----------|
| `modular.agent.accepted` | `TOPIC_MODULAR_AGENT_ACCEPTED` |
| `modular.tool.accepted` | `TOPIC_MODULAR_TOOL_ACCEPTED` |

## Connectors & policy plane

| Topic | Constant |
|-------|----------|
| `connectors.registered` | `TOPIC_CONNECTORS_REGISTERED` |
| `connectors.token.updated` | `TOPIC_CONNECTORS_TOKEN_UPDATED` |
| `egress.settings.updated` | `TOPIC_EGRESS_SETTINGS_UPDATED` |

## Feedback misc signals

| Topic | Constant |
|-------|----------|
| `feedback.signal` | `TOPIC_FEEDBACK_SIGNAL` |
| `feedback.applied` | `TOPIC_FEEDBACK_APPLIED` |

## Distillation

| Topic | Constant |
|-------|----------|
| `distill.completed` | `TOPIC_DISTILL_COMPLETED` |

## RPU (trust store)

| Topic | Constant |
|-------|----------|
| `rpu.trust.changed` | `TOPIC_RPU_TRUST_CHANGED` |

## Personas

| Topic | Constant |
|-------|----------|
| `persona.feedback` | `TOPIC_PERSONA_FEEDBACK` |

## Runtimes / supervisor (alphabetized)

| Topic | Constant |
|-------|----------|
| `runtime.claim.acquired` | `TOPIC_RUNTIME_CLAIM_ACQUIRED` |
| `runtime.health` | `TOPIC_RUNTIME_HEALTH` |
| `runtime.claim.released` | `TOPIC_RUNTIME_CLAIM_RELEASED` |
| `runtime.claim.request` | `TOPIC_RUNTIME_CLAIM_REQUEST` |
| `runtime.state.changed` | `TOPIC_RUNTIME_STATE_CHANGED` |
| `runtime.restore.requested` | `TOPIC_RUNTIME_RESTORE_REQUESTED` |
| `runtime.restore.completed` | `TOPIC_RUNTIME_RESTORE_COMPLETED` |

<!-- Generated by scripts/gen_topics_doc.py -->

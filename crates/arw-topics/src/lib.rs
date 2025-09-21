//! Canonical event topic constants shared across services.
//!
//! This crate centralizes the string constants used when publishing events
//! so that both the legacy service and the unified server stay in sync.
//! Keep this list alphabetized within sections and favor dot.case names.

// Models / downloads
pub const TOPIC_PROGRESS: &str = "models.download.progress";
pub const TOPIC_MODELS_CHANGED: &str = "models.changed";
pub const TOPIC_MODELS_MANIFEST_WRITTEN: &str = "models.manifest.written";
pub const TOPIC_EGRESS_PREVIEW: &str = "egress.preview";
pub const TOPIC_EGRESS_LEDGER_APPENDED: &str = "egress.ledger.appended";
pub const TOPIC_CONCURRENCY_CHANGED: &str = "models.concurrency.changed";
pub const TOPIC_READMODEL_PATCH: &str = "state.read.model.patch";
pub const TOPIC_MODELS_REFRESHED: &str = "models.refreshed";
pub const TOPIC_MODELS_CAS_GC: &str = "models.cas.gc";
pub const TOPIC_RESEARCH_WATCHER_UPDATED: &str = "research.watcher.updated";
pub const TOPIC_TRAINING_METRICS_UPDATED: &str = "training.metrics.updated";
pub const TOPIC_STAGING_PENDING: &str = "staging.pending";
pub const TOPIC_STAGING_DECIDED: &str = "staging.decided";

// Interactive performance (snappy)
pub const TOPIC_SNAPPY_NOTICE: &str = "snappy.notice";
pub const TOPIC_SNAPPY_DETAIL: &str = "snappy.detail";

// Chat / Debug chat
pub const TOPIC_CHAT_MESSAGE: &str = "chat.message";
pub const TOPIC_CHAT_PLANNER: &str = "chat.planner";
pub const TOPIC_CHAT_PROBE: &str = "chat.probe";

// Memory / Review
pub const TOPIC_MEMORY_APPLIED: &str = "memory.applied";
pub const TOPIC_MEMORY_QUARANTINED: &str = "memory.quarantined";
pub const TOPIC_MEMORY_ADMITTED: &str = "memory.admitted";
pub const TOPIC_MEMORY_RECORD_PUT: &str = "memory.record.put";
pub const TOPIC_MEMORY_LINK_PUT: &str = "memory.link.put";
pub const TOPIC_MEMORY_ITEM_UPSERTED: &str = "memory.item.upserted";
pub const TOPIC_MEMORY_ITEM_EXPIRED: &str = "memory.item.expired";
pub const TOPIC_MEMORY_PACK_JOURNALED: &str = "memory.pack.journaled";

// Beliefs (read-model summaries)
pub const TOPIC_BELIEFS_UPDATED: &str = "beliefs.updated";

// Feedback engine
pub const TOPIC_FEEDBACK_SUGGESTED: &str = "feedback.suggested";
pub const TOPIC_FEEDBACK_UPDATED: &str = "feedback.updated";

// Projects
pub const TOPIC_PROJECTS_CREATED: &str = "projects.created";
pub const TOPIC_PROJECTS_NOTES_SAVED: &str = "projects.notes.saved";
pub const TOPIC_PROJECTS_FILE_WRITTEN: &str = "projects.file.written";

// Experiments
pub const TOPIC_EXPERIMENT_RESULT: &str = "experiment.result";
pub const TOPIC_EXPERIMENT_WINNER: &str = "experiment.winner";
pub const TOPIC_EXPERIMENT_STARTED: &str = "experiment.started";
pub const TOPIC_EXPERIMENT_COMPLETED: &str = "experiment.completed";
pub const TOPIC_EXPERIMENT_VARIANT_CHOSEN: &str = "experiment.variant.chosen";
pub const TOPIC_EXPERIMENT_ACTIVATED: &str = "experiment.activated";

// Self-model & world diffs (review)
pub const TOPIC_SELFMODEL_PROPOSED: &str = "self.model.proposed";
pub const TOPIC_SELFMODEL_UPDATED: &str = "self.model.updated";
pub const TOPIC_WORLDDIFF_QUEUED: &str = "world.diff.queued";
pub const TOPIC_WORLDDIFF_APPLIED: &str = "world.diff.applied";
pub const TOPIC_WORLDDIFF_REJECTED: &str = "world.diff.rejected";

// Hierarchy / Cluster
pub const TOPIC_HIERARCHY_HELLO: &str = "hierarchy.hello";
pub const TOPIC_HIERARCHY_OFFER: &str = "hierarchy.offer";
pub const TOPIC_HIERARCHY_ACCEPTED: &str = "hierarchy.accepted";
pub const TOPIC_HIERARCHY_STATE: &str = "hierarchy.state";
pub const TOPIC_HIERARCHY_ROLE_CHANGED: &str = "hierarchy.role.changed";
pub const TOPIC_CLUSTER_NODE_ADVERTISE: &str = "cluster.node.advertise";
pub const TOPIC_CLUSTER_NODE_CHANGED: &str = "cluster.node.changed";

// Governor / Actions
pub const TOPIC_GOVERNOR_CHANGED: &str = "governor.changed";
pub const TOPIC_ACTIONS_HINT_APPLIED: &str = "actions.hint.applied";
pub const TOPIC_ACTIONS_RUNNING: &str = "actions.running";
pub const TOPIC_ACTIONS_COMPLETED: &str = "actions.completed";
pub const TOPIC_ACTIONS_FAILED: &str = "actions.failed";
pub const TOPIC_ACTIONS_UPDATED: &str = "actions.updated";

// Service lifecycle and misc
pub const TOPIC_SERVICE_CONNECTED: &str = "service.connected";
pub const TOPIC_SERVICE_START: &str = "service.start";
pub const TOPIC_SERVICE_HEALTH: &str = "service.health";
pub const TOPIC_SERVICE_TEST: &str = "service.test";
pub const TOPIC_SERVICE_STOP: &str = "service.stop";
pub const TOPIC_PROBE_HW: &str = "probe.hw";
pub const TOPIC_PROBE_METRICS: &str = "probe.metrics";
pub const TOPIC_CATALOG_UPDATED: &str = "catalog.updated";
pub const TOPIC_CONFIG_PATCH_APPLIED: &str = "config.patch.applied";
pub const TOPIC_POLICY_DECISION: &str = "policy.decision";
pub const TOPIC_POLICY_RELOADED: &str = "policy.reloaded";
pub const TOPIC_POLICY_CAPSULE_APPLIED: &str = "policy.capsule.applied";
pub const TOPIC_POLICY_CAPSULE_FAILED: &str = "policy.capsule.failed";
pub const TOPIC_POLICY_CAPSULE_EXPIRED: &str = "policy.capsule.expired";

// Apps / desktop integration
pub const TOPIC_APPS_VSCODE_OPENED: &str = "apps.vscode.opened";

// Orchestrator
pub const TOPIC_TASK_COMPLETED: &str = "task.completed";
pub const TOPIC_ORCHESTRATOR_JOB_CREATED: &str = "orchestrator.job.created";
pub const TOPIC_ORCHESTRATOR_JOB_PROGRESS: &str = "orchestrator.job.progress";
pub const TOPIC_ORCHESTRATOR_JOB_COMPLETED: &str = "orchestrator.job.completed";

// World model
pub const TOPIC_WORLD_UPDATED: &str = "world.updated";

// Context assembly
pub const TOPIC_CONTEXT_ASSEMBLED: &str = "context.assembled";
pub const TOPIC_CONTEXT_COVERAGE: &str = "context.coverage";
pub const TOPIC_WORKING_SET_STARTED: &str = "working_set.started";
pub const TOPIC_WORKING_SET_SEED: &str = "working_set.seed";
pub const TOPIC_WORKING_SET_EXPANDED: &str = "working_set.expanded";
pub const TOPIC_WORKING_SET_EXPAND_QUERY: &str = "working_set.expand_query";
pub const TOPIC_WORKING_SET_SELECTED: &str = "working_set.selected";
pub const TOPIC_WORKING_SET_COMPLETED: &str = "working_set.completed";
pub const TOPIC_WORKING_SET_ITERATION_SUMMARY: &str = "working_set.iteration.summary";
pub const TOPIC_WORKING_SET_ERROR: &str = "working_set.error";

// Goldens
pub const TOPIC_GOLDENS_EVALUATED: &str = "goldens.evaluated";

// Logic Units
pub const TOPIC_LOGICUNIT_INSTALLED: &str = "logic.unit.installed";
pub const TOPIC_LOGICUNIT_APPLIED: &str = "logic.unit.applied";
pub const TOPIC_LOGICUNIT_REVERTED: &str = "logic.unit.reverted";
pub const TOPIC_LOGICUNIT_SUGGESTED: &str = "logic.unit.suggested";

// Intents & Actions (legacy CamelCase variants used in debug streams)
pub const TOPIC_INTENTS_PROPOSED: &str = "intents.proposed";
pub const TOPIC_INTENTS_APPROVED: &str = "intents.approved";
pub const TOPIC_INTENTS_REJECTED: &str = "intents.rejected";
pub const TOPIC_ACTIONS_APPLIED: &str = "actions.applied";
pub const TOPIC_ACTIONS_SUBMITTED: &str = "actions.submitted";

// Tools
pub const TOPIC_TOOL_CACHE: &str = "tool.cache";
pub const TOPIC_TOOL_RAN: &str = "tool.ran";

// Screenshots
pub const TOPIC_SCREENSHOTS_CAPTURED: &str = "screenshots.captured";

// Connectors & policy plane
pub const TOPIC_CONNECTORS_REGISTERED: &str = "connectors.registered";
pub const TOPIC_CONNECTORS_TOKEN_UPDATED: &str = "connectors.token.updated";
pub const TOPIC_EGRESS_SETTINGS_UPDATED: &str = "egress.settings.updated";

// Feedback misc signals
pub const TOPIC_FEEDBACK_SIGNAL: &str = "feedback.signal";
pub const TOPIC_FEEDBACK_APPLIED: &str = "feedback.applied";

// Distillation
pub const TOPIC_DISTILL_COMPLETED: &str = "distill.completed";

// RPU (trust store)
pub const TOPIC_RPU_TRUST_CHANGED: &str = "rpu.trust.changed";

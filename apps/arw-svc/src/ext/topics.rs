// Centralized event topic constants used across the service

pub const TOPIC_PROGRESS: &str = "models.download.progress";
pub const TOPIC_MODELS_CHANGED: &str = "models.changed";
pub const TOPIC_MODELS_MANIFEST_WRITTEN: &str = "models.manifest.written";
pub const TOPIC_EGRESS_PREVIEW: &str = "egress.preview";
pub const TOPIC_EGRESS_LEDGER_APPENDED: &str = "egress.ledger.appended";
pub const TOPIC_CONCURRENCY_CHANGED: &str = "models.concurrency.changed";
pub const TOPIC_READMODEL_PATCH: &str = "state.read.model.patch";
pub const TOPIC_MODELS_REFRESHED: &str = "models.refreshed";
pub const TOPIC_MODELS_CAS_GC: &str = "models.cas.gc";
// Snappy (latency budgets) topics
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

// Service lifecycle and misc
pub const TOPIC_SERVICE_START: &str = "service.start";
pub const TOPIC_SERVICE_HEALTH: &str = "service.health";
pub const TOPIC_SERVICE_TEST: &str = "service.test";
pub const TOPIC_SERVICE_STOP: &str = "service.stop";
pub const TOPIC_PROBE_HW: &str = "probe.hw";
pub const TOPIC_CATALOG_UPDATED: &str = "catalog.updated";

// Orchestrator
pub const TOPIC_TASK_COMPLETED: &str = "task.completed";

// World model
pub const TOPIC_WORLD_UPDATED: &str = "world.updated";

// Context assembly
pub const TOPIC_CONTEXT_ASSEMBLED: &str = "context.assembled";
pub const TOPIC_CONTEXT_COVERAGE: &str = "context.coverage";

// Goldens
pub const TOPIC_GOLDENS_EVALUATED: &str = "goldens.evaluated";

// Logic Units
pub const TOPIC_LOGICUNIT_INSTALLED: &str = "logic.unit.installed";
pub const TOPIC_LOGICUNIT_APPLIED: &str = "logic.unit.applied";
pub const TOPIC_LOGICUNIT_REVERTED: &str = "logic.unit.reverted";

// Intents & Actions (legacy CamelCase variants used in debug streams)
pub const TOPIC_INTENTS_PROPOSED: &str = "intents.proposed";
pub const TOPIC_INTENTS_APPROVED: &str = "intents.approved";
pub const TOPIC_INTENTS_REJECTED: &str = "intents.rejected";
pub const TOPIC_ACTIONS_APPLIED: &str = "actions.applied";

// Tools
pub const TOPIC_TOOL_CACHE: &str = "tool.cache";
pub const TOPIC_TOOL_RAN: &str = "tool.ran";

// Feedback misc signals
pub const TOPIC_FEEDBACK_SIGNAL: &str = "feedback.signal";

// Distillation
pub const TOPIC_DISTILL_COMPLETED: &str = "distill.completed";

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

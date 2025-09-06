use serde::{Deserialize, Serialize};

/// Minimal event envelope; will extend with OTel correlation.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Event<T> {
    pub time: String,     // RFC3339
    pub kind: String,     // e.g., "TaskStarted"
    pub payload: T,
}

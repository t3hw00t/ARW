use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A unit of work submitted to the orchestrator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    /// Sticky routing key; connectors can shard by this.
    pub shard_key: Option<String>,
    /// Tool or operation identifier.
    pub kind: String,
    /// JSON payload for the operation.
    pub payload: serde_json::Value,
    /// Client-supplied idempotency key for exactly-once semantics (best-effort).
    pub idem_key: Option<String>,
    /// Priority lane: higher first; implementation may map to subjects/streams.
    pub priority: i32,
    /// Attempt count; incremented on re-delivery.
    pub attempt: u32,
}

pub const DEFAULT_LEASE_TTL_MS: u64 = 30_000;
pub const MIN_LEASE_TTL_MS: u64 = 100;

impl Task {
    pub fn new(kind: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            shard_key: None,
            kind: kind.into(),
            payload,
            idem_key: None,
            priority: 0,
            attempt: 0,
        }
    }
}

/// Lease token for in-flight work.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaseToken {
    pub task_id: String,
    pub lease_id: String,
    /// Epoch millis when lease expires.
    pub expires_at_ms: u64,
}

/// Result envelope returned by connectors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub task_id: String,
    pub ok: bool,
    pub output: serde_json::Value,
    pub error: Option<String>,
    pub latency_ms: Option<u64>,
}

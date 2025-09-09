use serde::{Deserialize, Serialize};

/// RFC7807-style error payload used at service edges.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProblemDetails {
    pub r#type: String,
    pub title: String,
    pub status: u16,
    pub detail: Option<String>,
    pub instance: Option<String>,
    pub trace_id: Option<String>,
    pub code: Option<String>,
}

/// Opaque cursor pagination envelope.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OperationId(pub String);

/// Connector hello/registration message.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConnectorHello {
    pub id: String,
    pub version: String,
    pub capabilities: Vec<String>,
    pub limits: Option<ConnectorLimits>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConnectorLimits {
    pub max_concurrency: Option<u32>,
    pub max_memory_mb: Option<u64>,
}

/// Periodic liveness and load update.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConnectorHeartbeat {
    pub id: String,
    pub inflight: u32,
    pub load01: f32,
}

// -------- Hierarchy / Core-to-Core negotiation (types only) --------

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum CoreRole {
    Root,
    Regional,
    Edge,
    Connector,
    Observer,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CoreHello {
    pub id: String,
    pub role: CoreRole,
    pub capabilities: Vec<String>,
    pub scope_tags: Vec<String>,
    pub epoch: u64,
    pub nonce: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CoreOffer {
    pub from_id: String,
    pub proposed_role: CoreRole,
    pub parent_hint: Option<String>,
    pub shard_ranges: Vec<String>,
    pub capacity_hint: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CoreAccept {
    pub child_id: String,
    pub parent_id: String,
    pub role: CoreRole,
    pub epoch: u64,
}

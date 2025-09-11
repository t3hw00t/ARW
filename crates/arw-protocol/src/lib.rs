use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// RFC7807-style error payload used at service edges.
#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
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
#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
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

// -------- Gating / Policy Capsule (propagatable) --------

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
pub struct GatingContract {
    pub id: String,
    #[serde(default)]
    pub patterns: Vec<String>,
    #[serde(default)]
    pub subject_role: Option<String>,
    #[serde(default)]
    pub subject_node: Option<String>,
    #[serde(default)]
    pub tags_any: Option<Vec<String>>,
    #[serde(default)]
    pub valid_from_ms: Option<u64>,
    #[serde(default)]
    pub valid_to_ms: Option<u64>,
    #[serde(default)]
    pub auto_renew_secs: Option<u64>,
    #[serde(default)]
    pub immutable: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
pub struct GatingCapsule {
    pub id: String,
    pub version: String,
    pub issued_at_ms: u64,
    #[serde(default)]
    pub issuer: Option<String>,
    #[serde(default)]
    pub hop_ttl: Option<u32>,
    #[serde(default)]
    pub propagate: Option<String>, // none|children|peers|all
    #[serde(default)]
    pub denies: Vec<String>, // immediate deny patterns
    #[serde(default)]
    pub contracts: Vec<GatingContract>,
    #[serde(default)]
    pub signature: Option<String>,
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

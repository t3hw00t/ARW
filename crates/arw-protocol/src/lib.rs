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

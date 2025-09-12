use axum::{extract::State, response::IntoResponse};

use crate::AppState;

// Return memory quarantine entries (planned). If file absent, return [].
pub async fn memory_quarantine_get(_state: State<AppState>) -> impl IntoResponse {
    let path = super::paths::memory_quarantine_path();
    let v = match tokio::fs::read(&path).await {
        Ok(bytes) => serde_json::from_slice::<serde_json::Value>(&bytes)
            .unwrap_or_else(|_| serde_json::Value::Array(Vec::new())),
        Err(_) => serde_json::Value::Array(Vec::new()),
    };
    super::ok(v).into_response()
}

// Return world diff review items (planned). If file absent, return [].
pub async fn world_diffs_get(_state: State<AppState>) -> impl IntoResponse {
    let path = super::paths::world_diffs_review_path();
    let v = match tokio::fs::read(&path).await {
        Ok(bytes) => serde_json::from_slice::<serde_json::Value>(&bytes)
            .unwrap_or_else(|_| serde_json::Value::Array(Vec::new())),
        Err(_) => serde_json::Value::Array(Vec::new()),
    };
    super::ok(v).into_response()
}


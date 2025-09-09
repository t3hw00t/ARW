use axum::{extract::State, response::IntoResponse, Json};
use serde::Deserialize;
use serde_json::Value;
use std::sync::OnceLock;
use tokio::sync::RwLock;

use super::io::{load_json_file, load_json_file_async, save_json_file_async};
use super::paths::memory_path;
use crate::AppState;

fn default_memory() -> Value {
    serde_json::json!({
        "ephemeral":  [],
        "episodic":   [],
        "semantic":   [],
        "procedural": []
    })
}
static MEMORY: OnceLock<RwLock<Value>> = OnceLock::new();
pub(crate) fn memory() -> &'static RwLock<Value> {
    MEMORY.get_or_init(|| {
        let initial = load_json_file(&memory_path()).unwrap_or_else(default_memory);
        RwLock::new(initial)
    })
}

static MEM_LIMIT: OnceLock<RwLock<usize>> = OnceLock::new();
fn initial_mem_limit() -> usize {
    std::env::var("ARW_MEM_LIMIT")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(200)
}
pub(crate) fn mem_limit() -> &'static RwLock<usize> {
    MEM_LIMIT.get_or_init(|| RwLock::new(initial_mem_limit()))
}

#[derive(Deserialize)]
pub(crate) struct ApplyMemory {
    pub kind: String,
    pub value: Value,
    #[serde(default)]
    pub ttl_ms: Option<u64>,
}

#[derive(Deserialize)]
pub(crate) struct SetLimit {
    pub limit: usize,
}

pub(crate) async fn memory_get() -> impl IntoResponse {
    let snap = memory().read().await.clone();
    Json::<Value>(snap)
}
pub(crate) async fn memory_save() -> impl IntoResponse {
    let snap = memory().read().await.clone();
    match save_json_file_async(&memory_path(), &snap).await {
        Ok(_) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"ok": false, "error": e.to_string()})),
        )
            .into_response(),
    }
}
pub(crate) async fn memory_load() -> impl IntoResponse {
    match load_json_file_async(&memory_path()).await {
        Some(v) => {
            let mut m = memory().write().await;
            *m = v.clone();
            Json::<Value>(v).into_response()
        }
        None => (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({"ok": false, "error":"no memory.json"})),
        )
            .into_response(),
    }
}
pub(crate) async fn memory_limit_get() -> impl IntoResponse {
    let n = { *mem_limit().read().await };
    Json(serde_json::json!({ "limit": n }))
}
pub(crate) async fn memory_limit_set(Json(req): Json<SetLimit>) -> impl IntoResponse {
    {
        let mut n = mem_limit().write().await;
        *n = req.limit.max(1);
    }
    Json(serde_json::json!({ "ok": true }))
}

pub(crate) async fn memory_apply(
    State(state): State<AppState>,
    Json(req): Json<ApplyMemory>,
) -> impl IntoResponse {
    let mut mem = memory().write().await;
    let lane = match req.kind.as_str() {
        "ephemeral" => mem.get_mut("ephemeral").and_then(Value::as_array_mut),
        "episodic" => mem.get_mut("episodic").and_then(Value::as_array_mut),
        "semantic" => mem.get_mut("semantic").and_then(Value::as_array_mut),
        "procedural" => mem.get_mut("procedural").and_then(Value::as_array_mut),
        _ => None,
    };
    if let Some(arr) = lane {
        arr.push(req.value.clone());
        let cap = { *mem_limit().read().await };
        while arr.len() > cap {
            arr.remove(0);
        }
        let snap = mem.clone();
        let _ = save_json_file_async(&memory_path(), &snap).await;
        let evt = serde_json::json!({"kind":"Memory.Applied","payload":{"kind": req.kind, "value": req.value, "ttl_ms": req.ttl_ms}});
        state.bus.publish("Memory.Applied", &evt);
        (
            axum::http::StatusCode::ACCEPTED,
            Json(serde_json::json!({"ok": true})),
        )
            .into_response()
    } else {
        (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"ok": false, "error": "invalid kind"})),
        )
            .into_response()
    }
}

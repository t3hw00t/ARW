use axum::{extract::State, response::IntoResponse, Json};
use arw_macros::{arw_admin, arw_gate};

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

#[derive(serde::Deserialize)]
pub struct QuarantineEntry {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub content_preview: Option<String>,
}

// Admin: add an entry to the memory quarantine list (planned shape; stored as array)
#[arw_admin(method = "POST", path = "/admin/memory/quarantine", summary = "Quarantine memory item")]
#[arw_gate("memory:quarantine")] 
pub async fn memory_quarantine_add(
    State(state): State<AppState>,
    Json(req): Json<QuarantineEntry>,
) -> impl IntoResponse {
    let p = super::paths::memory_quarantine_path();
    let mut arr = super::io::load_json_file_async(&p)
        .await
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_else(|| Vec::new());
    let id = req.id.unwrap_or_else(|| super::corr::new_corr_id());
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let entry = serde_json::json!({
        "id": id,
        "project_id": req.project_id.unwrap_or_else(|| std::env::var("ARW_PROJECT_ID").unwrap_or_else(|_| "default".into())),
        "time": now,
        "content_type": req.content_type.unwrap_or("text/plain".into()),
        "content_preview": req.content_preview.unwrap_or_default(),
        "state": "queued"
    });
    arr.push(entry.clone());
    let _ = super::io::save_json_file_async(&p, &serde_json::Value::Array(arr)).await;
    // Emit event
    let mut ev = entry.clone();
    super::corr::ensure_corr(&mut ev);
    state.bus.publish("Memory.Quarantined", &ev);
    super::ok(serde_json::json!({"ok": true})).into_response()
}

#[derive(serde::Deserialize)]
pub struct AdmitReq { pub id: String }

// Admin: admit (remove) an entry from the memory quarantine list by id
#[arw_admin(method = "POST", path = "/admin/memory/quarantine/admit", summary = "Admit quarantined item")]
#[arw_gate("memory:admit")] 
pub async fn memory_quarantine_admit(
    State(state): State<AppState>,
    Json(req): Json<AdmitReq>,
) -> impl IntoResponse {
    let p = super::paths::memory_quarantine_path();
    let mut arr = super::io::load_json_file_async(&p)
        .await
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_else(|| Vec::new());
    let before = arr.len();
    arr.retain(|v| v.get("id").and_then(|x| x.as_str()) != Some(req.id.as_str()));
    let _ = super::io::save_json_file_async(&p, &serde_json::Value::Array(arr)).await;
    // Emit event
    let mut ev = serde_json::json!({"id": req.id});
    super::corr::ensure_corr(&mut ev);
    state.bus.publish("Memory.Admitted", &ev);
    let removed = before.saturating_sub(
        super::io::load_json_file_async(&p)
            .await
            .and_then(|v| v.as_array().map(|a| a.len()))
            .unwrap_or(0),
    );
    super::ok(serde_json::json!({"removed": removed})).into_response()
}

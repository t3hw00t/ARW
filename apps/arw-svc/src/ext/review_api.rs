use axum::{extract::State, response::IntoResponse, Json};
use arw_macros::{arw_admin, arw_gate};

use crate::AppState;

// Return memory quarantine entries (planned). If file absent, return [].
#[arw_admin(method = "GET", path = "/admin/state/memory/quarantine", summary = "Get memory quarantine entries")]
#[arw_gate("state:memory_quarantine:get")]
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
#[arw_admin(method = "GET", path = "/admin/state/world_diffs", summary = "Get world diffs for review")]
#[arw_gate("state:world_diffs:get")]
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
    #[serde(default)]
    pub provenance: Option<String>,
    #[serde(default)]
    pub risk_markers: Option<Vec<String>>,
    #[serde(default)]
    pub evidence_score: Option<f64>,
    #[serde(default)]
    pub extractor: Option<String>,
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
    // Trim preview and enforce simple bounds
    let mut preview = req.content_preview.unwrap_or_default();
    if preview.len() > 2048 { preview.truncate(2048); }
    let score = req.evidence_score.unwrap_or(0.0);
    let entry = serde_json::json!({
        "id": id,
        "project_id": req.project_id.unwrap_or_else(|| std::env::var("ARW_PROJECT_ID").unwrap_or_else(|_| "default".into())),
        "time": now,
        "content_type": req.content_type.unwrap_or("text/plain".into()),
        "content_preview": preview,
        "provenance": req.provenance.unwrap_or_default(),
        "risk_markers": req.risk_markers.unwrap_or_default(),
        "evidence_score": score,
        "extractor": req.extractor,
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

#[derive(serde::Deserialize)]
pub struct WorldDiffQueueReq {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub from_node: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub changes: Option<serde_json::Value>,
}

#[arw_admin(method = "POST", path = "/admin/world_diffs/queue", summary = "Queue world diff for review")]
#[arw_gate("world_diffs:queue")] 
pub async fn world_diffs_queue(
    State(state): State<AppState>,
    Json(req): Json<WorldDiffQueueReq>,
) -> impl IntoResponse {
    let p = super::paths::world_diffs_review_path();
    let mut arr = super::io::load_json_file_async(&p)
        .await
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_else(|| Vec::new());
    let id = req.id.unwrap_or_else(|| super::corr::new_corr_id());
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let entry = serde_json::json!({
        "id": id,
        "project_id": req.project_id.unwrap_or_else(|| std::env::var("ARW_PROJECT_ID").unwrap_or_else(|_| "default".into())),
        "from_node": req.from_node.unwrap_or_default(),
        "issued_at": now,
        "summary": req.summary.unwrap_or_default(),
        "changes": req.changes.unwrap_or(serde_json::json!([])),
        "conflicts": [],
        "state": "queued"
    });
    arr.push(entry.clone());
    let _ = super::io::save_json_file_async(&p, &serde_json::Value::Array(arr)).await;
    let mut ev = entry.clone();
    super::corr::ensure_corr(&mut ev);
    state.bus.publish("WorldDiff.Queued", &ev);
    super::ok(serde_json::json!({"ok": true})).into_response()
}

#[derive(serde::Deserialize)]
pub struct WorldDiffDecisionReq {
    pub id: String,
    pub decision: String, // apply | reject | defer
    #[serde(default)]
    pub note: Option<String>,
}

#[arw_admin(method = "POST", path = "/admin/world_diffs/decision", summary = "Decide world diff")]
#[arw_gate("world_diffs:decide")] 
pub async fn world_diffs_decision(
    State(state): State<AppState>,
    Json(req): Json<WorldDiffDecisionReq>,
) -> impl IntoResponse {
    let p = super::paths::world_diffs_review_path();
    let mut arr = super::io::load_json_file_async(&p)
        .await
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_else(|| Vec::new());
    let mut found = None;
    for it in arr.iter_mut() {
        if it.get("id").and_then(|x| x.as_str()) == Some(req.id.as_str()) {
            let state_str = match req.decision.as_str() { "apply" => "applied", "reject" => "rejected", _ => "queued" };
            if let Some(obj) = it.as_object_mut() {
                obj.insert("state".into(), serde_json::Value::String(state_str.into()));
                if let Some(n) = &req.note { obj.insert("note".into(), serde_json::Value::String(n.clone())); }
            }
            found = Some(it.clone());
            break;
        }
    }
    let _ = super::io::save_json_file_async(&p, &serde_json::Value::Array(arr)).await;
    if let Some(mut ev) = found {
        super::corr::ensure_corr(&mut ev);
        match req.decision.as_str() {
            "apply" => state.bus.publish("WorldDiff.Applied", &ev),
            "reject" => state.bus.publish("WorldDiff.Rejected", &ev),
            _ => state.bus.publish("WorldDiff.Queued", &ev),
        };
        return super::ok(serde_json::json!({"ok": true})).into_response();
    }
    super::ApiError::not_found("diff not found").into_response()
}

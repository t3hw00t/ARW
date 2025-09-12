use crate::AppState;
use axum::{extract::State, response::IntoResponse, Json};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
pub struct DryRunReq {
    #[serde(default)]
    pub patches: Vec<Value>,
}

#[derive(Deserialize)]
pub struct ApplyPatchReq {
    #[serde(default)]
    pub unit_id: Option<String>,
    #[serde(default)]
    pub patches: Vec<Value>,
    #[serde(default)]
    pub scope: Option<String>,
}

#[derive(Deserialize)]
pub struct RevertPatchReq {
    #[serde(default)]
    pub unit_id: Option<String>,
    #[serde(default)]
    pub snapshot_id: Option<String>,
}

pub async fn dry_run(Json(req): Json<DryRunReq>) -> impl IntoResponse {
    let summary = json!({
        "patch_count": req.patches.len(),
        "targets": req
            .patches
            .iter()
            .filter_map(|p| p.get("target").and_then(|s| s.as_str()))
            .collect::<Vec<_>>()
    });
    super::ok(json!({ "diff": summary, "warnings": Vec::<Value>::new() }))
}

pub async fn apply(State(state): State<AppState>, Json(req): Json<ApplyPatchReq>) -> impl IntoResponse {
    // Load current config (object)
    let cfg_path = crate::ext::paths::config_path();
    let mut cfg = crate::ext::io::load_json_file_async(&cfg_path)
        .await
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    // Prepare snapshot of previous state
    let prev = Value::Object(cfg.clone());
    // Simple merge helper
    fn merge(a: &mut Value, b: &Value) {
        match (a, b) {
            (Value::Object(ao), Value::Object(bo)) => {
                for (k, bv) in bo {
                    match ao.get_mut(k) {
                        Some(av) => merge(av, bv),
                        None => {
                            ao.insert(k.clone(), bv.clone());
                        }
                    }
                }
            }
            (a, b) => *a = b.clone(),
        }
    }
    // Ensure root.targets exists
    let targets = cfg.entry("targets").or_insert(Value::Object(Default::default()));
    // Apply patches (merge only)
    for p in &req.patches {
        let target = p.get("target").and_then(|s| s.as_str()).unwrap_or("");
        let op = p.get("op").and_then(|s| s.as_str()).unwrap_or("merge");
        let val = p.get("value").cloned().unwrap_or(Value::Null);
        if target.is_empty() {
            continue;
        }
        if op != "merge" {
            // Unsupported op in MVP
            continue;
        }
        // Get or init target object
        let tgt_entry = match targets.as_object_mut() {
            Some(map) => map.entry(target.to_string()).or_insert(Value::Object(Default::default())),
            None => targets,
        };
        merge(tgt_entry, &val);
    }
    // Persist new config and snapshot
    let new_cfg = Value::Object(cfg.clone());
    let snap_id = uuid::Uuid::new_v4().to_string();
    let snap = json!({
        "id": snap_id,
        "time": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "unit_id": req.unit_id,
        "scope": req.scope,
        "prev_config": prev,
    });
    let snap_path = crate::ext::paths::snapshots_dir().join(format!("{}.json", snap_id));
    let _ = crate::ext::io::save_json_file_async(&snap_path, &snap).await;
    let _ = crate::ext::io::save_json_file_async(&cfg_path, &new_cfg).await;
    // Emit event
    let mut payload = json!({
        "unit_id": req.unit_id,
        "scope": req.scope,
        "patch_count": req.patches.len(),
        "snapshot_id": snap_id,
    });
    super::corr::ensure_corr(&mut payload);
    state.bus.publish("LogicUnit.Applied", &payload);
    super::ok(json!({ "applied": true, "snapshot_id": payload.get("snapshot_id") }))
}

pub async fn revert(State(state): State<AppState>, Json(req): Json<RevertPatchReq>) -> impl IntoResponse {
    // Revert from snapshot if provided
    if let Some(id) = &req.snapshot_id {
        let path = crate::ext::paths::snapshots_dir().join(format!("{}.json", id));
        if let Some(v) = crate::ext::io::load_json_file_async(&path).await {
            if let Some(prev) = v.get("prev_config") {
                let _ = crate::ext::io::save_json_file_async(
                    &crate::ext::paths::config_path(),
                    prev,
                )
                .await;
            }
        }
    }
    let mut payload = json!({ "unit_id": req.unit_id, "snapshot_id": req.snapshot_id });
    super::corr::ensure_corr(&mut payload);
    state.bus.publish("LogicUnit.Reverted", &payload);
    super::ok(json!({ "reverted": true }))
}

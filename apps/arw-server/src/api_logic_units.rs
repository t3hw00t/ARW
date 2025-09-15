use axum::{extract::{State, Query}, Json};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use jsonschema::{Draft, JSONSchema};
use serde_json::{json, Value};

use crate::{AppState, admin_ok};
use crate::api_config::{merge_values, ensure_path, get_by_dot, dot_to_pointer, infer_schema_for_target, validate_patch_value};

pub async fn logic_units_list(State(state): State<AppState>, Query(q): Query<std::collections::HashMap<String, String>>) -> impl IntoResponse {
    let limit = q.get("limit").and_then(|s| s.parse::<i64>().ok()).unwrap_or(200);
    let items = state.kernel.list_logic_units(limit).unwrap_or_default();
    Json(json!({"items": items}))
}

pub async fn state_logic_units(State(state): State<AppState>, Query(q): Query<std::collections::HashMap<String, String>>) -> impl IntoResponse {
    let limit = q.get("limit").and_then(|s| s.parse::<i64>().ok()).unwrap_or(200);
    let items = state.kernel.list_logic_units(limit).unwrap_or_default();
    Json(json!({"items": items}))
}

pub async fn logic_units_install(State(state): State<AppState>, headers: HeaderMap, Json(mut manifest): Json<Value>) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return (axum::http::StatusCode::UNAUTHORIZED, Json(json!({"type":"about:blank","title":"Unauthorized","status":401})));
    }
    let id = manifest.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()).unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    manifest["id"] = json!(id);
    let _ = state.kernel.insert_logic_unit(&id, &manifest, "installed");
    state.bus.publish("logic.unit.installed", &json!({"id": id}));
    (axum::http::StatusCode::CREATED, Json(json!({"id": manifest["id"].clone(), "ok": true})))
}

pub async fn logic_units_apply(State(state): State<AppState>, headers: HeaderMap, Json(body): Json<Value>) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return (axum::http::StatusCode::UNAUTHORIZED, Json(json!({"type":"about:blank","title":"Unauthorized","status":401})));
    }
    if let Err(errs) = validate_patch_value(&body) {
        return (axum::http::StatusCode::BAD_REQUEST, Json(json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"invalid patch body", "errors": errs})));
    }
    let id = body.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let dry = body.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(false);
    let mut schema_ref = body.get("schema_ref").and_then(|v| v.as_str()).map(|s| s.to_string());
    let mut schema_pointer = body.get("schema_pointer").and_then(|v| v.as_str()).map(|s| s.to_string());
    let patches = body.get("patches").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let current_cfg = state.config_state.lock().await.clone();
    let mut cfg = current_cfg.clone();
    let mut diffs: Vec<Value> = Vec::new();
    for p in patches.iter() {
        let target = p.get("target").and_then(|v| v.as_str()).unwrap_or("");
        let op = p.get("op").and_then(|v| v.as_str()).unwrap_or("merge");
        let val = p.get("value").cloned().unwrap_or(json!({}));
        if target.is_empty() { continue; }
        let mut_root = ensure_path(&mut cfg, target) as *mut Value;
        unsafe {
            let before = get_by_dot(&current_cfg, target).cloned();
            let dst = &mut *mut_root;
            if op == "set" { *dst = val; } else { merge_values(dst, &val); }
            let after = get_by_dot(&cfg, target).cloned();
            let pointer = dot_to_pointer(target);
            diffs.push(json!({"target": target, "pointer": pointer, "op": op, "before": before, "after": after}));
        }
    }
    if schema_ref.is_none() {
        if let Some(first) = patches.first() { if let Some((p,ptr)) = infer_schema_for_target(first.get("target").and_then(|v| v.as_str()).unwrap_or("")) { schema_ref = Some(p); schema_pointer = Some(ptr); } }
    }
    if let Some(schema_path) = schema_ref.as_deref() {
        let pointer = schema_pointer.as_deref();
        let to_validate = if let Some(ptr) = pointer { get_by_dot(&cfg, ptr).cloned().unwrap_or(json!({})) } else { cfg.clone() };
        match std::fs::read(schema_path) {
            Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                Ok(schema_json) => match JSONSchema::options().with_draft(Draft::Draft7).compile(&schema_json) {
                    Ok(compiled) => if let Err(errors) = compiled.validate(&to_validate) {
                        let errs: Vec<Value> = errors.map(|e| json!({"path": e.instance_path.to_string(), "error": e.to_string()})).collect();
                        return (axum::http::StatusCode::BAD_REQUEST, Json(json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"schema validation failed", "errors": errs})));
                    },
                    Err(e) => return (axum::http::StatusCode::BAD_REQUEST, Json(json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"invalid schema", "error": e.to_string()}))),
                },
                Err(e) => return (axum::http::StatusCode::BAD_REQUEST, Json(json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"invalid schema json", "error": e.to_string()}))),
            },
            Err(e) => return (axum::http::StatusCode::BAD_REQUEST, Json(json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"schema not found", "error": e.to_string()}))),
        }
    }
    if !dry {
        { let mut cur = state.config_state.lock().await; *cur = cfg.clone(); }
        if !id.is_empty() {
            let sid = state.kernel.list_config_snapshots(1).unwrap_or_default().first().and_then(|x| x.get("id").and_then(|v| v.as_str())).map(|s| s.to_string());
            state.bus.publish("logic.unit.applied", &json!({"id": id, "ops": patches.len(), "snapshot_id": sid}));
        }
        state.bus.publish("config.patch.applied", &json!({"ops": patches.len()}));
        let json_patch: Vec<Value> = diffs.iter().filter_map(|d| { let p = d.get("pointer").and_then(|v| v.as_str())?; let v = d.get("after").cloned().unwrap_or(json!({})); Some(json!({"op":"replace","path": p, "value": v})) }).collect();
        return (axum::http::StatusCode::OK, Json(json!({"ok": true, "id": if id.is_empty(){Value::Null}else{json!(id)}, "dry_run": false, "config": cfg, "diff_summary": diffs, "json_patch": json_patch })));
    }
    let json_patch: Vec<Value> = diffs.iter().filter_map(|d| { let p = d.get("pointer").and_then(|v| v.as_str())?; let v = d.get("after").cloned().unwrap_or(json!({})); Some(json!({"op":"replace","path": p, "value": v})) }).collect();
    (axum::http::StatusCode::OK, Json(json!({"ok": true, "id": if id.is_empty(){Value::Null}else{json!(id)}, "dry_run": true, "config": cfg, "diff_summary": diffs, "json_patch": json_patch })))
}

pub async fn logic_units_revert(State(state): State<AppState>, headers: HeaderMap, Json(body): Json<Value>) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return (axum::http::StatusCode::UNAUTHORIZED, Json(json!({"type":"about:blank","title":"Unauthorized","status":401})));
    }
    let snap = body.get("snapshot_id").and_then(|v| v.as_str()).unwrap_or("");
    if snap.is_empty() { return (axum::http::StatusCode::BAD_REQUEST, Json(json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"missing snapshot_id"}))); }
    let mut hist = state.config_history.lock().await;
    if let Some((_, cfg)) = hist.iter().rev().find(|(id, _)| id == snap).cloned() {
        let new_id = uuid::Uuid::new_v4().to_string();
        { let mut cur = state.config_state.lock().await; *cur = cfg.clone(); }
        hist.push((new_id.clone(), cfg.clone()));
        state.bus.publish("logic.unit.reverted", &json!({"snapshot_id": snap, "new_snapshot_id": new_id}));
        (axum::http::StatusCode::OK, Json(json!({"ok": true, "snapshot_id": new_id, "config": cfg})))
    } else {
        (axum::http::StatusCode::NOT_FOUND, Json(json!({"type":"about:blank","title":"Not Found","status":404})))
    }
}


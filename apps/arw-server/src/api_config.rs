use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::{
    extract::{Path, Query, State},
    Json,
};
use jsonschema::{Draft, JSONSchema};
use serde::Deserialize;
use serde_json::{json, Value};
use utoipa::ToSchema;

use crate::{admin_ok, AppState};
use arw_topics as topics;

/// Effective config JSON.
#[utoipa::path(get, path = "/state/config", tag = "Config", responses((status = 200, body = serde_json::Value)))]
pub async fn state_config(State(state): State<AppState>) -> impl IntoResponse {
    let snap = state.config_state.lock().await.clone();
    Json(json!({"config": snap}))
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct ApplyReq {
    #[serde(default)]
    #[allow(dead_code)]
    pub id: Option<String>,
    #[serde(default)]
    pub dry_run: Option<bool>,
    pub patches: Vec<PatchOp>,
    #[serde(default)]
    pub schema_ref: Option<String>,
    #[serde(default)]
    pub schema_pointer: Option<String>,
}

#[derive(Deserialize, Clone, ToSchema)]
pub(crate) struct PatchOp {
    pub target: String,
    pub op: String,
    pub value: Value,
}

pub(crate) fn merge_values(dst: &mut Value, add: &Value) {
    match (dst, add) {
        (Value::Object(d), Value::Object(a)) => {
            for (k, v) in a.iter() {
                match d.get_mut(k) {
                    Some(dv) => merge_values(dv, v),
                    None => {
                        d.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (d, v) => {
            *d = v.clone();
        }
    }
}

pub(crate) fn ensure_path<'a>(root: &'a mut Value, path: &str) -> &'a mut Value {
    let mut cur = root;
    for seg in path.split('.') {
        if seg.is_empty() {
            continue;
        }
        if !cur.is_object() {
            *cur = json!({});
        }
        let map = cur.as_object_mut().unwrap();
        if !map.contains_key(seg) {
            map.insert(seg.to_string(), json!({}));
        }
        cur = map.get_mut(seg).unwrap();
    }
    cur
}

pub(crate) fn get_by_dot<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cur = root;
    for seg in path.split('.') {
        if seg.is_empty() {
            continue;
        }
        match cur {
            Value::Object(map) => {
                if let Some(v) = map.get(seg) {
                    cur = v;
                } else {
                    return None;
                }
            }
            _ => return None,
        }
    }
    Some(cur)
}

pub(crate) fn dot_to_pointer(path: &str) -> String {
    if path.is_empty() {
        return "/".to_string();
    }
    let mut out = String::from("/");
    let mut first = true;
    for seg in path.split('.') {
        if !first {
            out.push('/');
        } else {
            first = false;
        }
        let esc = seg.replace('~', "~0").replace('/', "~1");
        out.push_str(&esc);
    }
    out
}

pub(crate) fn infer_schema_for_target(target: &str) -> Option<(String, String)> {
    fn load_map() -> Option<serde_json::Value> {
        let path = std::env::var("ARW_SCHEMA_MAP")
            .ok()
            .unwrap_or_else(|| "configs/schema_map.json".into());
        let p = std::path::Path::new(&path);
        if p.exists() {
            if let Ok(bytes) = std::fs::read(p) {
                return serde_json::from_slice::<serde_json::Value>(&bytes).ok();
            }
        }
        None
    }
    let seg = target.split('.').next().unwrap_or("");
    if let Some(map) = load_map() {
        if let Some(obj) = map.get(seg) {
            if let Some(schema_ref) = obj.get("schema_ref").and_then(|v| v.as_str()) {
                let pointer = if let Some(pp) = obj.get("pointer_prefix").and_then(|v| v.as_str()) {
                    if target.len() > seg.len() {
                        format!("{}{}", pp, &target[seg.len()..])
                    } else {
                        pp.to_string()
                    }
                } else {
                    target.to_string()
                };
                if std::path::Path::new(schema_ref).exists() {
                    return Some((schema_ref.to_string(), pointer));
                }
            }
        }
    }
    let (schema_file, pointer) = match seg {
        "recipes" => ("spec/schemas/recipe_manifest.json", target.to_string()),
        "policy" | "policy_network" | "network" => (
            "spec/schemas/policy_network_scopes.json",
            target.to_string(),
        ),
        _ => return None,
    };
    if std::path::Path::new(schema_file).exists() {
        Some((schema_file.to_string(), pointer))
    } else {
        None
    }
}

pub(crate) fn validate_patch_value(v: &Value) -> Result<(), Vec<Value>> {
    let schema = json!({
        "type":"object",
        "properties":{
            "id": {"type":"string"},
            "dry_run": {"type":"boolean"},
            "patches": {
                "type":"array",
                "items": {"type":"object","required": ["target","op","value"],
                    "properties": {"target": {"type":"string", "minLength": 1},"op": {"type":"string", "enum": ["merge","set"]},"value": {}},
                    "additionalProperties": false
                },
                "minItems": 1
            }
        },
        "required": ["patches"],
        "additionalProperties": true
    });
    let compiled = JSONSchema::options()
        .with_draft(Draft::Draft7)
        .compile(&schema)
        .unwrap();
    let res = compiled.validate(v);
    match res {
        Err(errors) => Err(errors
            .map(|e| json!({"path": e.instance_path.to_string(), "error": e.to_string()}))
            .collect()),
        Ok(_) => Ok(()),
    }
}

/// Apply config patches with optional schema validation (admin).
#[utoipa::path(post, path = "/patch/apply", tag = "Config", request_body = ApplyReq, responses((status = 200, body = serde_json::Value), (status = 401), (status = 400)))]
pub async fn patch_apply(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ApplyReq>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        );
    }
    let dry = req.dry_run.unwrap_or(false);
    let current_cfg = state.config_state.lock().await.clone();
    let mut cfg = current_cfg.clone();
    let mut diffs: Vec<Value> = Vec::new();
    for op in &req.patches {
        if op.op == "merge" || op.op == "set" {
            let mut_root = ensure_path(&mut cfg, &op.target) as *mut Value;
            unsafe {
                let before = get_by_dot(&current_cfg, &op.target).cloned();
                let dst = &mut *mut_root;
                if op.op == "merge" {
                    merge_values(dst, &op.value);
                } else {
                    *dst = op.value.clone();
                }
                let after = get_by_dot(&cfg, &op.target).cloned();
                let pointer = dot_to_pointer(&op.target);
                diffs.push(json!({"target": op.target, "pointer": pointer, "op": op.op, "before": before, "after": after}));
            }
        }
    }
    let mut schema_opt = req.schema_ref.clone();
    let mut pointer_opt = req.schema_pointer.clone();
    if schema_opt.is_none() {
        if let Some(first) = req.patches.first() {
            schema_opt = infer_schema_for_target(&first.target).map(|(p, ptr)| {
                pointer_opt = Some(ptr);
                p
            });
        }
    }
    if let Some(schema_path) = schema_opt.as_deref() {
        let pointer = pointer_opt.as_deref();
        let to_validate = if let Some(ptr) = pointer {
            get_by_dot(&cfg, ptr).cloned().unwrap_or(json!({}))
        } else {
            cfg.clone()
        };
        match std::fs::read(schema_path) {
            Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                Ok(schema_json) => match JSONSchema::options()
                    .with_draft(Draft::Draft7)
                    .compile(&schema_json)
                {
                    Ok(compiled) => {
                        if let Err(errors) = compiled.validate(&to_validate) {
                            let errs: Vec<Value> = errors.map(|e| json!({"path": e.instance_path.to_string(), "error": e.to_string()})).collect();
                            return (
                                axum::http::StatusCode::BAD_REQUEST,
                                Json(
                                    json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"schema validation failed", "errors": errs}),
                                ),
                            );
                        }
                    }
                    Err(e) => {
                        return (
                            axum::http::StatusCode::BAD_REQUEST,
                            Json(
                                json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"invalid schema", "error": e.to_string()}),
                            ),
                        )
                    }
                },
                Err(e) => {
                    return (
                        axum::http::StatusCode::BAD_REQUEST,
                        Json(
                            json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"invalid schema json", "error": e.to_string()}),
                        ),
                    )
                }
            },
            Err(e) => {
                return (
                    axum::http::StatusCode::BAD_REQUEST,
                    Json(
                        json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"schema not found", "error": e.to_string()}),
                    ),
                )
            }
        }
    }
    if !dry {
        let snapshot_id = match state.kernel.insert_config_snapshot(&cfg) {
            Ok(id) => id,
            Err(e) => {
                return (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    Json(
                        json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()}),
                    ),
                )
            }
        };
        {
            let mut hist = state.config_history.lock().await;
            hist.push((snapshot_id.clone(), cfg.clone()));
        }
        {
            let mut cur = state.config_state.lock().await;
            *cur = cfg.clone();
        }
        state.bus.publish(
            topics::TOPIC_CONFIG_PATCH_APPLIED,
            &json!({"ops": req.patches.len(), "snapshot_id": snapshot_id}),
        );
        let json_patch: Vec<Value> = diffs
            .iter()
            .filter_map(|d| {
                let p = d.get("pointer").and_then(|v| v.as_str())?;
                let v = d.get("after").cloned().unwrap_or(json!({}));
                Some(json!({"op":"replace","path": p, "value": v}))
            })
            .collect();
        return (
            axum::http::StatusCode::OK,
            Json(
                json!({"ok": true, "dry_run": false, "config": cfg, "diff_summary": diffs, "json_patch": json_patch }),
            ),
        );
    }
    let json_patch: Vec<Value> = diffs
        .iter()
        .filter_map(|d| {
            let p = d.get("pointer").and_then(|v| v.as_str())?;
            let v = d.get("after").cloned().unwrap_or(json!({}));
            Some(json!({"op":"replace","path": p, "value": v}))
        })
        .collect();
    (
        axum::http::StatusCode::OK,
        Json(
            json!({"ok": true, "dry_run": true, "config": cfg, "diff_summary": diffs, "json_patch": json_patch }),
        ),
    )
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct RevertReq {
    pub snapshot_id: String,
}

/// Revert to a snapshot id (admin).
#[utoipa::path(post, path = "/patch/revert", tag = "Config", request_body = RevertReq, responses((status = 200, body = serde_json::Value), (status = 404), (status = 401)))]
pub async fn patch_revert(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RevertReq>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        );
    }
    let mut hist = state.config_history.lock().await;
    if let Some((_, snap)) = hist
        .iter()
        .rev()
        .find(|(id, _)| id == &req.snapshot_id)
        .cloned()
    {
        let new_id = uuid::Uuid::new_v4().to_string();
        {
            let mut cur = state.config_state.lock().await;
            *cur = snap.clone();
        }
        hist.push((new_id.clone(), snap.clone()));
        state.bus.publish(
            topics::TOPIC_LOGICUNIT_REVERTED,
            &json!({"snapshot_id": req.snapshot_id, "new_snapshot_id": new_id}),
        );
        (
            axum::http::StatusCode::OK,
            Json(json!({"ok": true, "snapshot_id": new_id, "config": snap})),
        )
    } else {
        (
            axum::http::StatusCode::NOT_FOUND,
            Json(json!({"type":"about:blank","title":"Not Found","status":404})),
        )
    }
}

/// List recent config snapshots.
#[utoipa::path(get, path = "/state/config/snapshots", tag = "Config", params(("limit" = Option<i64>, Query)), responses((status = 200, body = serde_json::Value)))]
pub async fn state_config_snapshots(
    State(state): State<AppState>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let limit = q
        .get("limit")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(50);
    let items = state
        .kernel
        .list_config_snapshots(limit)
        .unwrap_or_default();
    (axum::http::StatusCode::OK, Json(json!({"items": items}))).into_response()
}

/// Get a specific config snapshot by id.
#[utoipa::path(get, path = "/state/config/snapshots/{id}", tag = "Config", params(("id" = String, Path)), responses((status = 200, body = serde_json::Value), (status = 404)))]
pub async fn state_config_snapshot_get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.kernel.get_config_snapshot(&id) {
        Ok(Some(cfg)) => (axum::http::StatusCode::OK, Json(cfg)).into_response(),
        Ok(None) => (
            axum::http::StatusCode::NOT_FOUND,
            Json(json!({"type":"about:blank","title":"Not Found","status":404})),
        )
            .into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()}),
            ),
        )
            .into_response(),
    }
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct ValidateReq {
    pub schema_ref: String,
    #[serde(default)]
    pub schema_pointer: Option<String>,
    #[serde(default)]
    pub config: Option<Value>,
}
/// Validate a config against a JSON Schema.
#[utoipa::path(post, path = "/patch/validate", tag = "Config", request_body = ValidateReq, responses((status = 200, body = serde_json::Value), (status = 400)))]
pub async fn patch_validate(Json(req): Json<ValidateReq>) -> impl IntoResponse {
    let schema_path = req.schema_ref.as_str();
    let to_validate = req.config.unwrap_or(json!({}));
    match std::fs::read(schema_path) {
        Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
            Ok(schema_json) => match JSONSchema::options().with_draft(Draft::Draft7).compile(&schema_json) {
                Ok(compiled) => {
                    let pointer = req.schema_pointer.as_deref();
                    let sub = if let Some(ptr) = pointer { get_by_dot(&to_validate, ptr).cloned().unwrap_or(json!({})) } else { to_validate };
                    let res = compiled.validate(&sub);
                    match res {
                        Ok(_) => (axum::http::StatusCode::OK, Json(json!({"ok": true}))).into_response(),
                        Err(errors) => {
                            let errs: Vec<Value> = errors.map(|e| json!({"path": e.instance_path.to_string(), "error": e.to_string()})).collect();
                            (axum::http::StatusCode::BAD_REQUEST, Json(json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"schema validation failed", "errors": errs}))).into_response()
                        }
                    }
                }
                Err(e) => (axum::http::StatusCode::BAD_REQUEST, Json(json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"invalid schema", "error": e.to_string()}))).into_response(),
            }
            Err(e) => (axum::http::StatusCode::BAD_REQUEST, Json(json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"invalid schema json", "error": e.to_string()}))).into_response(),
        }
        Err(e) => (axum::http::StatusCode::BAD_REQUEST, Json(json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"schema not found", "error": e.to_string()}))).into_response(),
    }
}

/// Current schema mapping used for inference.
#[utoipa::path(get, path = "/state/schema_map", tag = "Config", responses((status = 200, body = serde_json::Value)))]
pub async fn state_schema_map() -> impl IntoResponse {
    let path = std::env::var("ARW_SCHEMA_MAP")
        .ok()
        .unwrap_or_else(|| "configs/schema_map.json".into());
    let p = std::path::Path::new(&path);
    if p.exists() {
        match tokio::fs::read(p).await {
            Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                Ok(v) => (axum::http::StatusCode::OK, Json(json!({"path": path, "map": v}))).into_response(),
                Err(e) => (axum::http::StatusCode::BAD_REQUEST, Json(json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"invalid schema_map json", "error": e.to_string(), "path": path}))).into_response(),
            },
            Err(e) => (axum::http::StatusCode::BAD_REQUEST, Json(json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"failed to read schema_map", "error": e.to_string(), "path": path}))).into_response(),
        }
    } else {
        (
            axum::http::StatusCode::OK,
            Json(json!({"path": path, "map": json!({})})),
        )
            .into_response()
    }
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct InferReq {
    pub target: String,
}
/// Infer a JSON Schema ref and pointer for a target path.
#[utoipa::path(post, path = "/patch/infer_schema", tag = "Config", request_body = InferReq, responses((status = 200, body = serde_json::Value), (status = 404)))]
pub async fn patch_infer_schema(Json(req): Json<InferReq>) -> impl IntoResponse {
    match infer_schema_for_target(&req.target) {
        Some((schema_ref, pointer)) => (axum::http::StatusCode::OK, Json(json!({"schema_ref": schema_ref, "schema_pointer": pointer}))).into_response(),
        None => (axum::http::StatusCode::NOT_FOUND, Json(json!({"type":"about:blank","title":"Not Found","status":404, "detail":"no matching schema mapping"}))).into_response(),
    }
}

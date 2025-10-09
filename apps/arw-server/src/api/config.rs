use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
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
use std::io::ErrorKind;
use std::path::{Path as FsPath, PathBuf};
use tokio::io::{AsyncReadExt, BufReader};

const MAX_SCHEMA_BYTES: usize = 512 * 1024;

fn unauthorized_response() -> axum::response::Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
    )
        .into_response()
}

#[derive(Debug)]
enum SchemaAccessError {
    Invalid,
    NotAllowed,
    TooLarge,
    Io(std::io::Error),
}

impl From<std::io::Error> for SchemaAccessError {
    fn from(value: std::io::Error) -> Self {
        SchemaAccessError::Io(value)
    }
}

fn schema_error_response(detail: &str) -> axum::response::Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"type":"about:blank","title":"Bad Request","status":400,"detail": detail})),
    )
        .into_response()
}

fn schema_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd.join("spec/schemas"));
        roots.push(cwd.join("configs"));
    }
    if let Ok(map_path) = std::env::var("ARW_SCHEMA_MAP") {
        if let Some(parent) = FsPath::new(&map_path).parent() {
            roots.push(parent.to_path_buf());
        }
    }
    roots
        .into_iter()
        .filter_map(|path| path.canonicalize().ok())
        .collect()
}

fn resolve_schema_path(schema_ref: &str) -> Result<PathBuf, SchemaAccessError> {
    let trimmed = schema_ref.trim();
    if trimmed.is_empty() {
        return Err(SchemaAccessError::Invalid);
    }
    let candidate = PathBuf::from(trimmed);
    let resolved = if candidate.is_absolute() {
        candidate
    } else {
        std::env::current_dir()
            .map_err(SchemaAccessError::Io)?
            .join(candidate)
    };
    let canonical = resolved.canonicalize().map_err(SchemaAccessError::Io)?;
    if schema_roots()
        .iter()
        .any(|root| canonical.starts_with(root))
    {
        Ok(canonical)
    } else {
        Err(SchemaAccessError::NotAllowed)
    }
}

async fn read_schema_file(path: &FsPath) -> Result<Vec<u8>, SchemaAccessError> {
    let file = tokio::fs::File::open(path).await?;
    let mut reader = BufReader::new(file);
    let mut data = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        if data.len() + read > MAX_SCHEMA_BYTES {
            return Err(SchemaAccessError::TooLarge);
        }
        data.extend_from_slice(&chunk[..read]);
    }
    Ok(data)
}

/// Effective config JSON.
#[utoipa::path(
    get,
    path = "/state/config",
    tag = "Config",
    responses(
        (status = 200, body = serde_json::Value),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn state_config(headers: HeaderMap, State(state): State<AppState>) -> Response {
    if !admin_ok(&headers).await {
        return unauthorized_response();
    }
    let snap = state.config_state().lock().await.clone();
    Json(json!({"config": snap})).into_response()
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
        let p = FsPath::new(&path);
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
                if FsPath::new(schema_ref).exists() {
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
    if FsPath::new(schema_file).exists() {
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
    if !admin_ok(&headers).await {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        );
    }
    let dry = req.dry_run.unwrap_or(false);
    let current_cfg = state.config_state().lock().await.clone();
    let patch_values: Vec<Value> = req
        .patches
        .iter()
        .map(|op| json!({"target": op.target, "op": op.op, "value": op.value }))
        .collect();
    let safety_issues = crate::patch_guard::check_patches_for_risks(&patch_values);
    if crate::patch_guard::safety_enforced() && !safety_issues.is_empty() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(
                json!({"type":"about:blank","title":"Bad Request","status":400,"detail":"patch safety checks failed","issues": safety_issues}),
            ),
        );
    }
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
        let snapshot_id = if state.kernel_enabled() {
            match state
                .kernel()
                .insert_config_snapshot_async(cfg.clone())
                .await
            {
                Ok(id) => id,
                Err(e) => {
                    return (
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        Json(
                            json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()}),
                        ),
                    )
                }
            }
        } else {
            "kernel-disabled".to_string()
        };
        {
            let history = state.config_history();
            let mut hist = history.lock().await;
            hist.push((snapshot_id.clone(), cfg.clone()));
        }
        {
            let cfg_state = state.config_state();
            let mut cur = cfg_state.lock().await;
            *cur = cfg.clone();
        }
        crate::config::apply_env_overrides_from(&cfg);
        let mut event_payload = json!({"ops": req.patches.len(), "snapshot_id": snapshot_id});
        if !safety_issues.is_empty() {
            event_payload["safety_issues"] = Value::Array(safety_issues.clone());
        }
        state
            .bus()
            .publish(topics::TOPIC_CONFIG_PATCH_APPLIED, &event_payload);
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
                json!({"ok": true, "dry_run": false, "config": cfg, "diff_summary": diffs, "json_patch": json_patch, "safety_issues": safety_issues }),
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
            json!({"ok": true, "dry_run": true, "config": cfg, "diff_summary": diffs, "json_patch": json_patch, "safety_issues": safety_issues }),
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
    if !admin_ok(&headers).await {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        );
    }
    let history = state.config_history();
    let mut hist = history.lock().await;
    if let Some((_, snap)) = hist
        .iter()
        .rev()
        .find(|(id, _)| id == &req.snapshot_id)
        .cloned()
    {
        let new_id = uuid::Uuid::new_v4().to_string();
        {
            let cfg_state = state.config_state();
            let mut cur = cfg_state.lock().await;
            *cur = snap.clone();
        }
        hist.push((new_id.clone(), snap.clone()));
        state.bus().publish(
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
#[utoipa::path(
    get,
    path = "/state/config/snapshots",
    tag = "Config",
    params(("limit" = Option<i64>, Query)),
    responses(
        (status = 200, body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
        (status = 501, description = "Kernel disabled", body = serde_json::Value)
    )
)]
pub async fn state_config_snapshots(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> Response {
    if !admin_ok(&headers).await {
        return unauthorized_response();
    }
    let limit = q
        .get("limit")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(50);
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }
    match state.kernel().list_config_snapshots_async(limit).await {
        Ok(items) => (axum::http::StatusCode::OK, Json(json!({"items": items}))).into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()}),
            ),
        )
            .into_response(),
    }
}

/// Get a specific config snapshot by id.
#[utoipa::path(
    get,
    path = "/state/config/snapshots/{id}",
    tag = "Config",
    params(("id" = String, Path)),
    responses(
        (status = 200, body = serde_json::Value),
        (status = 404),
        (status = 401, description = "Unauthorized"),
        (status = 501, description = "Kernel disabled", body = serde_json::Value)
    )
)]
pub async fn state_config_snapshot_get(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    if !admin_ok(&headers).await {
        return unauthorized_response();
    }
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }
    match state.kernel().get_config_snapshot_async(id.clone()).await {
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

#[derive(Deserialize, ToSchema, Clone)]
pub(crate) struct ValidateReq {
    pub schema_ref: String,
    #[serde(default)]
    pub schema_pointer: Option<String>,
    #[serde(default)]
    pub config: Option<Value>,
}
/// Validate a config against a JSON Schema.
#[utoipa::path(
    post,
    path = "/patch/validate",
    tag = "Config",
    request_body = ValidateReq,
    responses(
        (status = 200, body = serde_json::Value),
        (status = 400),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn patch_validate(headers: HeaderMap, Json(req): Json<ValidateReq>) -> Response {
    if !admin_ok(&headers).await {
        return unauthorized_response();
    }
    let schema_path = req.schema_ref.as_str();
    let to_validate = req.config.unwrap_or(json!({}));
    let resolved = match resolve_schema_path(schema_path) {
        Ok(path) => path,
        Err(SchemaAccessError::Invalid) => return schema_error_response("schema ref missing"),
        Err(SchemaAccessError::NotAllowed) => {
            return schema_error_response("schema path not permitted")
        }
        Err(SchemaAccessError::Io(err)) if err.kind() == ErrorKind::NotFound => {
            return schema_error_response("schema not found")
        }
        Err(SchemaAccessError::Io(_)) => return schema_error_response("schema not found"),
        Err(SchemaAccessError::TooLarge) => unreachable!("size limit enforced during read"),
    };
    let bytes = match read_schema_file(&resolved).await {
        Ok(data) => data,
        Err(SchemaAccessError::TooLarge) => {
            return schema_error_response("schema file exceeds size limit")
        }
        Err(SchemaAccessError::NotAllowed) => {
            return schema_error_response("schema path not permitted")
        }
        Err(SchemaAccessError::Invalid) => return schema_error_response("schema ref missing"),
        Err(SchemaAccessError::Io(err)) if err.kind() == ErrorKind::NotFound => {
            return schema_error_response("schema not found")
        }
        Err(SchemaAccessError::Io(_)) => return schema_error_response("failed to read schema"),
    };
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(schema_json) => match JSONSchema::options()
            .with_draft(Draft::Draft7)
            .compile(&schema_json)
        {
            Ok(compiled) => {
                let pointer = req.schema_pointer.as_deref();
                let sub = if let Some(ptr) = pointer {
                    get_by_dot(&to_validate, ptr)
                        .cloned()
                        .unwrap_or(json!({}))
                } else {
                    to_validate
                };
                let res = compiled.validate(&sub);
                match res {
                    Ok(_) => (StatusCode::OK, Json(json!({"ok": true}))).into_response(),
                    Err(errors) => {
                        let errs: Vec<Value> = errors
                            .map(|e| {
                                json!({"path": e.instance_path.to_string(), "error": e.to_string()})
                            })
                            .collect();
                        (
                            StatusCode::BAD_REQUEST,
                            Json(json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"schema validation failed", "errors": errs})),
                        )
                            .into_response()
                    }
                }
            }
            Err(e) => (
                StatusCode::BAD_REQUEST,
                Json(
                    json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"invalid schema", "error": e.to_string()}),
                ),
            )
                .into_response(),
        },
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(
                json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"invalid schema json", "error": e.to_string()}),
            ),
        )
            .into_response(),
    }
}

/// Current schema mapping used for inference.
#[utoipa::path(
    get,
    path = "/state/schema_map",
    tag = "Config",
    responses(
        (status = 200, body = serde_json::Value),
        (status = 400),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn state_schema_map(headers: HeaderMap) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return unauthorized_response();
    }
    let path = std::env::var("ARW_SCHEMA_MAP")
        .ok()
        .unwrap_or_else(|| "configs/schema_map.json".into());
    let p = FsPath::new(&path);
    if p.exists() {
        let resolved = match resolve_schema_path(&path) {
            Ok(path) => path,
            Err(SchemaAccessError::NotAllowed) => {
                return schema_error_response("schema path not permitted")
            }
            Err(SchemaAccessError::Invalid) => return schema_error_response("schema ref missing"),
            Err(SchemaAccessError::Io(err)) if err.kind() == ErrorKind::NotFound => {
                return (
                    StatusCode::OK,
                    Json(json!({"path": path, "map": json!({})})),
                )
                    .into_response();
            }
            Err(SchemaAccessError::Io(_)) => {
                return schema_error_response("failed to read schema_map")
            }
            Err(SchemaAccessError::TooLarge) => unreachable!("size limit enforced during read"),
        };
        match read_schema_file(&resolved).await {
            Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                Ok(v) => (StatusCode::OK, Json(json!({"path": path, "map": v}))).into_response(),
                Err(e) => (
                    StatusCode::BAD_REQUEST,
                    Json(
                        json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"invalid schema_map json", "error": e.to_string(), "path": path}),
                    ),
                )
                    .into_response(),
            },
            Err(SchemaAccessError::TooLarge) => schema_error_response("schema file exceeds size limit"),
            Err(SchemaAccessError::NotAllowed) => schema_error_response("schema path not permitted"),
            Err(SchemaAccessError::Invalid) => schema_error_response("schema ref missing"),
            Err(SchemaAccessError::Io(err)) if err.kind() == ErrorKind::NotFound => (
                StatusCode::OK,
                Json(json!({"path": path, "map": json!({})})),
            )
                .into_response(),
            Err(SchemaAccessError::Io(err)) => (
                StatusCode::BAD_REQUEST,
                Json(
                    json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"failed to read schema_map", "error": err.to_string(), "path": path}),
                ),
            )
                .into_response(),
        }
    } else {
        (
            StatusCode::OK,
            Json(json!({"path": path, "map": json!({})})),
        )
            .into_response()
    }
}

#[derive(Deserialize, ToSchema, Clone)]
pub(crate) struct InferReq {
    pub target: String,
}
/// Infer a JSON Schema ref and pointer for a target path.
#[utoipa::path(
    post,
    path = "/patch/infer_schema",
    tag = "Config",
    request_body = InferReq,
    responses(
        (status = 200, body = serde_json::Value),
        (status = 404),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn patch_infer_schema(headers: HeaderMap, Json(req): Json<InferReq>) -> Response {
    if !admin_ok(&headers).await {
        return unauthorized_response();
    }
    match infer_schema_for_target(&req.target) {
        Some((schema_ref, pointer)) => match resolve_schema_path(&schema_ref) {
            Ok(_) => (
                StatusCode::OK,
                Json(json!({"schema_ref": schema_ref, "schema_pointer": pointer})),
            )
                .into_response(),
            Err(SchemaAccessError::Io(err)) if err.kind() == ErrorKind::NotFound => {
                (
                    StatusCode::NOT_FOUND,
                    Json(json!({"type":"about:blank","title":"Not Found","status":404, "detail":"no matching schema mapping"})),
                )
                    .into_response()
            }
            Err(_) => (
                StatusCode::NOT_FOUND,
                Json(json!({"type":"about:blank","title":"Not Found","status":404, "detail":"no matching schema mapping"})),
            )
                .into_response(),
        },
        None => (
            StatusCode::NOT_FOUND,
            Json(
                json!({"type":"about:blank","title":"Not Found","status":404, "detail":"no matching schema mapping"}),
            ),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{begin_state_env, build_state};
    use axum::{
        extract::{Path, Query, State},
        http::{HeaderMap, HeaderValue},
        response::IntoResponse,
    };
    use serde_json::json;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn admin_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("X-ARW-Admin", HeaderValue::from_static("secret"));
        headers
    }

    fn setup_schema_env(
        temp: &tempfile::TempDir,
        ctx: &mut crate::test_support::TestCtx,
    ) -> (PathBuf, PathBuf) {
        let schema_dir = temp.path().join("schemas");
        std::fs::create_dir_all(&schema_dir).unwrap();
        let map_path = schema_dir.join("schema_map.json");
        std::fs::write(&map_path, "{}").unwrap();
        ctx.env.set("ARW_SCHEMA_MAP", map_path.to_string_lossy());
        (schema_dir, map_path)
    }

    #[tokio::test]
    async fn state_config_requires_admin() {
        let temp = tempdir().unwrap();
        let mut ctx = begin_state_env(temp.path());
        ctx.env.set("ARW_ADMIN_TOKEN", "secret");
        let state = build_state(temp.path(), &mut ctx.env).await;
        ctx.env.set("ARW_DEBUG", "0");

        let resp = state_config(HeaderMap::new(), State(state.clone()))
            .await
            .into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let resp = state_config(admin_headers(), State(state))
            .await
            .into_response();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn config_snapshot_endpoints_require_admin() {
        let temp = tempdir().unwrap();
        let mut ctx = begin_state_env(temp.path());
        ctx.env.set("ARW_ADMIN_TOKEN", "secret");
        let state = build_state(temp.path(), &mut ctx.env).await;
        ctx.env.set("ARW_DEBUG", "0");
        let query = Query(HashMap::<String, String>::new());

        let resp = state_config_snapshots(HeaderMap::new(), State(state.clone()), query.clone())
            .await
            .into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let resp = state_config_snapshots(admin_headers(), State(state.clone()), query.clone())
            .await
            .into_response();
        assert_eq!(resp.status(), StatusCode::OK);

        let resp = state_config_snapshot_get(
            HeaderMap::new(),
            State(state.clone()),
            Path("does-not-exist".into()),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let resp = state_config_snapshot_get(admin_headers(), State(state), Path("missing".into()))
            .await
            .into_response();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn patch_validate_requires_admin_and_limits_schema() {
        let temp = tempdir().unwrap();
        let mut ctx = begin_state_env(temp.path());
        ctx.env.set("ARW_ADMIN_TOKEN", "secret");
        let (schema_dir, _) = setup_schema_env(&temp, &mut ctx);
        ctx.env.set("ARW_DEBUG", "0");
        let schema_path = schema_dir.join("minimal.json");
        std::fs::write(&schema_path, br#"{"type":"object"}"#).unwrap();

        let body = Json(ValidateReq {
            schema_ref: schema_path.to_string_lossy().into_owned(),
            schema_pointer: None,
            config: Some(json!({})),
        });

        let resp = patch_validate(HeaderMap::new(), body.clone())
            .await
            .into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let resp = patch_validate(admin_headers(), body).await.into_response();
        assert_eq!(resp.status(), StatusCode::OK);

        let large_path = schema_dir.join("large.json");
        std::fs::write(&large_path, vec![b'0'; MAX_SCHEMA_BYTES + 1]).unwrap();
        let body = Json(ValidateReq {
            schema_ref: large_path.to_string_lossy().into_owned(),
            schema_pointer: None,
            config: Some(json!({})),
        });
        let resp = patch_validate(admin_headers(), body).await.into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn schema_map_requires_admin() {
        let temp = tempdir().unwrap();
        let mut ctx = begin_state_env(temp.path());
        ctx.env.set("ARW_ADMIN_TOKEN", "secret");
        setup_schema_env(&temp, &mut ctx);
        ctx.env.set("ARW_DEBUG", "0");

        let resp = state_schema_map(HeaderMap::new()).await.into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let resp = state_schema_map(admin_headers()).await.into_response();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn patch_infer_schema_requires_admin_and_known_paths() {
        let temp = tempdir().unwrap();
        let mut ctx = begin_state_env(temp.path());
        ctx.env.set("ARW_ADMIN_TOKEN", "secret");
        let (schema_dir, map_path) = setup_schema_env(&temp, &mut ctx);
        ctx.env.set("ARW_DEBUG", "0");

        let mapping = json!({
            "policy": {
                "schema_ref": schema_dir.join("policy.json").to_string_lossy(),
                "pointer_prefix": "policy"
            }
        });
        std::fs::write(&map_path, serde_json::to_vec(&mapping).unwrap()).unwrap();
        std::fs::write(schema_dir.join("policy.json"), br#"{"type":"object"}"#).unwrap();

        let resp = patch_infer_schema(
            HeaderMap::new(),
            Json(InferReq {
                target: "policy.foo".into(),
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let resp = patch_infer_schema(
            admin_headers(),
            Json(InferReq {
                target: "policy.foo".into(),
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::OK);

        let resp = patch_infer_schema(
            admin_headers(),
            Json(InferReq {
                target: "missing.foo".into(),
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn patch_validate_rejects_outside_roots() {
        let temp = tempdir().unwrap();
        let mut ctx = begin_state_env(temp.path());
        ctx.env.set("ARW_ADMIN_TOKEN", "secret");
        setup_schema_env(&temp, &mut ctx);
        ctx.env.set("ARW_DEBUG", "0");

        // Point schema map to a path outside allowed roots.
        ctx.env.set("ARW_SCHEMA_MAP", "/tmp/nonexistent_map.json");

        let body = Json(ValidateReq {
            schema_ref: "/etc/passwd".into(),
            schema_pointer: None,
            config: Some(json!({})),
        });
        let resp = patch_validate(admin_headers(), body).await.into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}

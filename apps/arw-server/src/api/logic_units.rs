use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::{
    extract::{Query, State},
    Json,
};
use jsonschema::{self, Draft};
use serde_json::{json, Value};

use crate::api::config::{
    dot_to_pointer, ensure_path, get_by_dot, infer_schema_for_target, merge_values,
    validate_patch_value,
};
use crate::{capsule_guard, AppState};
use arw_topics as topics;

/// Catalog installed logic units.
#[utoipa::path(
    get,
    path = "/logic-units",
    tag = "Logic Units",
    params(("limit" = Option<i64>, Query)),
    responses(
        (status = 200, body = serde_json::Value),
        (status = 501, description = "Kernel disabled", body = serde_json::Value)
    )
)]
pub async fn logic_units_list(
    State(state): State<AppState>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }
    let limit = q
        .get("limit")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(200);
    match state.kernel().list_logic_units_async(limit).await {
        Ok(items) => Json(json!({"items": items})).into_response(),
        Err(e) => Json(json!({
            "items": Vec::<Value>::new(),
            "error": e.to_string()
        }))
        .into_response(),
    }
}

/// Read-model snapshot of logic units.
#[utoipa::path(
    get,
    path = "/state/logic_units",
    tag = "Logic Units",
    params(("limit" = Option<i64>, Query)),
    responses(
        (status = 200, body = serde_json::Value),
        (status = 501, description = "Kernel disabled", body = serde_json::Value)
    )
)]
pub async fn state_logic_units(
    State(state): State<AppState>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }
    let limit = q
        .get("limit")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(200);
    match state.kernel().list_logic_units_async(limit).await {
        Ok(items) => Json(json!({"items": items})).into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()}),
            ),
        )
            .into_response(),
    }
}

/// Install a logic unit manifest (admin).
#[utoipa::path(
    post,
    path = "/logic-units/install",
    tag = "Logic Units",
    request_body = serde_json::Value,
    responses(
        (status = 201, body = serde_json::Value),
        (status = 401, body = arw_protocol::ProblemDetails),
        (status = 501, description = "Kernel disabled", body = serde_json::Value)
    )
)]
pub async fn logic_units_install(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(mut manifest): Json<Value>,
) -> impl IntoResponse {
    if let Err(resp) = crate::responses::require_admin(&headers).await {
        return *resp;
    }
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }
    let _ = capsule_guard::refresh_capsules(&state).await;
    let id = manifest
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    manifest["id"] = json!(id);
    let _ = state
        .kernel()
        .insert_logic_unit_async(id.clone(), manifest.clone(), "installed".to_string())
        .await;
    state
        .bus()
        .publish(topics::TOPIC_LOGICUNIT_INSTALLED, &json!({"id": id}));
    (
        axum::http::StatusCode::CREATED,
        Json(json!({"id": manifest["id"].clone(), "ok": true})),
    )
        .into_response()
}

/// Apply a logic unit patch set (admin).
#[utoipa::path(
    post,
    path = "/logic-units/apply",
    tag = "Logic Units",
    request_body = serde_json::Value,
    responses(
        (status = 200, body = serde_json::Value),
        (status = 401, body = arw_protocol::ProblemDetails),
        (status = 400, body = arw_protocol::ProblemDetails),
        (status = 501, description = "Kernel disabled", body = serde_json::Value)
    )
)]
pub async fn logic_units_apply(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> axum::response::Response {
    if let Err(resp) = crate::responses::require_admin(&headers).await {
        return *resp;
    }
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }
    let _ = capsule_guard::refresh_capsules(&state).await;
    if let Err(errs) = validate_patch_value(&body) {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(
                json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"invalid patch body", "errors": errs}),
            ),
        )
            .into_response();
    }
    let id = body
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let dry = body
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let mut schema_ref = body
        .get("schema_ref")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let mut schema_pointer = body
        .get("schema_pointer")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let patches = body
        .get("patches")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let safety_issues = crate::patch_guard::check_patches_for_risks(&patches);
    if crate::patch_guard::safety_enforced() && !safety_issues.is_empty() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({"type":"about:blank","title":"Bad Request","status":400,"detail":"patch safety checks failed","issues": safety_issues})),
        )
            .into_response();
    }
    let current_cfg = state.config_state().lock().await.clone();
    let mut cfg = current_cfg.clone();
    let mut diffs: Vec<Value> = Vec::new();
    for p in patches.iter() {
        let target = p.get("target").and_then(|v| v.as_str()).unwrap_or("");
        let op = p.get("op").and_then(|v| v.as_str()).unwrap_or("merge");
        let val = p.get("value").cloned().unwrap_or(json!({}));
        if target.is_empty() {
            continue;
        }
        let mut_root = ensure_path(&mut cfg, target) as *mut Value;
        unsafe {
            let before = get_by_dot(&current_cfg, target).cloned();
            let dst = &mut *mut_root;
            if op == "set" {
                *dst = val;
            } else {
                merge_values(dst, &val);
            }
            let after = get_by_dot(&cfg, target).cloned();
            let pointer = dot_to_pointer(target);
            diffs.push(json!({"target": target, "pointer": pointer, "op": op, "before": before, "after": after}));
        }
    }
    if schema_ref.is_none() {
        if let Some(first) = patches.first() {
            if let Some((p, ptr)) =
                infer_schema_for_target(first.get("target").and_then(|v| v.as_str()).unwrap_or(""))
            {
                schema_ref = Some(p);
                schema_pointer = Some(ptr);
            }
        }
    }
    if let Some(schema_path) = schema_ref.as_deref() {
        let pointer = schema_pointer.as_deref();
        let to_validate = if let Some(ptr) = pointer {
            get_by_dot(&cfg, ptr).cloned().unwrap_or(json!({}))
        } else {
            cfg.clone()
        };
        match tokio::fs::read(schema_path).await {
            Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                Ok(schema_json) => match jsonschema::options()
                    .with_draft(Draft::Draft7)
                    .build(&schema_json)
                {
                    Ok(compiled) => {
                        let errs: Vec<Value> = compiled
                            .iter_errors(&to_validate)
                            .map(|e| {
                                json!({"path": e.instance_path.to_string(), "error": e.to_string()})
                            })
                            .collect();
                        if !errs.is_empty() {
                            return (
                                axum::http::StatusCode::BAD_REQUEST,
                                Json(
                                    json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"schema validation failed", "errors": errs}),
                                ),
                            )
                                .into_response();
                        }
                    }
                    Err(e) => {
                        return (
                            axum::http::StatusCode::BAD_REQUEST,
                            Json(
                                json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"invalid schema", "error": e.to_string()}),
                            ),
                        )
                            .into_response()
                    }
                },
                Err(e) => {
                    return (
                        axum::http::StatusCode::BAD_REQUEST,
                        Json(
                            json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"invalid schema json", "error": e.to_string()}),
                        ),
                    )
                        .into_response()
                }
            },
            Err(e) => {
                return (
                    axum::http::StatusCode::BAD_REQUEST,
                    Json(
                        json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"schema not found", "error": e.to_string()}),
                    ),
                )
                    .into_response()
            }
        }
    }
    if !dry {
        {
            let cfg_state = state.config_state();
            let mut cur = cfg_state.lock().await;
            *cur = cfg.clone();
        }
        crate::config::apply_env_overrides_from(&cfg);
        if !id.is_empty() {
            let snapshot_items: Vec<Value> = state
                .kernel()
                .list_config_snapshots_async(1)
                .await
                .unwrap_or_default();
            let sid = snapshot_items
                .first()
                .and_then(|x| x.get("id").and_then(|v| v.as_str()))
                .map(|s| s.to_string());
            let mut applied_payload = json!({"id": id, "ops": patches.len(), "snapshot_id": sid});
            if !safety_issues.is_empty() {
                applied_payload["safety_issues"] = Value::Array(safety_issues.clone());
            }
            state
                .bus()
                .publish(topics::TOPIC_LOGICUNIT_APPLIED, &applied_payload);
        }
        let mut cfg_event = json!({"ops": patches.len()});
        if !safety_issues.is_empty() {
            cfg_event["safety_issues"] = Value::Array(safety_issues.clone());
        }
        state
            .bus()
            .publish(topics::TOPIC_CONFIG_PATCH_APPLIED, &cfg_event);
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
                json!({"ok": true, "id": if id.is_empty(){Value::Null}else{json!(id)}, "dry_run": false, "config": cfg, "diff_summary": diffs, "json_patch": json_patch, "safety_issues": safety_issues }),
            ),
        )
            .into_response();
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
            json!({"ok": true, "id": if id.is_empty(){Value::Null}else{json!(id)}, "dry_run": true, "config": cfg, "diff_summary": diffs, "json_patch": json_patch, "safety_issues": safety_issues }),
        ),
    )
        .into_response()
}

/// Revert to a config snapshot (admin).
#[utoipa::path(
    post,
    path = "/logic-units/revert",
    tag = "Logic Units",
    request_body = serde_json::Value,
    responses(
        (status = 200, body = serde_json::Value),
        (status = 404),
        (status = 401, body = arw_protocol::ProblemDetails),
        (status = 501, description = "Kernel disabled", body = serde_json::Value)
    )
)]
pub async fn logic_units_revert(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> axum::response::Response {
    if let Err(resp) = crate::responses::require_admin(&headers).await {
        return *resp;
    }
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }
    let _ = capsule_guard::refresh_capsules(&state).await;
    let snap = body
        .get("snapshot_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if snap.is_empty() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(
                json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"missing snapshot_id"}),
            ),
        )
            .into_response();
    }
    let history = state.config_history();
    let mut hist = history.lock().await;
    if let Some((_, cfg)) = hist.iter().rev().find(|(id, _)| id == snap).cloned() {
        let new_id = uuid::Uuid::new_v4().to_string();
        {
            let cfg_state = state.config_state();
            let mut cur = cfg_state.lock().await;
            *cur = cfg.clone();
        }
        crate::config::apply_env_overrides_from(&cfg);
        hist.push((new_id.clone(), cfg.clone()));
        state.bus().publish(
            topics::TOPIC_LOGICUNIT_REVERTED,
            &json!({"snapshot_id": snap, "new_snapshot_id": new_id}),
        );
        (
            axum::http::StatusCode::OK,
            Json(json!({"ok": true, "snapshot_id": new_id, "config": cfg})),
        )
            .into_response()
    } else {
        (
            axum::http::StatusCode::NOT_FOUND,
            Json(json!({"type":"about:blank","title":"Not Found","status":404})),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;
    use crate::test_support::env as test_env;
    use arw_policy::PolicyEngine;
    use arw_protocol::GatingCapsule;
    use arw_wasi::NoopHost;
    use axum::http::{HeaderMap, HeaderValue};
    use serde_json::json;
    use serde_yaml;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tempfile::tempdir;
    use tokio::sync::Mutex;

    async fn build_state(dir: &Path, env_guard: &mut test_env::EnvGuard) -> AppState {
        env_guard.set("ARW_DEBUG", "1");
        env_guard.set("ARW_ADMIN_TOKEN", "local");
        crate::util::reset_state_dir_for_tests();
        env_guard.set("ARW_STATE_DIR", dir.display().to_string());
        let bus = arw_events::Bus::new_with_replay(64, 64);
        let kernel = arw_kernel::Kernel::open(dir).expect("init kernel for tests");
        let policy = PolicyEngine::load_from_env();
        let policy_handle = crate::policy::PolicyHandle::new(policy, bus.clone());
        let host: Arc<dyn arw_wasi::ToolHost> = Arc::new(NoopHost);
        AppState::builder(bus, kernel, policy_handle, host, true)
            .with_config_state(Arc::new(Mutex::new(json!({"mode": "test"}))))
            .with_config_history(Arc::new(Mutex::new(Vec::new())))
            .with_sse_capacity(64)
            .build()
            .await
    }

    fn capsule_with_hops(id: &str, ttl: u32) -> GatingCapsule {
        GatingCapsule {
            id: id.to_string(),
            version: "1".into(),
            issued_at_ms: 0,
            issuer: Some("issuer".into()),
            hop_ttl: Some(ttl),
            propagate: None,
            denies: vec![],
            contracts: vec![],
            lease_duration_ms: Some(60_000),
            renew_within_ms: Some(10_000),
            signature: Some("sig".into()),
        }
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    #[tokio::test]
    async fn install_refreshes_capsules() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = test_support::begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;

        let capsule = capsule_with_hops("logic-refresh", 3);
        state.capsules().adopt(&capsule, now_ms()).await;

        let before = state.capsules().snapshot().await;
        let items = before["items"].as_array().expect("items array");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["remaining_hops"].as_u64(), Some(2));

        let mut headers = HeaderMap::new();
        headers.insert("X-ARW-Admin", HeaderValue::from_static("local"));
        let manifest = json!({"name": "demo"});
        let response = logic_units_install(State(state.clone()), headers, Json(manifest))
            .await
            .into_response();
        assert_eq!(response.status(), axum::http::StatusCode::CREATED);

        let after = state.capsules().snapshot().await;
        let items = after["items"].as_array().expect("items array");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["remaining_hops"].as_u64(), Some(1));
    }

    #[test]
    fn logic_unit_examples_validate_against_schema() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..");
        let schema_path = repo_root.join("spec/schemas/logic_unit_manifest.json");
        let schema_src = std::fs::read_to_string(&schema_path).expect("logic unit schema to load");
        let schema_json: serde_json::Value =
            serde_json::from_str(&schema_src).expect("logic unit schema to parse");
        let schema = jsonschema::options()
            .with_draft(Draft::Draft7)
            .build(&schema_json)
            .expect("logic unit schema to compile");

        let examples_dir = repo_root.join("examples/logic-units");
        let mut checked = 0usize;
        for entry in std::fs::read_dir(&examples_dir).expect("examples/logic-units dir to exist") {
            let entry = entry.expect("read_dir entries accessible");
            let path = entry.path();
            if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                continue;
            }
            let ext = path
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or_default();
            if !matches!(ext.to_ascii_lowercase().as_str(), "yaml" | "yml" | "json") {
                continue;
            }
            let raw = std::fs::read_to_string(&path).expect("logic unit manifest to load");
            let manifest: serde_json::Value = if ext.eq_ignore_ascii_case("json") {
                serde_json::from_str(&raw).expect("logic unit manifest json to parse")
            } else {
                serde_yaml::from_str(&raw).expect("logic unit manifest yaml to parse")
            };
            let joined: Vec<_> = schema
                .iter_errors(&manifest)
                .map(|err| format!("{}: {}", err.instance_path, err))
                .collect();
            if !joined.is_empty() {
                panic!(
                    "example {} failed logic unit schema validation: {}",
                    path.display(),
                    joined.join("; ")
                );
            }
            checked += 1;
        }
        assert!(
            checked > 0,
            "expected at least one logic unit example to validate"
        );
    }
}

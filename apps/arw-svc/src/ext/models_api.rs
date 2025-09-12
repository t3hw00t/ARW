use super::super::resources::models_service::ModelsService;
use arw_core::gating;
use crate::AppState;
use arw_macros::{arw_admin, arw_gate};
use axum::{extract::State, response::IntoResponse, Json};
use serde::Deserialize;
use serde_json::json;
use axum::extract::Path;
use axum::http::{HeaderMap, HeaderValue};

#[arw_admin(method = "GET", path = "/admin/models", summary = "List models")]
#[arw_gate("models:list")]
pub(crate) async fn list_models(State(state): State<AppState>) -> impl IntoResponse {
    let Some(svc) = state.resources.get::<ModelsService>() else {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "ModelsService missing",
        )
            .into_response();
    };
    let list: Vec<serde_json::Value> = svc.list().await;
    Json::<Vec<serde_json::Value>>(list).into_response()
}
#[arw_admin(
    method = "POST",
    path = "/admin/models/refresh",
    summary = "Refresh model list"
)]
#[arw_gate("models:refresh")]
pub(crate) async fn refresh_models(State(state): State<AppState>) -> impl IntoResponse {
    let Some(svc) = state.resources.get::<ModelsService>() else {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "ModelsService missing",
        )
            .into_response();
    };
    let list: Vec<serde_json::Value> = svc.refresh(&state).await;
    Json::<Vec<serde_json::Value>>(list).into_response()
}
#[arw_admin(
    method = "POST",
    path = "/admin/models/save",
    summary = "Save models to disk"
)]
#[arw_gate("models:save")]
pub(crate) async fn models_save() -> impl IntoResponse {
    // Write via ext::io directly (no service needed)
    match super::super::ext::io::save_json_file_async(
        &super::super::ext::paths::models_path(),
        &serde_json::Value::Array(super::super::ext::models().read().await.clone()),
    )
    .await
    {
        Ok(_) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("save failed: {}", e),
        )
            .into_response(),
    }
}
#[arw_admin(
    method = "POST",
    path = "/admin/models/load",
    summary = "Load models from disk"
)]
#[arw_gate("models:load")]
pub(crate) async fn models_load() -> impl IntoResponse {
    match super::super::ext::io::load_json_file_async(&super::super::ext::paths::models_path())
        .await
        .and_then(|v| v.as_array().cloned())
    {
        Some(arr) => Json::<Vec<serde_json::Value>>(arr).into_response(),
        None => (axum::http::StatusCode::NOT_FOUND, "no models.json").into_response(),
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct ModelId {
    id: String,
    #[serde(default)]
    provider: Option<String>,
}
#[arw_admin(
    method = "POST",
    path = "/admin/models/add",
    summary = "Add model entry"
)]
#[arw_gate("models:add")]
pub(crate) async fn models_add(
    State(state): State<AppState>,
    Json(req): Json<ModelId>,
) -> impl IntoResponse {
    let Some(svc) = state.resources.get::<ModelsService>() else {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "ModelsService missing",
        )
            .into_response();
    };
    svc.add(&state, req.id, req.provider).await;
    Json(serde_json::json!({"ok": true})).into_response()
}
#[arw_admin(
    method = "POST",
    path = "/admin/models/delete",
    summary = "Delete model entry"
)]
#[arw_gate("models:delete")]
pub(crate) async fn models_delete(
    State(state): State<AppState>,
    Json(req): Json<ModelId>,
) -> impl IntoResponse {
    let Some(svc) = state.resources.get::<ModelsService>() else {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "ModelsService missing",
        )
            .into_response();
    };
    svc.delete(&state, req.id).await;
    Json(serde_json::json!({"ok": true})).into_response()
}
#[arw_admin(
    method = "GET",
    path = "/admin/models/default",
    summary = "Get default model"
)]
#[arw_gate("models:default:get")]
pub(crate) async fn models_default_get(State(state): State<AppState>) -> impl IntoResponse {
    let Some(svc) = state.resources.get::<ModelsService>() else {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "ModelsService missing",
        )
            .into_response();
    };
    let id = svc.default_get().await;
    Json(serde_json::json!({"default": id})).into_response()
}
#[arw_admin(
    method = "POST",
    path = "/admin/models/default",
    summary = "Set default model"
)]
#[arw_gate("models:default:set")]
pub(crate) async fn models_default_set(
    State(state): State<AppState>,
    Json(req): Json<ModelId>,
) -> impl IntoResponse {
    let Some(svc) = state.resources.get::<ModelsService>() else {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "ModelsService missing",
        )
            .into_response();
    };
    let ok = svc.default_set(&state, req.id).await.is_ok();
    Json(serde_json::json!({"ok": ok})).into_response()
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct BudgetOverrideReq {
    #[serde(default)]
    soft_ms: Option<u64>,
    #[serde(default)]
    hard_ms: Option<u64>,
    #[serde(default)]
    class: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct DownloadReq {
    id: String,
    url: String,
    #[serde(default)]
    provider: Option<String>,
    // Integrity is mandatory: require sha256 for all downloads
    sha256: String,
    #[serde(default)]
    budget: Option<BudgetOverrideReq>,
}
#[arw_admin(
    method = "POST",
    path = "/admin/models/download",
    summary = "Download model file"
)]
#[arw_gate("models:download")]
pub(crate) async fn models_download(
    State(state): State<AppState>,
    Json(req): Json<DownloadReq>,
) -> impl IntoResponse {
    // Egress policy gate (coarse). Deny if not allowed.
    if !gating::allowed("io:egress:models.download") {
        return super::ApiError::forbidden("gated:egress").into_response();
    }
    let Some(svc) = state.resources.get::<ModelsService>() else {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "ModelsService missing",
        )
            .into_response();
    };
    let budget_override =
        req.budget.map(
            |b| crate::resources::models_service::DownloadBudgetOverride {
                soft_ms: b.soft_ms,
                hard_ms: b.hard_ms,
                class: b.class,
            },
        );
    match svc
        .download_with_budget(
            &state,
            req.id,
            req.url,
            req.provider,
            Some(req.sha256),
            budget_override,
        )
        .await
    {
        Ok(()) => super::ok(serde_json::json!({})).into_response(),
        Err(e) => super::ApiError::bad_request(&e).into_response(),
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct CancelReq {
    id: String,
}

#[arw_admin(
    method = "POST",
    path = "/admin/models/download/cancel",
    summary = "Cancel model download"
)]
#[arw_gate("models:download")]
pub(crate) async fn models_download_cancel(
    State(state): State<AppState>,
    Json(req): Json<CancelReq>,
) -> impl IntoResponse {
    let Some(svc) = state.resources.get::<ModelsService>() else {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "ModelsService missing",
        )
            .into_response();
    };
    svc.cancel_download(&state, req.id).await;
    super::ok(serde_json::json!({})).into_response()
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct CasGcReq {
    #[serde(default = "CasGcReq::default_ttl")] 
    ttl_days: u64,
}
impl CasGcReq { fn default_ttl() -> u64 { 7 } }

/// Run a one-off GC of models/by-hash, removing unreferenced blobs older than ttl_days.
#[arw_admin(
    method = "POST",
    path = "/admin/models/cas_gc",
    summary = "Run CAS GC once (delete stale blobs)"
)]
#[arw_gate("models:cas_gc")]
pub(crate) async fn models_cas_gc(State(state): State<AppState>, Json(req): Json<CasGcReq>) -> impl IntoResponse {
    ModelsService::cas_gc_once(&state.bus, req.ttl_days).await;
    super::ok(serde_json::json!({"started": true, "ttl_days": req.ttl_days})).into_response()
}

// Public read-model: summarize installed model hashes for clustering/ads.
#[arw_gate("state:models_hashes:get")]
pub(crate) async fn models_hashes_get(State(_state): State<AppState>) -> impl IntoResponse {
    use std::collections::{HashMap, HashSet};
    let models = crate::ext::models().read().await.clone();
    let mut by_hash: HashMap<String, (u64, String, HashSet<String>)> = HashMap::new();
    for m in models.into_iter() {
        let sh = match m.get("sha256").and_then(|v| v.as_str()) {
            Some(s) if s.len() == 64 => s.to_string(),
            _ => continue,
        };
        let path = m
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let bytes = m
            .get("bytes")
            .and_then(|v| v.as_u64())
            .unwrap_or(0u64);
        let prov = m
            .get("provider")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let entry = by_hash
            .entry(sh)
            .or_insert((bytes, path.clone(), HashSet::new()));
        if entry.0 == 0 && bytes > 0 {
            entry.0 = bytes;
        }
        if entry.1.is_empty() && !path.is_empty() {
            entry.1 = path;
        }
        entry.2.insert(prov);
    }
    let mut items = Vec::with_capacity(by_hash.len());
    for (sha256, (bytes, path, providers)) in by_hash.into_iter() {
        items.push(json!({
            "sha256": sha256,
            "bytes": bytes,
            "path": path,
            "providers": providers.into_iter().collect::<Vec<_>>()
        }));
    }
    Json(json!({"count": items.len(), "items": items})).into_response()
}

// Admin: serve a CAS blob by hash (gated). Intended for invited peers.
#[arw_admin(
    method = "GET",
    path = "/admin/models/by-hash/:sha256",
    summary = "Serve model blob by sha256 (egress gated)"
)]
#[arw_gate("io:egress:models.peer")]
pub(crate) async fn models_blob_get(Path(sha256): Path<String>) -> impl IntoResponse {
    // Validate hash
    let ok = sha256.len() == 64 && sha256.chars().all(|c| c.is_ascii_hexdigit());
    if !ok {
        return (axum::http::StatusCode::BAD_REQUEST, "invalid sha256").into_response();
    }
    // Find matching CAS file in models/by-hash (sha256 or sha256.ext)
    let dir = crate::ext::paths::state_dir().join("models").join("by-hash");
    let mut found: Option<std::path::PathBuf> = None;
    if let Ok(mut rd) = tokio::fs::read_dir(&dir).await {
        while let Ok(Some(ent)) = rd.next_entry().await {
            let f = ent.file_name();
            let name = f.to_string_lossy();
            if name == sha256 || name.starts_with(&format!("{}.", sha256)) {
                found = Some(ent.path());
                break;
            }
        }
    }
    let Some(path) = found else {
        return axum::http::StatusCode::NOT_FOUND.into_response();
    };
    match tokio::fs::File::open(&path).await {
        Ok(file) => {
            let meta = tokio::fs::metadata(&path).await.ok();
            let stream = tokio_util::io::ReaderStream::new(file);
            let body = axum::body::Body::from_stream(stream);
            let mut headers = HeaderMap::new();
            headers.insert(
                axum::http::header::CONTENT_TYPE,
                HeaderValue::from_static("application/octet-stream"),
            );
            if let Some(m) = meta {
                headers.insert(
                    axum::http::header::CONTENT_LENGTH,
                    HeaderValue::from_str(&m.len().to_string()).unwrap_or(HeaderValue::from_static("0")),
                );
            }
            (headers, body).into_response()
        }
        Err(_) => axum::http::StatusCode::NOT_FOUND.into_response(),
    }
}

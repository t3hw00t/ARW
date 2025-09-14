use super::super::resources::models_service::ModelsService;
use crate::AppState;
use arw_core::gating;
use arw_macros::{arw_admin, arw_gate};
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::http::{HeaderMap, HeaderValue};
use axum::{extract::State, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ModelsConcurrency {
    configured_max: u64,
    available_permits: u64,
    held_permits: u64,
    #[serde(default)]
    hard_cap: Option<u64>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ModelsMetrics {
    started: u64,
    queued: u64,
    admitted: u64,
    resumed: u64,
    canceled: u64,
    completed: u64,
    completed_cached: u64,
    errors: u64,
    bytes_total: u64,
    #[serde(default)]
    ewma_mbps: Option<f64>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ModelItem {
    id: String,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    sha256: Option<String>,
    #[serde(default)]
    bytes: Option<u64>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    error_code: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ModelsSummary {
    items: Vec<ModelItem>,
    #[serde(default)]
    default: String,
    concurrency: ModelsConcurrency,
    metrics: ModelsMetrics,
}

/// Aggregate models state in one call for UI: items, default, concurrency, metrics
#[arw_admin(
    method = "GET",
    path = "/admin/models/summary",
    summary = "Summarize models state (items/default/concurrency/metrics)"
)]
#[arw_gate("models:summary")]
pub(crate) async fn models_summary(State(state): State<AppState>) -> impl IntoResponse {
    use tokio::join;
    let svc = match super::require_service::<ModelsService>(&state) {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    let models_fut = async {
        let arr = super::models().read().await.clone();
        arr.into_iter()
            .filter_map(|m| {
                let id = m.get("id").and_then(|v| v.as_str()).map(|s| s.to_string());
                let id = match id {
                    Some(v) => v,
                    None => return None,
                };
                let provider = m
                    .get("provider")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let path = m
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let sha256 = m
                    .get("sha256")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let bytes = m.get("bytes").and_then(|v| v.as_u64());
                let status = m
                    .get("status")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let error_code = m
                    .get("error_code")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                Some(ModelItem {
                    id,
                    provider,
                    path,
                    sha256,
                    bytes,
                    status,
                    error_code,
                })
            })
            .collect::<Vec<_>>()
    };
    let default_fut = async { super::default_model().read().await.clone() };
    let conc_fut = async {
        let v = svc.concurrency_get().await;
        ModelsConcurrency {
            configured_max: v
                .get("configured_max")
                .and_then(|x| x.as_u64())
                .unwrap_or_default(),
            available_permits: v
                .get("available_permits")
                .and_then(|x| x.as_u64())
                .unwrap_or_default(),
            held_permits: v
                .get("held_permits")
                .and_then(|x| x.as_u64())
                .unwrap_or_default(),
            hard_cap: v.get("hard_cap").and_then(|x| x.as_u64()),
        }
    };
    let metrics_fut = async {
        let base = svc.downloads_metrics().await;
        ModelsMetrics {
            started: base
                .get("started")
                .and_then(|x| x.as_u64())
                .unwrap_or_default(),
            queued: base
                .get("queued")
                .and_then(|x| x.as_u64())
                .unwrap_or_default(),
            admitted: base
                .get("admitted")
                .and_then(|x| x.as_u64())
                .unwrap_or_default(),
            resumed: base
                .get("resumed")
                .and_then(|x| x.as_u64())
                .unwrap_or_default(),
            canceled: base
                .get("canceled")
                .and_then(|x| x.as_u64())
                .unwrap_or_default(),
            completed: base
                .get("completed")
                .and_then(|x| x.as_u64())
                .unwrap_or_default(),
            completed_cached: base
                .get("completed_cached")
                .and_then(|x| x.as_u64())
                .unwrap_or_default(),
            errors: base
                .get("errors")
                .and_then(|x| x.as_u64())
                .unwrap_or_default(),
            bytes_total: base
                .get("bytes_total")
                .and_then(|x| x.as_u64())
                .unwrap_or_default(),
            ewma_mbps: base.get("ewma_mbps").and_then(|x| x.as_f64()),
        }
    };
    let (items, default, concurrency, metrics) =
        join!(models_fut, default_fut, conc_fut, metrics_fut);
    super::ok(ModelsSummary {
        items,
        default,
        concurrency,
        metrics,
    })
    .into_response()
}

#[arw_admin(method = "GET", path = "/admin/models", summary = "List models")]
#[arw_gate("models:list")]
pub(crate) async fn list_models(State(state): State<AppState>) -> impl IntoResponse {
    let svc = match super::require_service::<ModelsService>(&state) {
        Ok(s) => s,
        Err(e) => return e.into_response(),
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
    let svc = match super::require_service::<ModelsService>(&state) {
        Ok(s) => s,
        Err(e) => return e.into_response(),
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
pub(crate) async fn models_save(State(state): State<AppState>) -> impl IntoResponse {
    let svc = match super::require_service::<ModelsService>(&state) {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    match svc.save().await {
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
pub(crate) async fn models_load(State(state): State<AppState>) -> impl IntoResponse {
    let svc = match super::require_service::<ModelsService>(&state) {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    match svc.load().await {
        Ok(arr) => Json::<Vec<serde_json::Value>>(arr).into_response(),
        Err(_) => (axum::http::StatusCode::NOT_FOUND, "no models.json").into_response(),
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
    let svc = match super::require_service::<ModelsService>(&state) {
        Ok(s) => s,
        Err(e) => return e.into_response(),
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
    let svc = match super::require_service::<ModelsService>(&state) {
        Ok(s) => s,
        Err(e) => return e.into_response(),
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
    let svc = match super::require_service::<ModelsService>(&state) {
        Ok(s) => s,
        Err(e) => return e.into_response(),
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
    let svc = match super::require_service::<ModelsService>(&state) {
        Ok(s) => s,
        Err(e) => return e.into_response(),
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
    let svc = match super::require_service::<ModelsService>(&state) {
        Ok(s) => s,
        Err(e) => return e.into_response(),
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
    let svc = match super::require_service::<ModelsService>(&state) {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    svc.cancel_download(&state, req.id).await;
    super::ok(serde_json::json!({})).into_response()
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct CasGcReq {
    #[serde(default = "CasGcReq::default_ttl")]
    ttl_days: u64,
}
impl CasGcReq {
    fn default_ttl() -> u64 {
        7
    }
}

/// Run a one-off GC of models/by-hash, removing unreferenced blobs older than ttl_days.
#[arw_admin(
    method = "POST",
    path = "/admin/models/cas_gc",
    summary = "Run CAS GC once (delete stale blobs)"
)]
#[arw_gate("models:cas_gc")]
pub(crate) async fn models_cas_gc(
    State(state): State<AppState>,
    Json(req): Json<CasGcReq>,
) -> impl IntoResponse {
    ModelsService::cas_gc_once(&state.bus, req.ttl_days).await;
    super::ok(serde_json::json!({"started": true, "ttl_days": req.ttl_days})).into_response()
}

// (legacy models migrate endpoint removed)

// Public read-model: summarize installed model hashes for clustering/ads.
#[derive(Deserialize)]
pub(crate) struct ModelsHashesQs {
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    sort: Option<String>, // bytes | sha256 | path | providers_count
    #[serde(default)]
    order: Option<String>, // asc | desc
}

#[arw_admin(
    method = "GET",
    path = "/admin/state/models_hashes",
    summary = "List installed model hashes (paginated)"
)]
#[arw_gate("state:models_hashes:get")]
pub(crate) async fn models_hashes_get(
    State(_state): State<AppState>,
    Query(q): Query<ModelsHashesQs>,
) -> impl IntoResponse {
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
        let bytes = m.get("bytes").and_then(|v| v.as_u64()).unwrap_or(0u64);
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
    // Optional provider filter
    if let Some(p) = q.provider.as_deref() {
        let prov = p.to_string();
        items.retain(|it| {
            it["providers"]
                .as_array()
                .map(|arr| arr.iter().any(|v| v.as_str() == Some(prov.as_str())))
                .unwrap_or(false)
        });
    }
    // Sorting
    let sort_key = q
        .sort
        .as_deref()
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_else(|| "bytes".to_string());
    let desc_default = sort_key == "bytes"; // default desc for bytes, asc otherwise
    let order_s = q.order.as_deref().map(|s| s.to_ascii_lowercase());
    let desc = match order_s.as_deref() {
        Some("asc") => false,
        Some("desc") => true,
        _ => desc_default,
    };
    items.sort_by(|a, b| {
        let ord = match sort_key.as_str() {
            "sha256" => a["sha256"].as_str().cmp(&b["sha256"].as_str()),
            "path" => a["path"].as_str().cmp(&b["path"].as_str()),
            "providers_count" => a["providers"]
                .as_array()
                .map(|x| x.len())
                .cmp(&b["providers"].as_array().map(|x| x.len())),
            _ => a["bytes"].as_u64().cmp(&b["bytes"].as_u64()),
        };
        if desc {
            ord.reverse()
        } else {
            ord
        }
    });
    // Pagination
    let total = items.len();
    let offset = q.offset.unwrap_or(0).min(total);
    let limit = q.limit.unwrap_or(200).clamp(1, 10_000);
    let end = offset.saturating_add(limit).min(total);
    let page = items[offset..end].to_vec();
    Json(json!({
        "total": total,
        "count": page.len(),
        "limit": limit,
        "offset": offset,
        "items": page
    }))
    .into_response()
}

// Admin: serve a CAS blob by hash (gated). Intended for invited peers.
#[arw_admin(
    method = "GET",
    path = "/admin/models/by-hash/:sha256",
    summary = "Serve model blob by sha256 (egress gated)"
)]
#[arw_gate("io:egress:models.peer")]
#[utoipa::path(
    get,
    path = "/models/blob/{sha256}",
    tag = "Public",
    operation_id = "models_blob_get_doc",
    params(("sha256" = String, Path, description = "Hex lowercase SHA-256 (64 chars)")),
    responses(
        (status=200, description="CAS blob bytes (ETag/immutable cache)", body=String),
        (status=304, description="Not Modified (If-None-Match)"),
        (status=400, description="Invalid sha256", body = arw_protocol::ProblemDetails),
        (status=404, description="Not found")
    )
)]
pub(crate) async fn models_blob_get(
    headers_in: HeaderMap,
    Path(sha256): Path<String>,
) -> impl IntoResponse {
    // Validate hash
    let ok = sha256.len() == 64 && sha256.chars().all(|c| c.is_ascii_hexdigit());
    if !ok {
        return (axum::http::StatusCode::BAD_REQUEST, "invalid sha256").into_response();
    }
    // Find matching CAS file in models/by-hash (sha256 or sha256.ext)
    let dir = crate::ext::paths::state_dir()
        .join("models")
        .join("by-hash");
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
        Ok(mut file) => {
            use tokio::io::{AsyncReadExt, AsyncSeekExt};
            let meta = tokio::fs::metadata(&path).await.ok();
            let total_len = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let mut headers = HeaderMap::new();
            headers.insert(
                axum::http::header::CONTENT_TYPE,
                HeaderValue::from_static("application/octet-stream"),
            );
            // Strong validators for immutable CAS blobs
            let etag_val = format!("\"{}\"", sha256);
            if let Ok(h) = HeaderValue::from_str(&etag_val) {
                headers.insert(axum::http::header::ETAG, h);
            }
            // Long-lived immutable cache control (blob addressed by digest)
            headers.insert(
                axum::http::header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=31536000, immutable"),
            );
            if let Some(m) = meta.as_ref() {
                // Last-Modified from file mtime (best-effort)
                if let Ok(modified) = m.modified() {
                    let dt = chrono::DateTime::<chrono::Utc>::from(modified).to_rfc2822();
                    if let Ok(h) = HeaderValue::from_str(&dt) {
                        headers.insert(axum::http::header::LAST_MODIFIED, h);
                    }
                }
            }
            // If-None-Match handling (304 Not Modified)
            if let Some(inm) = headers_in.get(axum::http::header::IF_NONE_MATCH) {
                if inm
                    .to_str()
                    .ok()
                    .map(|s| s.contains(&etag_val))
                    .unwrap_or(false)
                {
                    return (StatusCode::NOT_MODIFIED, headers).into_response();
                }
            }

            // Range support: bytes=start-end | bytes=start- | bytes=-suffix
            let range_hdr = headers_in
                .get(axum::http::header::RANGE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .trim()
                .to_string();
            if !range_hdr.is_empty() && total_len > 0 {
                let prefix = "bytes=";
                if let Some(spec) = range_hdr.strip_prefix(prefix) {
                    let mut start: Option<u64> = None;
                    let mut end: Option<u64> = None;
                    if let Some(hy) = spec.find('-') {
                        let (a, b) = spec.split_at(hy);
                        let b = &b[1..];
                        if a.is_empty() {
                            // suffix: bytes=-N
                            if let Ok(n) = b.parse::<u64>() {
                                let n = n.min(total_len);
                                start = Some(total_len.saturating_sub(n));
                                end = Some(total_len.saturating_sub(1));
                            }
                        } else if b.is_empty() {
                            // bytes=START-
                            if let Ok(sv) = a.parse::<u64>() {
                                start = Some(sv);
                                end = Some(total_len.saturating_sub(1));
                            }
                        } else {
                            // bytes=START-END
                            if let (Ok(sv), Ok(ev)) = (a.parse::<u64>(), b.parse::<u64>()) {
                                start = Some(sv);
                                end = Some(ev);
                            }
                        }
                    }
                    if let (Some(s), Some(e)) = (start, end) {
                        if s <= e && e < total_len {
                            // valid range
                            let len = e - s + 1;
                            let _ = file.seek(std::io::SeekFrom::Start(s)).await;
                            let reader = file.take(len);
                            let stream = tokio_util::io::ReaderStream::new(reader);
                            let body = axum::body::Body::from_stream(stream);
                            headers.insert(
                                axum::http::header::CONTENT_RANGE,
                                HeaderValue::from_str(&format!("bytes {}-{}/{}", s, e, total_len))
                                    .unwrap_or(HeaderValue::from_static("")),
                            );
                            headers.insert(
                                axum::http::header::CONTENT_LENGTH,
                                HeaderValue::from_str(&len.to_string())
                                    .unwrap_or(HeaderValue::from_static("0")),
                            );
                            return (StatusCode::PARTIAL_CONTENT, headers, body).into_response();
                        } else {
                            // 416 Range Not Satisfiable
                            headers.insert(
                                axum::http::header::CONTENT_RANGE,
                                HeaderValue::from_str(&format!("bytes */{}", total_len))
                                    .unwrap_or(HeaderValue::from_static("")),
                            );
                            return (StatusCode::RANGE_NOT_SATISFIABLE, headers).into_response();
                        }
                    }
                }
            }
            // Full body
            if let Some(m) = meta.as_ref() {
                headers.insert(
                    axum::http::header::CONTENT_LENGTH,
                    HeaderValue::from_str(&m.len().to_string())
                        .unwrap_or(HeaderValue::from_static("0")),
                );
            }
            let stream = tokio_util::io::ReaderStream::new(file);
            let body = axum::body::Body::from_stream(stream);
            (headers, body).into_response()
        }
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Lightweight downloads metrics (throughput EWMA for admission checks)
#[arw_admin(
    method = "GET",
    path = "/admin/models/downloads_metrics",
    summary = "Get downloads metrics (EWMA MB/s)"
)]
#[arw_gate("state:downloads_metrics:get")]
pub(crate) async fn models_downloads_metrics(State(state): State<AppState>) -> impl IntoResponse {
    let svc = match super::require_service::<ModelsService>(&state) {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    let v = svc.downloads_metrics().await;
    Json(v).into_response()
}

/// Get current models CAS quota and usage snapshot
#[arw_admin(
    method = "GET",
    path = "/admin/models/quota",
    summary = "Get models CAS quota and usage"
)]
#[arw_gate("models:quota:get")]
pub(crate) async fn models_quota_get(State(state): State<AppState>) -> impl IntoResponse {
    let svc = match super::require_service::<ModelsService>(&state) {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    let v = svc.quota_status().await;
    Json(v).into_response()
}

#[cfg(test)]
mod tests {
    use super::{models_downloads_metrics, models_quota_get};
    use crate::AppState;
    use axum::{http::Request, routing::get, Router};
    use http_body_util::BodyExt; // for collecting body
    use tower::ServiceExt; // for `oneshot`

    #[tokio::test]
    async fn http_downloads_metrics_shape() {
        // Build minimal app with the handler and a state containing ModelsService
        let state = {
            let st = AppState::default();
            st.resources.insert(std::sync::Arc::new(
                crate::resources::models_service::ModelsService::new(),
            ));
            st
        };
        let app = Router::new()
            .route(
                "/admin/models/downloads_metrics",
                get(models_downloads_metrics),
            )
            .with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/admin/models/downloads_metrics")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(resp.status().is_success());
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        // Verify shape contains ewma and counters
        assert!(v.get("ewma_mbps").is_some());
        for k in [
            "started",
            "queued",
            "admitted",
            "resumed",
            "canceled",
            "completed",
            "completed_cached",
            "errors",
            "bytes_total",
        ] {
            assert!(v.get(k).is_some(), "missing key: {}", k);
        }
    }

    #[tokio::test]
    async fn http_models_quota_shape() {
        // Minimal app wiring for the quota endpoint
        let state = {
            let st = AppState::default();
            st.resources.insert(std::sync::Arc::new(
                crate::resources::models_service::ModelsService::new(),
            ));
            st
        };
        let app = Router::new()
            .route("/admin/models/quota", get(models_quota_get))
            .with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/admin/models/quota")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(resp.status().is_success());
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        // Verify shape contains basic keys
        for k in ["dir", "files", "used_bytes", "used_mb", "over_quota"] {
            assert!(v.get(k).is_some(), "missing key: {}", k);
        }
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct ConcurrencySetReq {
    max: usize,
    #[serde(default)]
    block: Option<bool>,
}
/// Set download concurrency at runtime
#[arw_admin(
    method = "POST",
    path = "/admin/models/concurrency",
    summary = "Set models download concurrency"
)]
#[arw_gate("models:concurrency:set")]
pub(crate) async fn models_concurrency_set(
    State(state): State<AppState>,
    Json(req): Json<ConcurrencySetReq>,
) -> impl IntoResponse {
    let svc = match super::require_service::<ModelsService>(&state) {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    let block = req.block.unwrap_or(true);
    match svc.concurrency_set(&state, req.max, block).await {
        Ok(v) => super::ok(v).into_response(),
        Err(e) => super::ApiError::internal(&e).into_response(),
    }
}

/// Get current models download concurrency settings
#[arw_admin(
    method = "GET",
    path = "/admin/models/concurrency",
    summary = "Get models download concurrency"
)]
#[arw_gate("models:concurrency:get")]
pub(crate) async fn models_concurrency_get(State(state): State<AppState>) -> impl IntoResponse {
    let Some(svc) = state.resources.get::<ModelsService>() else {
        return super::ApiError::internal("ModelsService missing").into_response();
    };
    let v = svc.concurrency_get().await;
    Json(v).into_response()
}

/// Admin: snapshot of active download jobs and inflight hashes
#[arw_admin(
    method = "GET",
    path = "/admin/models/jobs",
    summary = "List active jobs and inflight hashes"
)]
#[arw_gate("models:jobs")]
pub(crate) async fn models_jobs(State(state): State<AppState>) -> impl IntoResponse {
    let Some(svc) = state.resources.get::<ModelsService>() else {
        return super::ApiError::internal("ModelsService missing").into_response();
    };
    let v = svc.jobs_status().await;
    Json(v).into_response()
}

/// Read-model: models download metrics (counters + EWMA MB/s)
#[arw_admin(
    method = "GET",
    path = "/admin/state/models_metrics",
    summary = "Get models download metrics"
)]
#[arw_gate("state:models_metrics:get")]
pub(crate) async fn models_metrics_get(State(state): State<AppState>) -> impl IntoResponse {
    // Use the service helper to return a consistent shape (counters + ewma)
    match super::require_service::<ModelsService>(&state) {
        Ok(svc) => Json(svc.downloads_metrics().await).into_response(),
        Err(e) => e.into_response(),
    }
}
// SPDX-License-Identifier: MIT OR Apache-2.0

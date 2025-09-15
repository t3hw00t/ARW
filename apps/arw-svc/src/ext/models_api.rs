//! Models API — admin endpoints for the models lifecycle
//!
//! Updated: 2025-09-14
//!
//! Overview
//! - List, refresh, save, and load models; manage the default; trigger
//!   downloads with mandatory SHA‑256 integrity.
//! - Get/Set download concurrency; run a CAS GC pass.
//! - Provide read-model snapshots for download metrics and installed hashes.
//! - Serve CAS blobs by SHA‑256 (immutable; ETag-aware).
//!
//! Events & read-models
//! - Publishes `models.changed`, `models.download.progress`, `models.cas.gc`, and
//!   `models.manifest.written` via the service.
//! - Read-models: `models` (items + default) and `models_metrics` (counters + `ewma_mbps`).
//!
//! Gating
//! - All admin endpoints are gated via `#[arw_admin]` and `#[arw_gate]`.
//! - Egress: downloads require `io:egress:models.download`; blob serving requires
//!   `io:egress:models.peer`.
//!
//! See also
//! - Service logic and progress/status codes: `crate::resources::models_service`.
//! - Topics reference: `docs/reference/topics.md`.
use super::super::resources::models_service::ModelsService;
use super::http::{build_blob_headers, is_not_modified, parse_range_spec};
use crate::AppState;
use arw_core::gating;
use arw_macros::{arw_admin, arw_gate};
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::http::{HeaderMap, HeaderValue};
use axum::{extract::State, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};

/// Runtime download concurrency snapshot (configured vs. available), optionally
/// including a hard cap from `ARW_MODELS_MAX_CONC_HARD`.
#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ModelsConcurrency {
    configured_max: u64,
    available_permits: u64,
    held_permits: u64,
    #[serde(default)]
    hard_cap: Option<u64>,
    #[serde(default)]
    pending_shrink: Option<u64>,
}

/// Aggregated counters and throughput estimate for downloads. Matches the
/// shape returned by `ModelsService::downloads_metrics`.
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

/// Active download job pair.
#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ActiveJob {
    model_id: String,
    job_id: String,
}

/// Snapshot of downloader jobs and inflight hashes plus a concurrency view.
#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ModelsJobs {
    active: Vec<ActiveJob>,
    inflight_hashes: Vec<String>,
    concurrency: ModelsConcurrency,
}

/// Item in the installed-hashes summary.
#[derive(Clone, Serialize, utoipa::ToSchema)]
pub(crate) struct HashItem {
    sha256: String,
    bytes: u64,
    path: String,
    providers: Vec<String>,
}

/// Paginated page of installed-hash items.
#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct HashesPage {
    total: usize,
    count: usize,
    limit: usize,
    offset: usize,
    items: Vec<HashItem>,
}

/// Item in the models list. Many fields are optional while downloading or
/// when entries were added before materialization.
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

/// Composite summary for UI: models items, default id, concurrency, metrics.
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
    let svc = match super::require_service::<ModelsService>(&state) {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    let v = svc.summary_value().await;
    // Map to typed struct for OpenAPI schema stability
    let items = v
        .get("items")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    let id = m.get("id").and_then(|v| v.as_str())?.to_string();
                    Some(ModelItem {
                        id,
                        provider: m
                            .get("provider")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        path: m
                            .get("path")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        sha256: m
                            .get("sha256")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        bytes: m.get("bytes").and_then(|v| v.as_u64()),
                        status: m
                            .get("status")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        error_code: m
                            .get("error_code")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let default = v
        .get("default")
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_string();
    let c = v.get("concurrency").cloned().unwrap_or_default();
    let concurrency = ModelsConcurrency {
        configured_max: c
            .get("configured_max")
            .and_then(|x| x.as_u64())
            .unwrap_or_default(),
        available_permits: c
            .get("available_permits")
            .and_then(|x| x.as_u64())
            .unwrap_or_default(),
        held_permits: c
            .get("held_permits")
            .and_then(|x| x.as_u64())
            .unwrap_or_default(),
        hard_cap: c.get("hard_cap").and_then(|x| x.as_u64()),
        pending_shrink: c.get("pending_shrink").and_then(|x| x.as_u64()),
    };
    let m = v.get("metrics").cloned().unwrap_or_default();
    let metrics = ModelsMetrics {
        started: m
            .get("started")
            .and_then(|x| x.as_u64())
            .unwrap_or_default(),
        queued: m.get("queued").and_then(|x| x.as_u64()).unwrap_or_default(),
        admitted: m
            .get("admitted")
            .and_then(|x| x.as_u64())
            .unwrap_or_default(),
        resumed: m
            .get("resumed")
            .and_then(|x| x.as_u64())
            .unwrap_or_default(),
        canceled: m
            .get("canceled")
            .and_then(|x| x.as_u64())
            .unwrap_or_default(),
        completed: m
            .get("completed")
            .and_then(|x| x.as_u64())
            .unwrap_or_default(),
        completed_cached: m
            .get("completed_cached")
            .and_then(|x| x.as_u64())
            .unwrap_or_default(),
        errors: m.get("errors").and_then(|x| x.as_u64()).unwrap_or_default(),
        bytes_total: m
            .get("bytes_total")
            .and_then(|x| x.as_u64())
            .unwrap_or_default(),
        ewma_mbps: m.get("ewma_mbps").and_then(|x| x.as_f64()),
    };
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
/// List all model entries currently known to the service.
///
/// Returns the raw array persisted at `<state>/models/models.json` augmented by
/// runtime fields (e.g., `status`, `error_code`) while downloads are in-flight.
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
/// Reset to defaults (provider-curated), persist, and publish events/patches.
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
/// Persist the current models array to `<state>/models/models.json`.
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
/// Load the models array from `<state>/models/models.json`. 404 if missing.
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
/// Add a model id (and optional provider); does not download.
/// Publishes `models.changed {op:add}` and read-model patches.
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
/// Delete a model id; publishes `models.changed {op:delete}` and patches.
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
/// Return the default model id.
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
/// Set the default model id; publishes `models.changed {op:default}` and patches.
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
/// Start a download with mandatory `sha256`.
/// Publishes `models.download.progress` and updates the models list.
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
/// Request cancellation by model `id`. Publishes progress and changed events.
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
/// Trigger a single CAS GC pass for unreferenced blobs older than `ttl_days`.
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
/// List installed model hashes with pagination and optional provider filter.
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
    let mut items: Vec<HashItem> = Vec::with_capacity(by_hash.len());
    for (sha256, (bytes, path, providers)) in by_hash.into_iter() {
        items.push(HashItem {
            sha256,
            bytes,
            path,
            providers: providers.into_iter().collect::<Vec<_>>(),
        });
    }
    // Optional provider filter
    if let Some(p) = q.provider.as_deref() {
        let prov = p.to_string();
        items.retain(|it| it.providers.iter().any(|s| s == prov.as_str()));
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
            "sha256" => a.sha256.as_str().cmp(b.sha256.as_str()),
            "path" => a.path.as_str().cmp(b.path.as_str()),
            "providers_count" => a.providers.len().cmp(&b.providers.len()),
            _ => a.bytes.cmp(&b.bytes),
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
    Json(HashesPage {
        total,
        count: page.len(),
        limit,
        offset,
        items: page,
    })
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
        (status=304, description="Not Modified (If-None-Match/If-Modified-Since)"),
        (status=400, description="Invalid sha256", body = arw_protocol::ProblemDetails),
        (status=404, description="Not found"),
        (status=416, description="Range Not Satisfiable")
    )
)]
pub(crate) async fn models_blob_get(
    headers_in: HeaderMap,
    Path(sha256): Path<String>,
) -> impl IntoResponse {
    // Validate hash
    if !(sha256.len() == 64 && sha256.chars().all(|c| c.is_ascii_hexdigit())) {
        return (axum::http::StatusCode::BAD_REQUEST, "invalid sha256").into_response();
    }
    // Locate blob path
    let Some(path) = find_cas_blob_path(&sha256).await else {
        return axum::http::StatusCode::NOT_FOUND.into_response();
    };
    match tokio::fs::File::open(&path).await {
        Ok(mut file) => {
            use tokio::io::{AsyncReadExt, AsyncSeekExt};
            let meta = match tokio::fs::metadata(&path).await {
                Ok(m) => m,
                Err(_) => return StatusCode::NOT_FOUND.into_response(),
            };
            let total_len = meta.len();
            let mut headers = build_blob_headers(&meta, &sha256);
            // Conditional requests (ETag/Last-Modified)
            if is_not_modified(&headers_in, &headers, &meta, &sha256) {
                return (StatusCode::NOT_MODIFIED, headers).into_response();
            }

            // Range support: bytes=start-end | bytes=start- | bytes=-suffix
            if let Some(r) = headers_in
                .get(axum::http::header::RANGE)
                .and_then(|v| v.to_str().ok())
            {
                if let Some((s, e)) = parse_range_spec(r, total_len) {
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
                } else if r.trim().starts_with("bytes=") {
                    // Invalid/unsatisfiable range
                    headers.insert(
                        axum::http::header::CONTENT_RANGE,
                        HeaderValue::from_str(&format!("bytes */{}", total_len))
                            .unwrap_or(HeaderValue::from_static("")),
                    );
                    return (StatusCode::RANGE_NOT_SATISFIABLE, headers).into_response();
                }
            }
            // Full body
            headers.insert(
                axum::http::header::CONTENT_LENGTH,
                HeaderValue::from_str(&total_len.to_string())
                    .unwrap_or(HeaderValue::from_static("0")),
            );
            let stream = tokio_util::io::ReaderStream::new(file);
            let body = axum::body::Body::from_stream(stream);
            (headers, body).into_response()
        }
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

// HEAD metadata for a CAS blob by hash (same path as GET)
#[arw_admin(
    method = "HEAD",
    path = "/admin/models/by-hash/:sha256",
    summary = "HEAD model blob by sha256 (egress gated)"
)]
#[arw_gate("io:egress:models.peer")]
pub(crate) async fn models_blob_head(
    headers_in: HeaderMap,
    Path(sha256): Path<String>,
) -> impl IntoResponse {
    // Validate hash
    if !(sha256.len() == 64 && sha256.chars().all(|c| c.is_ascii_hexdigit())) {
        return (axum::http::StatusCode::BAD_REQUEST, "invalid sha256").into_response();
    }
    let Some(path) = find_cas_blob_path(&sha256).await else {
        return axum::http::StatusCode::NOT_FOUND.into_response();
    };
    let Ok(meta) = tokio::fs::metadata(&path).await else {
        return axum::http::StatusCode::NOT_FOUND.into_response();
    };
    let mut headers = build_blob_headers(&meta, &sha256);
    if is_not_modified(&headers_in, &headers, &meta, &sha256) {
        return (StatusCode::NOT_MODIFIED, headers).into_response();
    }
    let _ = headers.insert(
        axum::http::header::CONTENT_LENGTH,
        HeaderValue::from_str(&meta.len().to_string()).unwrap_or(HeaderValue::from_static("0")),
    );
    (StatusCode::OK, headers).into_response()
}

/// Internal: locate a CAS blob file for a given hex sha256, allowing optional extensions.
async fn find_cas_blob_path(sha256: &str) -> Option<std::path::PathBuf> {
    let dir = crate::ext::paths::state_dir()
        .join("models")
        .join("by-hash");
    let p_exact = dir.join(sha256);
    if std::fs::metadata(&p_exact).is_ok() {
        return Some(p_exact);
    }
    if let Ok(mut rd) = tokio::fs::read_dir(&dir).await {
        while let Ok(Some(ent)) = rd.next_entry().await {
            let name = ent.file_name().to_string_lossy().to_string();
            if name == sha256 || name.starts_with(&format!("{}.", sha256)) {
                return Some(ent.path());
            }
        }
    }
    None
}

// helpers moved to super::http

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
    use super::{models_blob_get, models_blob_head, models_metrics_get, models_quota_get};
    use crate::AppState;
    use axum::{
        http::Request,
        routing::{get, head},
        Router,
    };
    use http_body_util::BodyExt; // for collecting body
    use tower::ServiceExt; // for `oneshot`

    #[tokio::test]
    async fn http_models_metrics_shape() {
        // Build minimal app with the handler and a state containing ModelsService
        let state = {
            let st = AppState::default();
            st.resources.insert(std::sync::Arc::new(
                crate::resources::models_service::ModelsService::new(),
            ));
            st
        };
        let app = Router::new()
            .route("/admin/state/models_metrics", get(models_metrics_get))
            .with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/admin/state/models_metrics")
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

    #[serial_test::serial]
    #[tokio::test]
    async fn http_models_blob_head_and_304() {
        // Use a temp state dir for CAS
        let (_tmp, _guard) = crate::test_support::scoped_state_dir();
        let base = crate::ext::paths::state_dir();
        let cas_dir = base.join("models").join("by-hash");
        std::fs::create_dir_all(&cas_dir).unwrap();
        let sha = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let fpath = cas_dir.join(sha);
        std::fs::write(&fpath, b"hello").unwrap();
        assert!(std::fs::metadata(&fpath).is_ok());

        let app = Router::new()
            .route("/admin/models/by-hash/:sha256", head(models_blob_head))
            .with_state(AppState::default());

        // Initial HEAD: expect 200 with ETag, Accept-Ranges, Content-Length
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("HEAD")
                    .uri(format!("/admin/models/by-hash/{}", sha))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
        let h = resp.headers();
        assert_eq!(
            h.get("ETag").unwrap().to_str().unwrap(),
            format!("\"{}\"", sha)
        );
        assert_eq!(h.get("Accept-Ranges").unwrap().to_str().unwrap(), "bytes");
        assert_eq!(h.get("Content-Length").unwrap().to_str().unwrap(), "5");

        // With If-None-Match -> 304
        let resp2 = app
            .oneshot(
                Request::builder()
                    .method("HEAD")
                    .uri(format!("/admin/models/by-hash/{}", sha))
                    .header("If-None-Match", format!("\"{}\"", sha))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp2.status(), axum::http::StatusCode::NOT_MODIFIED);
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn http_models_blob_range_416() {
        // Use a temp state dir for CAS
        let (_tmp, _guard) = crate::test_support::scoped_state_dir();
        let base = crate::ext::paths::state_dir();
        let cas_dir = base.join("models").join("by-hash");
        std::fs::create_dir_all(&cas_dir).unwrap();
        let sha = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let fpath = cas_dir.join(sha);
        std::fs::write(&fpath, b"hello").unwrap();
        assert!(std::fs::metadata(&fpath).is_ok());

        let app = Router::new()
            .route("/admin/models/by-hash/:sha256", get(models_blob_get))
            .with_state(AppState::default());

        // Request an invalid range
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/admin/models/by-hash/{}", sha))
                    .header("Range", "bytes=10-20")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::RANGE_NOT_SATISFIABLE);
        let h = resp.headers();
        assert_eq!(
            h.get("Content-Range").unwrap().to_str().unwrap(),
            "bytes */5"
        );
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn http_models_blob_range_206() {
        // Use a temp state dir for CAS
        let (_tmp, _guard) = crate::test_support::scoped_state_dir();
        let base = crate::ext::paths::state_dir();
        let cas_dir = base.join("models").join("by-hash");
        std::fs::create_dir_all(&cas_dir).unwrap();
        let sha = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let fpath = cas_dir.join(sha);
        std::fs::write(&fpath, b"hello").unwrap();

        let app = Router::new()
            .route("/admin/models/by-hash/:sha256", get(models_blob_get))
            .with_state(AppState::default());

        // Request a valid range
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/admin/models/by-hash/{}", sha))
                    .header("Range", "bytes=0-1")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::PARTIAL_CONTENT);
        let h = resp.headers();
        assert_eq!(
            h.get("Content-Range").unwrap().to_str().unwrap(),
            "bytes 0-1/5"
        );
        assert_eq!(h.get("Content-Length").unwrap().to_str().unwrap(), "2");
        // Verify body content matches the requested slice
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"he");
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn http_models_blob_if_modified_since_304() {
        let (_tmp, _guard) = crate::test_support::scoped_state_dir();
        let base = crate::ext::paths::state_dir();
        let cas_dir = base.join("models").join("by-hash");
        std::fs::create_dir_all(&cas_dir).unwrap();
        let sha = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
        let fpath = cas_dir.join(sha);
        std::fs::write(&fpath, b"hello").unwrap();

        let app = Router::new()
            .route("/admin/models/by-hash/:sha256", get(models_blob_get))
            .with_state(AppState::default());

        // First GET to obtain Last-Modified
        let resp1 = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/admin/models/by-hash/{}", sha))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(resp1.status().is_success());
        let lm = resp1
            .headers()
            .get(axum::http::header::LAST_MODIFIED)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();

        // Second GET with If-Modified-Since should return 304
        let resp2 = app
            .oneshot(
                Request::builder()
                    .uri(format!("/admin/models/by-hash/{}", sha))
                    .header(axum::http::header::IF_MODIFIED_SINCE, lm)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp2.status(), axum::http::StatusCode::NOT_MODIFIED);
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
/// Set the maximum concurrent downloads.
/// With `block=true`, waits to shrink; with `false`, shrinks opportunistically and reports pending.
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
/// Return configured max, available permits, held permits, and optional hard cap.
pub(crate) async fn models_concurrency_get(State(state): State<AppState>) -> impl IntoResponse {
    let Some(svc) = state.resources.get::<ModelsService>() else {
        return super::ApiError::internal("ModelsService missing").into_response();
    };
    let v = svc.concurrency_get().await;
    let out = ModelsConcurrency {
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
        pending_shrink: v.get("pending_shrink").and_then(|x| x.as_u64()),
    };
    Json(out).into_response()
}

/// Admin: snapshot of active download jobs and inflight hashes
#[arw_admin(
    method = "GET",
    path = "/admin/models/jobs",
    summary = "List active jobs and inflight hashes"
)]
#[arw_gate("models:jobs")]
/// Return active `model_id`/`job_id` pairs, inflight SHA‑256 hashes, and a concurrency snapshot.
pub(crate) async fn models_jobs(State(state): State<AppState>) -> impl IntoResponse {
    let Some(svc) = state.resources.get::<ModelsService>() else {
        return super::ApiError::internal("ModelsService missing").into_response();
    };
    let v = svc.jobs_status().await;
    // Map JSON to typed output (stable field names)
    let active = v
        .get("active")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .map(|it| ActiveJob {
                    model_id: it
                        .get("model_id")
                        .and_then(|x| x.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    job_id: it
                        .get("job_id")
                        .and_then(|x| x.as_str())
                        .unwrap_or_default()
                        .to_string(),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let inflight_hashes = v
        .get("inflight_hashes")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|s| s.as_str().map(|t| t.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let conc_v = v.get("concurrency").cloned().unwrap_or_default();
    let concurrency = ModelsConcurrency {
        configured_max: conc_v
            .get("configured_max")
            .and_then(|x| x.as_u64())
            .unwrap_or_default(),
        available_permits: conc_v
            .get("available_permits")
            .and_then(|x| x.as_u64())
            .unwrap_or_default(),
        held_permits: conc_v
            .get("held_permits")
            .and_then(|x| x.as_u64())
            .unwrap_or_default(),
        hard_cap: conc_v.get("hard_cap").and_then(|x| x.as_u64()),
        pending_shrink: conc_v.get("pending_shrink").and_then(|x| x.as_u64()),
    };
    Json(ModelsJobs {
        active,
        inflight_hashes,
        concurrency,
    })
    .into_response()
}

/// Read-model: models download metrics (counters + EWMA MB/s)
#[arw_admin(
    method = "GET",
    path = "/admin/state/models_metrics",
    summary = "Get models download metrics"
)]
#[arw_gate("state:models_metrics:get")]
/// Return the same shape as the `models_metrics` read-model snapshot.
pub(crate) async fn models_metrics_get(State(state): State<AppState>) -> impl IntoResponse {
    // Use the service helper to return a consistent shape (counters + ewma)
    match super::require_service::<ModelsService>(&state) {
        Ok(svc) => {
            let base = svc.downloads_metrics().await;
            let out = ModelsMetrics {
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
            };
            Json(out).into_response()
        }
        Err(e) => e.into_response(),
    }
}
// SPDX-License-Identifier: MIT OR Apache-2.0

use axum::body::Body;
use axum::http::{
    header::{
        ACCEPT_RANGES, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, ETAG,
        LAST_MODIFIED, RANGE, X_CONTENT_TYPE_OPTIONS,
    },
    HeaderMap, HeaderValue, Method, StatusCode,
};
use axum::response::{IntoResponse, Response};
use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::SeekFrom;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;

use crate::{ext, models, AppState};
use models::{HashPage, ModelsConcurrencySnapshot, ModelsMetricsResponse};
use utoipa::ToSchema;

fn unauthorized() -> axum::response::Response {
    (
        axum::http::StatusCode::UNAUTHORIZED,
        Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
    )
        .into_response()
}

#[utoipa::path(
    get,
    path = "/admin/models/summary",
    tag = "Models",
    responses(
        (status = 200, description = "Models summary", body = serde_json::Value),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn models_summary(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return unauthorized();
    }
    let summary = state.models().summary().await;
    Json(summary).into_response()
}

#[utoipa::path(
    get,
    path = "/admin/models",
    tag = "Models",
    responses(
        (status = 200, description = "Models list", body = [serde_json::Value]),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn models_list(headers: HeaderMap, State(state): State<AppState>) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return unauthorized();
    }
    let items = state.models().list().await;
    Json(items).into_response()
}

#[utoipa::path(
    post,
    path = "/admin/models/refresh",
    tag = "Models",
    responses((status = 200, description = "Refreshed list", body = [serde_json::Value]))
)]
pub async fn models_refresh(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return unauthorized();
    }
    let items = state.models().refresh().await;
    Json(items).into_response()
}

#[utoipa::path(
    post,
    path = "/admin/models/save",
    tag = "Models",
    responses((status = 200, description = "Saved", body = serde_json::Value))
)]
pub async fn models_save(headers: HeaderMap, State(state): State<AppState>) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return unauthorized();
    }
    match state.models().save().await {
        Ok(_) => Json(json!({"ok": true})).into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"type":"about:blank","title":"Save failed","status":500,"detail":e})),
        )
            .into_response(),
    }
}

#[utoipa::path(
    post,
    path = "/admin/models/load",
    tag = "Models",
    responses(
        (status = 200, description = "Loaded list", body = [serde_json::Value]),
        (status = 404, description = "Missing models.json")
    )
)]
pub async fn models_load(headers: HeaderMap, State(state): State<AppState>) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return unauthorized();
    }
    match state.models().load().await {
        Ok(items) => Json(items).into_response(),
        Err(_) => (axum::http::StatusCode::NOT_FOUND, "no models.json").into_response(),
    }
}

#[derive(Deserialize, ToSchema)]
pub struct ModelEntry {
    pub id: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

#[utoipa::path(
    post,
    path = "/admin/models/add",
    tag = "Models",
    request_body = ModelEntry,
    responses(
        (status = 200, description = "Added", body = serde_json::Value),
        (status = 400, description = "Invalid input")
    )
)]
pub async fn models_add(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<ModelEntry>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return unauthorized();
    }
    let mut obj = serde_json::Map::new();
    obj.insert("id".into(), Value::String(req.id));
    if let Some(provider) = req.provider {
        obj.insert("provider".into(), Value::String(provider));
    }
    if let Some(path) = req.path {
        obj.insert("path".into(), Value::String(path));
    }
    if let Some(sha) = req.sha256 {
        obj.insert("sha256".into(), Value::String(sha));
    }
    if let Some(status) = req.status {
        obj.insert("status".into(), Value::String(status));
    }
    match state.models().add_model(Value::Object(obj)).await {
        Ok(()) => Json(json!({"ok": true})).into_response(),
        Err(e) => (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({"type":"about:blank","title":"Invalid model","status":400,"detail":e})),
        )
            .into_response(),
    }
}

#[derive(Deserialize, ToSchema)]
pub struct ModelId {
    pub id: String,
}

#[utoipa::path(
    post,
    path = "/admin/models/remove",
    tag = "Models",
    request_body = ModelId,
    responses((status = 200, description = "Removed", body = serde_json::Value))
)]
pub async fn models_remove(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<ModelId>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return unauthorized();
    }
    let removed = state.models().remove_model(&req.id).await;
    Json(json!({"removed": removed})).into_response()
}

#[utoipa::path(
    get,
    path = "/admin/models/default",
    tag = "Models",
    responses((status = 200, description = "Default model", body = serde_json::Value))
)]
pub async fn models_default_get(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return unauthorized();
    }
    let id = state.models().default_get().await;
    Json(json!({"default": id})).into_response()
}

#[utoipa::path(
    post,
    path = "/admin/models/default",
    tag = "Models",
    request_body = ModelId,
    responses(
        (status = 200, description = "Set", body = serde_json::Value),
        (status = 400, description = "Unknown model")
    )
)]
pub async fn models_default_set(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<ModelId>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return unauthorized();
    }
    match state.models().default_set(req.id).await {
        Ok(()) => Json(json!({"ok": true})).into_response(),
        Err(e) => (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({"type":"about:blank","title":"Invalid model","status":400,"detail":e})),
        )
            .into_response(),
    }
}

#[derive(Deserialize, ToSchema)]
pub struct ConcurrencyUpdate {
    #[serde(default)]
    pub max: Option<u64>,
    #[serde(default)]
    pub hard_cap: Option<u64>,
    #[serde(default)]
    pub block: Option<bool>,
}

#[utoipa::path(
    get,
    path = "/admin/models/concurrency",
    tag = "Models",
    responses((status = 200, description = "Concurrency", body = ModelsConcurrencySnapshot))
)]
pub async fn models_concurrency_get(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return unauthorized();
    }
    Json(state.models().concurrency_get().await).into_response()
}

#[utoipa::path(
    post,
    path = "/admin/models/concurrency",
    tag = "Models",
    request_body = ConcurrencyUpdate,
    responses((status = 200, description = "Updated", body = ModelsConcurrencySnapshot))
)]
pub async fn models_concurrency_set(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<ConcurrencyUpdate>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return unauthorized();
    }
    let value = state
        .models()
        .concurrency_set(req.max, req.hard_cap, req.block)
        .await;
    Json(value).into_response()
}

#[utoipa::path(
    get,
    path = "/admin/models/jobs",
    tag = "Models",
    responses((status = 200, description = "Jobs", body = serde_json::Value))
)]
pub async fn models_jobs(headers: HeaderMap, State(state): State<AppState>) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return unauthorized();
    }
    Json(state.models().jobs_snapshot().await).into_response()
}

#[utoipa::path(
    get,
    path = "/state/models_metrics",
    tag = "State",
    operation_id = "state_models_metrics_doc",
    description = "Models metrics snapshot.",
    responses(
        (status = 200, description = "Metrics", body = ModelsMetricsResponse),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn state_models_metrics(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return unauthorized();
    }
    Json(state.models().metrics_value().await).into_response()
}

#[derive(Deserialize, ToSchema)]
pub struct HashesQuery {
    #[serde(default = "HashesQuery::default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub sort: Option<String>,
    #[serde(default)]
    pub order: Option<String>,
}

impl HashesQuery {
    fn default_limit() -> usize {
        100
    }
}

#[utoipa::path(
    get,
    path = "/state/models_hashes",
    tag = "State",
    params(
        ("limit" = Option<usize>, Query, description = "Page size (default 100)"),
        ("offset" = Option<usize>, Query, description = "Start offset"),
        ("provider" = Option<String>, Query, description = "Filter by provider"),
        ("sort" = Option<String>, Query, description = "Sort key (bytes|sha256|path|providers_count)"),
        ("order" = Option<String>, Query, description = "Order asc|desc")
    ),
    responses((status = 200, description = "Installed hashes", body = HashPage))
)]
pub async fn state_models_hashes(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(q): Query<HashesQuery>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return unauthorized();
    }
    let page = state
        .models()
        .hashes_page(
            q.limit,
            q.offset,
            q.provider.clone(),
            q.sort.clone(),
            q.order.clone(),
        )
        .await;
    Json(page).into_response()
}

#[utoipa::path(
    post,
    path = "/admin/models/download",
    tag = "Models",
    request_body = models::DownloadRequest,
    responses(
        (status = 200, description = "Download accepted", body = serde_json::Value),
        (status = 501, description = "Download unavailable"))
)]
pub async fn models_download(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<models::DownloadRequest>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return unauthorized();
    }
    match state.models().start_download(req).await {
        Ok(_) => Json(json!({"ok": true})).into_response(),
        Err(e) => (
            axum::http::StatusCode::NOT_IMPLEMENTED,
            Json(json!({"type":"about:blank","title":"Download unavailable","status":501,"detail":e})),
        )
            .into_response(),
    }
}

#[utoipa::path(
    post,
    path = "/admin/models/download/cancel",
    tag = "Models",
    request_body = ModelId,
    responses(
        (status = 200, description = "Cancellation requested", body = serde_json::Value),
        (status = 501, description = "Cancel unavailable"))
)]
pub async fn models_download_cancel(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<ModelId>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return unauthorized();
    }
    match state.models().cancel_download(&req.id).await {
        Ok(_) => Json(json!({"ok": true})).into_response(),
        Err(e) => (
            axum::http::StatusCode::NOT_IMPLEMENTED,
            Json(
                json!({"type":"about:blank","title":"Cancel unavailable","status":501,"detail":e}),
            ),
        )
            .into_response(),
    }
}

#[utoipa::path(
    post,
    path = "/admin/models/cas_gc",
    tag = "Models",
    request_body = models::CasGcRequest,
    responses(
        (status = 200, description = "GC summary", body = serde_json::Value),
        (status = 501, description = "CAS GC unavailable"))
)]
pub async fn models_cas_gc(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<models::CasGcRequest>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return unauthorized();
    }
    match state.models().cas_gc(req).await {
        Ok(value) => Json(value).into_response(),
        Err(e) => (
            axum::http::StatusCode::NOT_IMPLEMENTED,
            Json(
                json!({"type":"about:blank","title":"CAS GC unavailable","status":501,"detail":e}),
            ),
        )
            .into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/admin/models/by-hash/{sha256}",
    tag = "Models",
    params(("sha256" = String, Path, description = "Model blob SHA-256 (hex)")),
    responses(
        (status = 200, description = "Model blob", content_type = "application/octet-stream"),
        (status = 304, description = "Not modified"),
        (status = 400, description = "Invalid hash", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Blob not found", body = serde_json::Value),
        (status = 500, description = "Read error", body = serde_json::Value)
    )
)]
pub async fn models_blob_by_hash(
    method: Method,
    Path(sha256): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if !crate::admin_ok(&headers) {
        return unauthorized();
    }

    let hash = sha256.trim();
    if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "type": "about:blank",
                "title": "Invalid hash",
                "status": 400
            })),
        )
            .into_response();
    }
    let hash = hash.to_ascii_lowercase();
    let cas_path = state.models().cas_blob_path(&hash);

    let metadata = match fs::metadata(&cas_path).await {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "type": "about:blank",
                    "title": "Blob not found",
                    "status": 404
                })),
            )
                .into_response();
        }
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "type": "about:blank",
                    "title": "Read error",
                    "status": 500,
                    "detail": err.to_string()
                })),
            )
                .into_response();
        }
    };

    let len = metadata.len();
    let modified = metadata.modified().ok();
    let cache_control = "public, max-age=31536000, immutable";
    let etag = ext::http::etag_value(&hash);
    let last_modified_header = modified.and_then(ext::http::http_date_value);

    if ext::http::if_none_match_matches(&headers, &hash) {
        let mut response =
            ext::http::not_modified_response(&etag, last_modified_header.as_ref(), cache_control);
        response
            .headers_mut()
            .insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));
        return response;
    }
    if let Some(modified_time) = modified {
        if ext::http::not_modified_since(&headers, modified_time) {
            let mut response = ext::http::not_modified_response(
                &etag,
                last_modified_header.as_ref(),
                cache_control,
            );
            response
                .headers_mut()
                .insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));
            return response;
        }
    }

    let range = match headers
        .get(RANGE)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(value) => match ext::http::parse_single_byte_range(value, len) {
            Ok(range) => Some(range),
            Err(_) => {
                let content_range = format!("bytes */{len}");
                let mut builder = Response::builder()
                    .status(StatusCode::RANGE_NOT_SATISFIABLE)
                    .header(ETAG, etag.clone())
                    .header(CACHE_CONTROL, cache_control)
                    .header(X_CONTENT_TYPE_OPTIONS, "nosniff")
                    .header(ACCEPT_RANGES, "bytes")
                    .header(CONTENT_TYPE, "application/octet-stream")
                    .header(
                        CONTENT_RANGE,
                        HeaderValue::from_str(&content_range).expect("content-range header value"),
                    );
                if let Some(ref last_modified) = last_modified_header {
                    builder = builder.header(LAST_MODIFIED, last_modified.clone());
                }
                return builder
                    .body(Body::empty())
                    .unwrap_or_else(|_| Response::new(Body::empty()));
            }
        },
        None => None,
    };

    let mut builder = Response::builder()
        .header(ETAG, etag.clone())
        .header(CACHE_CONTROL, cache_control)
        .header(X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(ACCEPT_RANGES, "bytes")
        .header(CONTENT_TYPE, "application/octet-stream");
    if let Some(ref last_modified) = last_modified_header {
        builder = builder.header(LAST_MODIFIED, last_modified.clone());
    }

    if let Some(range) = range {
        let content_length = range.len();
        builder = builder
            .status(StatusCode::PARTIAL_CONTENT)
            .header(CONTENT_LENGTH, content_length.to_string())
            .header(
                CONTENT_RANGE,
                HeaderValue::from_str(&format!("bytes {}-{}/{}", range.start, range.end, len))
                    .expect("content-range header value"),
            );

        if method == Method::HEAD {
            return builder
                .body(Body::empty())
                .unwrap_or_else(|_| Response::new(Body::empty()));
        }

        let mut file = match fs::File::open(&cas_path).await {
            Ok(file) => file,
            Err(err) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "type": "about:blank",
                        "title": "Read error",
                        "status": 500,
                        "detail": err.to_string()
                    })),
                )
                    .into_response();
            }
        };

        if let Err(err) = file.seek(SeekFrom::Start(range.start)).await {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "type": "about:blank",
                    "title": "Read error",
                    "status": 500,
                    "detail": err.to_string()
                })),
            )
                .into_response();
        }

        let limited = file.take(content_length);
        let stream = ReaderStream::new(limited);
        builder
            .body(Body::from_stream(stream))
            .unwrap_or_else(|_| Response::new(Body::empty()))
    } else {
        builder = builder
            .status(StatusCode::OK)
            .header(CONTENT_LENGTH, len.to_string());

        if method == Method::HEAD {
            return builder
                .body(Body::empty())
                .unwrap_or_else(|_| Response::new(Body::empty()));
        }

        let file = match fs::File::open(&cas_path).await {
            Ok(file) => file,
            Err(err) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "type": "about:blank",
                        "title": "Read error",
                        "status": 500,
                        "detail": err.to_string()
                    })),
                )
                    .into_response();
            }
        };
        let stream = ReaderStream::new(file);
        builder
            .body(Body::from_stream(stream))
            .unwrap_or_else(|_| Response::new(Body::empty()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::env;
    use axum::http::{
        header::{ACCEPT_RANGES, CONTENT_RANGE, IF_NONE_MATCH, RANGE},
        HeaderMap, HeaderValue,
    };
    use http_body_util::BodyExt;
    use std::{path::Path, sync::Arc};
    use tempfile::tempdir;
    use tokio::sync::Mutex;

    async fn build_state(path: &Path, env_guard: &mut env::EnvGuard) -> AppState {
        env_guard.set("ARW_DEBUG", "1");
        crate::util::reset_state_dir_for_tests();
        env_guard.set("ARW_STATE_DIR", path.display().to_string());
        let bus = arw_events::Bus::new_with_replay(32, 32);
        let kernel = arw_kernel::Kernel::open(path).expect("init kernel for tests");
        let policy = arw_policy::PolicyEngine::load_from_env();
        let policy_arc = Arc::new(Mutex::new(policy));
        let host: Arc<dyn arw_wasi::ToolHost> = Arc::new(arw_wasi::NoopHost);
        AppState::builder(bus, kernel, policy_arc, host, true)
            .with_sse_capacity(32)
            .build()
            .await
    }

    async fn write_blob(state: &AppState, hash: &str, body: &[u8]) {
        let path = state.models().cas_blob_path(hash);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await.expect("create cas dir");
        }
        fs::write(&path, body).await.expect("write cas blob");
    }

    fn make_hash() -> String {
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string()
    }

    #[tokio::test]
    async fn models_blob_by_hash_serves_blob_with_headers() {
        let mut env_guard = env::guard();
        let temp = tempdir().expect("tempdir");
        let _state_guard = crate::util::scoped_state_dir_for_tests(temp.path());
        let state = build_state(temp.path(), &mut env_guard).await;

        let hash = make_hash();
        let payload = b"artifact-bytes";
        write_blob(&state, &hash, payload).await;

        let response = models_blob_by_hash(
            Method::GET,
            Path(hash.clone()),
            State(state.clone()),
            HeaderMap::new(),
        )
        .await;

        let (parts, body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::OK);
        assert_eq!(
            parts.headers.get(ETAG).and_then(|v| v.to_str().ok()),
            Some(format!("\"{}\"", hash).as_str())
        );
        assert_eq!(
            parts
                .headers
                .get(ACCEPT_RANGES)
                .and_then(|v| v.to_str().ok()),
            Some("bytes")
        );
        assert_eq!(
            parts
                .headers
                .get(CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some("public, max-age=31536000, immutable")
        );
        assert_eq!(
            parts
                .headers
                .get(CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("application/octet-stream")
        );
        assert_eq!(
            parts
                .headers
                .get(X_CONTENT_TYPE_OPTIONS)
                .and_then(|v| v.to_str().ok()),
            Some("nosniff")
        );
        assert_eq!(
            parts
                .headers
                .get(CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok()),
            Some(payload.len() as u64)
        );
        assert!(parts.headers.get(LAST_MODIFIED).is_some());

        let collected = BodyExt::collect(body)
            .await
            .expect("collect body")
            .to_bytes();
        assert_eq!(&collected[..], payload);
    }

    #[tokio::test]
    async fn models_blob_by_hash_head_omits_body() {
        let mut env_guard = env::guard();
        let temp = tempdir().expect("tempdir");
        let _state_guard = crate::util::scoped_state_dir_for_tests(temp.path());
        let state = build_state(temp.path(), &mut env_guard).await;

        let hash = make_hash();
        let payload = b"head-check";
        write_blob(&state, &hash, payload).await;

        let response = models_blob_by_hash(
            Method::HEAD,
            Path(hash.clone()),
            State(state.clone()),
            HeaderMap::new(),
        )
        .await;

        let (parts, body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::OK);
        let bytes = BodyExt::collect(body)
            .await
            .expect("collect head body")
            .to_bytes();
        assert!(bytes.is_empty(), "HEAD responses should omit body");
        assert_eq!(
            parts
                .headers
                .get(ACCEPT_RANGES)
                .and_then(|v| v.to_str().ok()),
            Some("bytes")
        );
        assert_eq!(
            parts
                .headers
                .get(CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok()),
            Some(payload.len() as u64)
        );
    }

    #[tokio::test]
    async fn models_blob_by_hash_honors_if_none_match() {
        let mut env_guard = env::guard();
        let temp = tempdir().expect("tempdir");
        let _state_guard = crate::util::scoped_state_dir_for_tests(temp.path());
        let state = build_state(temp.path(), &mut env_guard).await;

        let hash = make_hash();
        write_blob(&state, &hash, b"etag").await;

        let mut headers = HeaderMap::new();
        headers.insert(
            IF_NONE_MATCH,
            HeaderValue::from_str(&format!("\"{}\"", hash)).expect("header value"),
        );

        let response = models_blob_by_hash(
            Method::GET,
            Path(hash.clone()),
            State(state.clone()),
            headers,
        )
        .await;

        let (parts, body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::NOT_MODIFIED);
        let bytes = BodyExt::collect(body)
            .await
            .expect("collect body")
            .to_bytes();
        assert!(bytes.is_empty());
        assert_eq!(
            parts.headers.get(ETAG).and_then(|v| v.to_str().ok()),
            Some(format!("\"{}\"", hash).as_str())
        );
        assert_eq!(
            parts
                .headers
                .get(ACCEPT_RANGES)
                .and_then(|v| v.to_str().ok()),
            Some("bytes")
        );
    }

    #[tokio::test]
    async fn models_blob_by_hash_supports_range_requests() {
        let mut env_guard = env::guard();
        let temp = tempdir().expect("tempdir");
        let _state_guard = crate::util::scoped_state_dir_for_tests(temp.path());
        let state = build_state(temp.path(), &mut env_guard).await;

        let hash = make_hash();
        write_blob(&state, &hash, b"0123456789").await;

        let mut headers = HeaderMap::new();
        headers.insert(RANGE, HeaderValue::from_static("bytes=2-5"));

        let response = models_blob_by_hash(
            Method::GET,
            Path(hash.clone()),
            State(state.clone()),
            headers,
        )
        .await;

        let (parts, body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            parts
                .headers
                .get(CONTENT_RANGE)
                .and_then(|v| v.to_str().ok()),
            Some("bytes 2-5/10")
        );
        assert_eq!(
            parts
                .headers
                .get(CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok()),
            Some(4)
        );

        let collected = BodyExt::collect(body)
            .await
            .expect("collect body")
            .to_bytes();
        assert_eq!(&collected[..], b"2345");
    }

    #[tokio::test]
    async fn models_blob_by_hash_rejects_invalid_range() {
        let mut env_guard = env::guard();
        let temp = tempdir().expect("tempdir");
        let _state_guard = crate::util::scoped_state_dir_for_tests(temp.path());
        let state = build_state(temp.path(), &mut env_guard).await;

        let hash = make_hash();
        write_blob(&state, &hash, b"0123456789").await;

        let mut headers = HeaderMap::new();
        headers.insert(RANGE, HeaderValue::from_static("bytes=20-10"));

        let response = models_blob_by_hash(
            Method::GET,
            Path(hash.clone()),
            State(state.clone()),
            headers,
        )
        .await;

        let (parts, body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(
            parts
                .headers
                .get(CONTENT_RANGE)
                .and_then(|v| v.to_str().ok()),
            Some("bytes */10")
        );
        assert_eq!(
            parts
                .headers
                .get(ACCEPT_RANGES)
                .and_then(|v| v.to_str().ok()),
            Some("bytes")
        );
        assert!(BodyExt::collect(body)
            .await
            .expect("collect body")
            .to_bytes()
            .is_empty());
    }

    #[tokio::test]
    async fn models_blob_by_hash_rejects_invalid_hash() {
        let mut env_guard = env::guard();
        let temp = tempdir().expect("tempdir");
        let _state_guard = crate::util::scoped_state_dir_for_tests(temp.path());
        let state = build_state(temp.path(), &mut env_guard).await;

        let response = models_blob_by_hash(
            Method::GET,
            Path("not-a-hash".to_string()),
            State(state.clone()),
            HeaderMap::new(),
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}

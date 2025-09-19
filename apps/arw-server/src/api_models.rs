use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{models, AppState};
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
}

#[utoipa::path(
    get,
    path = "/admin/models/concurrency",
    tag = "Models",
    responses((status = 200, description = "Concurrency", body = serde_json::Value))
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
    responses((status = 200, description = "Updated", body = serde_json::Value))
)]
pub async fn models_concurrency_set(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<ConcurrencyUpdate>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return unauthorized();
    }
    let value = state.models().concurrency_set(req.max, req.hard_cap).await;
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
    path = "/admin/state/models_metrics",
    tag = "Models",
    responses((status = 200, description = "Metrics", body = serde_json::Value))
)]
pub async fn models_metrics(
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
    path = "/admin/state/models_hashes",
    tag = "Models",
    params(
        ("limit" = Option<usize>, Query, description = "Page size (default 100)"),
        ("offset" = Option<usize>, Query, description = "Start offset"),
        ("provider" = Option<String>, Query, description = "Filter by provider"),
        ("sort" = Option<String>, Query, description = "Sort key (bytes|sha256|path|providers_count)"),
        ("order" = Option<String>, Query, description = "Order asc|desc")
    ),
    responses((status = 200, description = "Installed hashes", body = serde_json::Value))
)]
pub async fn models_hashes(
    headers: HeaderMap,
    State(state): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<HashesQuery>,
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

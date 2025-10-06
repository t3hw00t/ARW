use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::{extract::State, Json};
use serde_json::json;
use utoipa::ToSchema;

use crate::app_state::AppState;

#[derive(ToSchema, serde::Serialize)]
pub struct RuntimeBundlesReloadResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Reload runtime bundle catalogs from disk.
#[utoipa::path(
    post,
    path = "/admin/runtime/bundles/reload",
    tag = "Admin/Runtime",
    responses(
        (status = 200, description = "Reloaded", body = RuntimeBundlesReloadResponse),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 500, description = "Reload failed", body = RuntimeBundlesReloadResponse)
    )
)]
pub async fn runtime_bundles_reload(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> axum::response::Response {
    if !crate::admin_ok(&headers).await {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    match state.runtime_bundles().reload().await {
        Ok(_) => Json(RuntimeBundlesReloadResponse {
            ok: true,
            error: None,
        })
        .into_response(),
        Err(err) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(RuntimeBundlesReloadResponse {
                ok: false,
                error: Some(err.to_string()),
            }),
        )
            .into_response(),
    }
}

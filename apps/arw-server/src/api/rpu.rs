use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::{extract::State, Json};
use utoipa::ToSchema;

use crate::{admin_ok, AppState};

#[derive(serde::Serialize, ToSchema)]
pub(crate) struct TrustIssuerOut {
    id: String,
    alg: String,
}

/// RPU trust issuers (admin).
#[utoipa::path(get, path = "/admin/rpu/trust", tag = "RPU", responses((status = 200, body = serde_json::Value), (status = 401)))]
pub async fn rpu_trust_get(
    State(_state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"type":"about:blank","title":"Unauthorized","status":401})),
        );
    }
    let issuers: Vec<TrustIssuerOut> = arw_core::rpu::trust_snapshot()
        .into_iter()
        .map(|e| TrustIssuerOut {
            id: e.id,
            alg: e.alg,
        })
        .collect();
    (
        axum::http::StatusCode::OK,
        Json(serde_json::json!({
            "count": issuers.len(),
            "last_reload_ms": arw_core::rpu::trust_last_reload_ms(),
            "issuers": issuers
        })),
    )
}

/// Reload RPU trust issuers and emit event (admin).
#[utoipa::path(post, path = "/admin/rpu/reload", tag = "RPU", responses((status = 200, body = serde_json::Value), (status = 401)))]
pub async fn rpu_reload_post(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"type":"about:blank","title":"Unauthorized","status":401})),
        );
    }
    arw_core::rpu::reload_trust();
    let issuers: Vec<TrustIssuerOut> = arw_core::rpu::trust_snapshot()
        .into_iter()
        .map(|e| TrustIssuerOut {
            id: e.id,
            alg: e.alg,
        })
        .collect();
    let payload = serde_json::json!({
        "count": issuers.len(),
        "ts_ms": arw_core::rpu::trust_last_reload_ms(),
    });
    state
        .bus()
        .publish(arw_topics::TOPIC_RPU_TRUST_CHANGED, &payload);
    (
        axum::http::StatusCode::OK,
        Json(serde_json::json!({
            "count": issuers.len(),
            "last_reload_ms": arw_core::rpu::trust_last_reload_ms(),
            "issuers": issuers
        })),
    )
}

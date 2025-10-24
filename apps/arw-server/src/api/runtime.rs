use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::json;
use utoipa::ToSchema;

use crate::app_state::AppState;
use crate::runtime_bundle_resolver;
use arw_compress::{KvMethod, KvPolicy};
use tracing::{info, warn};

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
        Ok(_) => {
            if let Err(err) = runtime_bundle_resolver::reconcile(
                state.runtime_supervisor(),
                state.runtime_bundles(),
            )
            .await
            {
                warn!(
                    target = "arw::runtime",
                    error = %err,
                    "bundle reload succeeded but runtime registration failed"
                );
                return (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    Json(RuntimeBundlesReloadResponse {
                        ok: false,
                        error: Some(err.to_string()),
                    }),
                )
                    .into_response();
            }
            Json(RuntimeBundlesReloadResponse {
                ok: true,
                error: None,
            })
            .into_response()
        }
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

#[derive(Debug, Deserialize, ToSchema)]
pub struct KvPolicyRequest {
    #[schema(example = "snapkv")]
    pub method: String,
    #[serde(default)]
    #[schema(example = 0.25)]
    pub ratio: Option<f32>,
    #[serde(default)]
    #[schema(example = 2)]
    pub bits: Option<u32>,
}

fn parse_kv_method(value: &str) -> Option<KvMethod> {
    match value.trim().to_ascii_lowercase().as_str() {
        "none" | "off" | "disable" => Some(KvMethod::None),
        "snapkv" | "snap" => Some(KvMethod::SnapKv),
        "kivi" | "kivi2bit" | "kivi-2bit" => Some(KvMethod::Kivi2Bit),
        "cachegen" | "cache-gen" | "cache" => Some(KvMethod::CacheGen),
        _ => None,
    }
}

#[utoipa::path(
    put,
    path = "/v1/runtime/kv_policy",
    tag = "Runtime",
    responses(
        (status = 200, description = "KV cache policy applied", body = serde_json::Value),
        (status = 400, description = "Invalid policy", body = arw_protocol::ProblemDetails)
    )
)]
pub async fn runtime_kv_policy_apply(
    State(state): State<AppState>,
    Json(request): Json<KvPolicyRequest>,
) -> axum::response::Response {
    let Some(method) = parse_kv_method(&request.method) else {
        return crate::responses::problem_response(
            axum::http::StatusCode::BAD_REQUEST,
            "Invalid Request",
            Some("unknown kv policy method"),
        );
    };

    let policy = KvPolicy {
        method,
        ratio: request.ratio,
        bits: request.bits,
    };

    match state.compression().set_kv_policy(policy).await {
        Ok(applied) => {
            info!(
                target = "arw::runtime",
                method = ?applied.method,
                ratio = ?applied.ratio,
                bits = ?applied.bits,
                "runtime kv policy updated"
            );
            crate::responses::json_ok(json!({ "policy": applied })).into_response()
        }
        Err(err) => crate::responses::problem_response(
            axum::http::StatusCode::BAD_REQUEST,
            "Invalid Request",
            Some(&err.to_string()),
        ),
    }
}

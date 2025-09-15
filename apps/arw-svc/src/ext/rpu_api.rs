use crate::AppState;
use arw_macros::{arw_admin, arw_gate};
use axum::{extract::State, response::IntoResponse};

#[derive(serde::Serialize, utoipa::ToSchema)]
struct TrustIssuerOut {
    id: String,
    alg: String,
}

#[derive(serde::Serialize, utoipa::ToSchema)]
struct TrustSummary {
    count: usize,
    last_reload_ms: u64,
    issuers: Vec<TrustIssuerOut>,
}

#[arw_gate("rpu:trust:get")]
#[arw_admin(
    method = "GET",
    path = "/admin/rpu/trust",
    summary = "Get RPU trust issuers (redacted)"
)]
pub(crate) async fn rpu_trust_get(State(_state): State<AppState>) -> impl IntoResponse {
    let issuers: Vec<TrustIssuerOut> = arw_core::rpu::trust_snapshot()
        .into_iter()
        .map(|e| TrustIssuerOut {
            id: e.id,
            alg: e.alg,
        })
        .collect();
    super::ok(TrustSummary {
        count: issuers.len(),
        last_reload_ms: arw_core::rpu::trust_last_reload_ms(),
        issuers,
    })
    .into_response()
}

#[arw_gate("rpu:trust:reload")]
#[arw_admin(
    method = "POST",
    path = "/admin/rpu/reload",
    summary = "Reload RPU trust store from disk"
)]
pub(crate) async fn rpu_reload_post(State(state): State<AppState>) -> impl IntoResponse {
    arw_core::rpu::reload_trust();
    let issuers: Vec<TrustIssuerOut> = arw_core::rpu::trust_snapshot()
        .into_iter()
        .map(|e| TrustIssuerOut {
            id: e.id,
            alg: e.alg,
        })
        .collect();
    // Emit change event
    let payload = serde_json::json!({ "count": issuers.len(), "ts_ms": arw_core::rpu::trust_last_reload_ms() });
    state
        .bus
        .publish(crate::ext::topics::TOPIC_RPU_TRUST_CHANGED, &payload);
    super::ok(TrustSummary {
        count: issuers.len(),
        last_reload_ms: arw_core::rpu::trust_last_reload_ms(),
        issuers,
    })
    .into_response()
}

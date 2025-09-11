use crate::AppState;
use arw_macros::{arw_gate, arw_admin};
use axum::{extract::State, response::IntoResponse, Json};
use serde::Deserialize;

#[arw_admin(method="GET", path="/admin/feedback/state", summary="Feedback state")]
#[arw_gate("feedback:state")]
pub(crate) async fn feedback_state_get() -> impl IntoResponse {
    super::feedback_state_get().await.into_response()
}
#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct FeedbackSignalPost {
    kind: String,
    target: String,
    confidence: f64,
    severity: u8,
    note: Option<String>,
}
#[arw_admin(method="POST", path="/admin/feedback/signal", summary="Submit feedback signal")]
#[arw_gate("feedback:signal")]
pub(crate) async fn feedback_signal_post(
    State(state): State<AppState>,
    Json(req): Json<FeedbackSignalPost>,
) -> impl IntoResponse {
    let req2 = super::FeedbackSignalPost {
        kind: req.kind,
        target: req.target,
        confidence: req.confidence,
        severity: req.severity,
        note: req.note,
    };
    super::feedback_signal_post(State(state), Json(req2))
        .await
        .into_response()
}
#[arw_admin(method="POST", path="/admin/feedback/analyze", summary="Analyze feedback")]
#[arw_gate("feedback:analyze")]
pub(crate) async fn feedback_analyze_post() -> impl IntoResponse {
    super::feedback_analyze_post().await.into_response()
}
#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct ApplyReq {
    id: String,
}
#[arw_admin(method="POST", path="/admin/feedback/apply", summary="Apply suggestion")]
#[arw_gate("feedback:apply")]
pub(crate) async fn feedback_apply_post(
    State(state): State<AppState>,
    Json(req): Json<ApplyReq>,
) -> impl IntoResponse {
    let req2 = super::ApplyReq { id: req.id };
    super::feedback_apply_post(State(state), Json(req2))
        .await
        .into_response()
}
#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct AutoReq {
    enabled: bool,
}
#[arw_admin(method="POST", path="/admin/feedback/auto", summary="Toggle auto feedback")]
#[arw_gate("feedback:auto")]
pub(crate) async fn feedback_auto_post(Json(req): Json<AutoReq>) -> impl IntoResponse {
    let req2 = super::AutoReq {
        enabled: req.enabled,
    };
    super::feedback_auto_post(Json(req2)).await.into_response()
}
#[arw_admin(method="POST", path="/admin/feedback/reset", summary="Reset feedback state")]
#[arw_gate("feedback:reset")]
pub(crate) async fn feedback_reset_post() -> impl IntoResponse {
    super::feedback_reset_post().await.into_response()
}

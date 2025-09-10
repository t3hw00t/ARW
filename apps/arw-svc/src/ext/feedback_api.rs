use crate::AppState;
use axum::{extract::State, response::IntoResponse, Json};
use arw_core::{gating, gating_keys as gk};
use serde::Deserialize;

pub(crate) async fn feedback_state_get() -> impl IntoResponse {
    if !gating::allowed(gk::FEEDBACK_STATE) { return (axum::http::StatusCode::FORBIDDEN, "gated").into_response(); }
    super::feedback_state_get().await.into_response()
}
#[derive(Deserialize)]
pub(crate) struct FeedbackSignalPost {
    kind: String,
    target: String,
    confidence: f64,
    severity: u8,
    note: Option<String>,
}
pub(crate) async fn feedback_signal_post(
    State(state): State<AppState>,
    Json(req): Json<FeedbackSignalPost>,
) -> impl IntoResponse {
    if !gating::allowed(gk::FEEDBACK_SIGNAL) { return (axum::http::StatusCode::FORBIDDEN, "gated").into_response(); }
    let req2 = super::FeedbackSignalPost {
        kind: req.kind,
        target: req.target,
        confidence: req.confidence,
        severity: req.severity,
        note: req.note,
    };
    super::feedback_signal_post(State(state), Json(req2)).await.into_response()
}
pub(crate) async fn feedback_analyze_post() -> impl IntoResponse {
    if !gating::allowed(gk::FEEDBACK_ANALYZE) { return (axum::http::StatusCode::FORBIDDEN, "gated").into_response(); }
    super::feedback_analyze_post().await.into_response()
}
#[derive(Deserialize)]
pub(crate) struct ApplyReq {
    id: String,
}
pub(crate) async fn feedback_apply_post(
    State(state): State<AppState>,
    Json(req): Json<ApplyReq>,
) -> impl IntoResponse {
    if !gating::allowed(gk::FEEDBACK_APPLY) { return (axum::http::StatusCode::FORBIDDEN, "gated").into_response(); }
    let req2 = super::ApplyReq { id: req.id };
    super::feedback_apply_post(State(state), Json(req2)).await.into_response()
}
#[derive(Deserialize)]
pub(crate) struct AutoReq {
    enabled: bool,
}
pub(crate) async fn feedback_auto_post(Json(req): Json<AutoReq>) -> impl IntoResponse {
    if !gating::allowed(gk::FEEDBACK_AUTO) { return (axum::http::StatusCode::FORBIDDEN, "gated").into_response(); }
    let req2 = super::AutoReq {
        enabled: req.enabled,
    };
    super::feedback_auto_post(Json(req2)).await.into_response()
}
pub(crate) async fn feedback_reset_post() -> impl IntoResponse {
    if !gating::allowed(gk::FEEDBACK_RESET) { return (axum::http::StatusCode::FORBIDDEN, "gated").into_response(); }
    super::feedback_reset_post().await.into_response()
}

use axum::{extract::State, response::IntoResponse, Json};
use serde::Deserialize;
use crate::AppState;

pub(crate) async fn feedback_state_get() -> impl IntoResponse { super::feedback_state_get().await }
#[derive(Deserialize)]
struct FeedbackSignalPost { kind: String, target: String, confidence: f64, severity: u8, note: Option<String> }
pub(crate) async fn feedback_signal_post(State(state): State<AppState>, Json(req): Json<FeedbackSignalPost>) -> impl IntoResponse {
    let req2 = super::FeedbackSignalPost { kind: req.kind, target: req.target, confidence: req.confidence, severity: req.severity, note: req.note };
    super::feedback_signal_post(State(state), Json(req2)).await
}
pub(crate) async fn feedback_analyze_post() -> impl IntoResponse { super::feedback_analyze_post().await }
#[derive(Deserialize)]
struct ApplyReq { id: String }
pub(crate) async fn feedback_apply_post(State(state): State<AppState>, Json(req): Json<ApplyReq>) -> impl IntoResponse {
    let req2 = super::ApplyReq { id: req.id };
    super::feedback_apply_post(State(state), Json(req2)).await
}
#[derive(Deserialize)]
struct AutoReq { enabled: bool }
pub(crate) async fn feedback_auto_post(Json(req): Json<AutoReq>) -> impl IntoResponse {
    let req2 = super::AutoReq { enabled: req.enabled };
    super::feedback_auto_post(Json(req2)).await
}
pub(crate) async fn feedback_reset_post() -> impl IntoResponse { super::feedback_reset_post().await }


use axum::{extract::State, response::IntoResponse, Json};
use serde::Deserialize;
use crate::AppState;

pub(crate) async fn governor_get() -> impl IntoResponse { super::governor_get().await }
#[derive(Deserialize)]
struct SetProfile { name: String }
pub(crate) async fn governor_set(State(state): State<AppState>, Json(req): Json<SetProfile>) -> impl IntoResponse {
    let req2 = super::SetProfile { name: req.name };
    super::governor_set(State(state), Json(req2)).await
}

pub(crate) async fn governor_hints_get() -> impl IntoResponse { super::governor_hints_get().await }
#[derive(Deserialize)]
struct Hints { #[serde(default)] max_concurrency: Option<usize>, #[serde(default)] event_buffer: Option<usize>, #[serde(default)] http_timeout_secs: Option<u64> }
pub(crate) async fn governor_hints_set(Json(req): Json<Hints>) -> impl IntoResponse {
    let req2 = super::Hints { max_concurrency: req.max_concurrency, event_buffer: req.event_buffer, http_timeout_secs: req.http_timeout_secs };
    super::governor_hints_set(Json(req2)).await
}


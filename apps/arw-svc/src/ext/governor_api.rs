use crate::AppState;
use arw_macros::{arw_gate, arw_admin};
use axum::{extract::State, response::IntoResponse, Json};
use serde::Deserialize;

#[arw_admin(method="GET", path="/admin/governor/profile", summary="Get governor profile")]
pub(crate) async fn governor_get() -> impl IntoResponse { super::governor_get().await }
#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct SetProfile {
    name: String,
}
#[arw_gate("governor:set")]
#[arw_admin(method="POST", path="/admin/governor/profile", summary="Set governor profile")]
pub(crate) async fn governor_set(
    State(state): State<AppState>,
    Json(req): Json<SetProfile>,
) -> impl IntoResponse {
    let req2 = super::SetProfile { name: req.name };
    super::governor_set(State(state), Json(req2))
        .await
        .into_response()
}

#[arw_admin(method="GET", path="/admin/governor/hints", summary="Get governor hints")]
pub(crate) async fn governor_hints_get() -> impl IntoResponse {
    super::governor_hints_get().await
}
#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct Hints {
    #[serde(default)]
    max_concurrency: Option<usize>,
    #[serde(default)]
    event_buffer: Option<usize>,
    #[serde(default)]
    http_timeout_secs: Option<u64>,
}
#[arw_gate("governor:hints:set")]
#[arw_admin(method="POST", path="/admin/governor/hints", summary="Set governor hints")]
pub(crate) async fn governor_hints_set(
    State(state): State<AppState>,
    Json(req): Json<Hints>,
) -> impl IntoResponse {
    let req2 = super::Hints {
        max_concurrency: req.max_concurrency,
        event_buffer: req.event_buffer,
        http_timeout_secs: req.http_timeout_secs,
    };
    super::governor_hints_set(State(state), Json(req2))
        .await
        .into_response()
}

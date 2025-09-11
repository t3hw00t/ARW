use axum::response::Html;
use axum::response::IntoResponse;
use arw_macros::arw_admin;

#[arw_admin(method="GET", path="/admin/debug", summary="Debug dashboard")]
pub(crate) async fn debug_ui() -> impl IntoResponse {
    use axum::http::header::{CACHE_CONTROL, REFERRER_POLICY, X_CONTENT_TYPE_OPTIONS};
    (
        [
            (X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (REFERRER_POLICY, "no-referrer"),
            (CACHE_CONTROL, "no-store"),
        ],
        Html(super::ASSET_DEBUG_HTML),
    )
}

#[arw_admin(method="GET", path="/admin/ui/models", summary="Models UI")]
pub(crate) async fn models_ui() -> impl IntoResponse {
    use axum::http::header::{CACHE_CONTROL, REFERRER_POLICY, X_CONTENT_TYPE_OPTIONS};
    (
        [
            (X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (REFERRER_POLICY, "no-referrer"),
            (CACHE_CONTROL, "no-store"),
        ],
        Html(include_str!("../../assets/models.html")),
    )
}
#[arw_admin(method="GET", path="/admin/ui/agents", summary="Agents UI")]
pub(crate) async fn agents_ui() -> impl IntoResponse {
    use axum::http::header::{CACHE_CONTROL, REFERRER_POLICY, X_CONTENT_TYPE_OPTIONS};
    (
        [
            (X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (REFERRER_POLICY, "no-referrer"),
            (CACHE_CONTROL, "no-store"),
        ],
        Html(include_str!("../../assets/agents.html")),
    )
}
#[arw_admin(method="GET", path="/admin/ui/projects", summary="Projects UI")]
pub(crate) async fn projects_ui() -> impl IntoResponse {
    use axum::http::header::{CACHE_CONTROL, REFERRER_POLICY, X_CONTENT_TYPE_OPTIONS};
    (
        [
            (X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (REFERRER_POLICY, "no-referrer"),
            (CACHE_CONTROL, "no-store"),
        ],
        Html(include_str!("../../assets/projects.html")),
    )
}

use arw_macros::arw_admin;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, REFERRER_POLICY, X_CONTENT_TYPE_OPTIONS};
use axum::response::Html;
use axum::response::IntoResponse;

#[arw_admin(method = "GET", path = "/admin/debug", summary = "Debug dashboard")]
pub(crate) async fn debug_ui() -> impl IntoResponse {
    (
        [
            (X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (REFERRER_POLICY, "no-referrer"),
            (CACHE_CONTROL, "no-store"),
        ],
        Html(super::ASSET_DEBUG_HTML),
    )
}

#[arw_admin(method = "GET", path = "/admin/ui/models", summary = "Models UI")]
pub(crate) async fn models_ui() -> impl IntoResponse {
    (
        [
            (X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (REFERRER_POLICY, "no-referrer"),
            (CACHE_CONTROL, "no-store"),
        ],
        Html(include_str!("../../assets/models.html")),
    )
}
#[arw_admin(method = "GET", path = "/admin/ui/agents", summary = "Agents UI")]
pub(crate) async fn agents_ui() -> impl IntoResponse {
    (
        [
            (X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (REFERRER_POLICY, "no-referrer"),
            (CACHE_CONTROL, "no-store"),
        ],
        Html(include_str!("../../assets/agents.html")),
    )
}
#[arw_admin(method = "GET", path = "/admin/ui/projects", summary = "Projects UI")]
pub(crate) async fn projects_ui() -> impl IntoResponse {
    (
        [
            (X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (REFERRER_POLICY, "no-referrer"),
            (CACHE_CONTROL, "no-store"),
        ],
        Html(include_str!("../../assets/projects.html")),
    )
}

#[arw_admin(
    method = "GET",
    path = "/admin/ui/flows",
    summary = "Flows UI (Logic Units)"
)]
pub(crate) async fn flows_ui() -> impl IntoResponse {
    (
        [
            (X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (REFERRER_POLICY, "no-referrer"),
            (CACHE_CONTROL, "no-store"),
        ],
        Html(include_str!("../../assets/flows.html")),
    )
}

#[arw_admin(
    method = "GET",
    path = "/admin/ui/assets/tokens.css",
    summary = "UI tokens (CSS)"
)]
pub(crate) async fn ui_tokens_css() -> impl IntoResponse {
    let css = include_str!("../../assets/ui/tokens.css");
    (
        [
            (X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (REFERRER_POLICY, "no-referrer"),
            // keep fresh during dev; adjust if caching later
            (CACHE_CONTROL, "no-store"),
            (CONTENT_TYPE, "text/css; charset=utf-8"),
        ],
        css,
    )
}

#[arw_admin(
    method = "GET",
    path = "/admin/ui/assets/ui-kit.css",
    summary = "UI kit (CSS)"
)]
pub(crate) async fn ui_kit_css() -> impl IntoResponse {
    let css = include_str!("../../assets/ui/ui-kit.css");
    (
        [
            (X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (REFERRER_POLICY, "no-referrer"),
            (CACHE_CONTROL, "no-store"),
            (CONTENT_TYPE, "text/css; charset=utf-8"),
        ],
        css,
    )
}

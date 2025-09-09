use axum::response::IntoResponse;
use axum::response::Html;

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

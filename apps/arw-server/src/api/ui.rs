use arw_macros::arw_admin;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, REFERRER_POLICY, X_CONTENT_TYPE_OPTIONS};
use axum::response::{Html, IntoResponse};

const DEBUG_HTML: &str = include_str!("../../assets/debug.html");
const MODELS_HTML: &str = include_str!("../../assets/models.html");
const AGENTS_HTML: &str = include_str!("../../assets/agents.html");
const PROJECTS_HTML: &str = include_str!("../../assets/projects.html");
const FLOWS_HTML: &str = include_str!("../../assets/flows.html");
// Crate-local copies are kept in sync from assets/design via `just tokens-sync`.
const TOKENS_CSS: &str = include_str!("../../assets/ui/tokens.css");
const UI_KIT_CSS: &str = include_str!("../../assets/ui/ui-kit.css");

fn common_headers() -> [(axum::http::HeaderName, &'static str); 3] {
    [
        (X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (REFERRER_POLICY, "no-referrer"),
        (CACHE_CONTROL, "no-store"),
    ]
}

#[arw_admin(method = "GET", path = "/admin/debug", summary = "Debug dashboard")]
pub(crate) async fn debug_ui() -> impl IntoResponse {
    (common_headers(), Html(DEBUG_HTML))
}

#[arw_admin(method = "GET", path = "/admin/ui/models", summary = "Models UI")]
pub(crate) async fn models_ui() -> impl IntoResponse {
    (common_headers(), Html(MODELS_HTML))
}

#[arw_admin(method = "GET", path = "/admin/ui/agents", summary = "Agents UI")]
pub(crate) async fn agents_ui() -> impl IntoResponse {
    (common_headers(), Html(AGENTS_HTML))
}

#[arw_admin(method = "GET", path = "/admin/ui/projects", summary = "Projects UI")]
pub(crate) async fn projects_ui() -> impl IntoResponse {
    (common_headers(), Html(PROJECTS_HTML))
}

#[arw_admin(
    method = "GET",
    path = "/admin/ui/flows",
    summary = "Flows UI (Logic Units)"
)]
pub(crate) async fn flows_ui() -> impl IntoResponse {
    (common_headers(), Html(FLOWS_HTML))
}

#[arw_admin(
    method = "GET",
    path = "/admin/ui/assets/tokens.css",
    summary = "UI tokens (CSS)"
)]
pub(crate) async fn ui_tokens_css() -> impl IntoResponse {
    (
        [
            (X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (REFERRER_POLICY, "no-referrer"),
            (CACHE_CONTROL, "no-store"),
            (CONTENT_TYPE, "text/css; charset=utf-8"),
        ],
        TOKENS_CSS,
    )
}

#[arw_admin(
    method = "GET",
    path = "/admin/ui/assets/ui-kit.css",
    summary = "UI kit (CSS)"
)]
pub(crate) async fn ui_kit_css() -> impl IntoResponse {
    (
        [
            (X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (REFERRER_POLICY, "no-referrer"),
            (CACHE_CONTROL, "no-store"),
            (CONTENT_TYPE, "text/css; charset=utf-8"),
        ],
        UI_KIT_CSS,
    )
}

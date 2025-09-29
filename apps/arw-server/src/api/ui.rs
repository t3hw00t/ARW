use arw_macros::arw_admin;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, REFERRER_POLICY, X_CONTENT_TYPE_OPTIONS};
use axum::http::HeaderMap;
use axum::response::{Html, IntoResponse};

const DEBUG_HTML: &str = include_str!("../../assets/debug.html");
const MODELS_HTML: &str = include_str!("../../assets/models.html");
const AGENTS_HTML: &str = include_str!("../../assets/agents.html");
const PROJECTS_HTML: &str = include_str!("../../assets/projects.html");
const FLOWS_HTML: &str = include_str!("../../assets/flows.html");
// Crate-local copies are kept in sync from assets/design via `just tokens-sync`.
const TOKENS_CSS: &str = include_str!("../../assets/ui/tokens.css");
const UI_KIT_CSS: &str = include_str!("../../assets/ui/ui-kit.css");
const PAGES_CSS: &str = include_str!("../../assets/ui/pages.css");
const MODELS_JS: &str = include_str!("../../assets/ui/models.js");
const AGENTS_JS: &str = include_str!("../../assets/ui/agents.js");
const PROJECTS_JS: &str = include_str!("../../assets/ui/projects.js");
const DEBUG_JS: &str = include_str!("../../assets/ui/debug.js");
const DEBUG_CORE_JS: &str = include_str!("../../assets/ui/debug-core.js");

fn common_headers() -> [(axum::http::HeaderName, &'static str); 3] {
    [
        (X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (REFERRER_POLICY, "no-referrer"),
        (CACHE_CONTROL, "no-store"),
    ]
}

#[arw_admin(method = "GET", path = "/admin/debug", summary = "Debug dashboard")]
pub(crate) async fn debug_ui(headers: HeaderMap) -> impl IntoResponse {
    if let Err(r) = crate::responses::require_admin(&headers) {
        return *r;
    }
    (common_headers(), Html(DEBUG_HTML)).into_response()
}

#[arw_admin(method = "GET", path = "/admin/ui/models", summary = "Models UI")]
pub(crate) async fn models_ui(headers: HeaderMap) -> impl IntoResponse {
    if let Err(r) = crate::responses::require_admin(&headers) {
        return *r;
    }
    (common_headers(), Html(MODELS_HTML)).into_response()
}

#[arw_admin(method = "GET", path = "/admin/ui/agents", summary = "Agents UI")]
pub(crate) async fn agents_ui(headers: HeaderMap) -> impl IntoResponse {
    if let Err(r) = crate::responses::require_admin(&headers) {
        return *r;
    }
    (common_headers(), Html(AGENTS_HTML)).into_response()
}

#[arw_admin(method = "GET", path = "/admin/ui/projects", summary = "Projects UI")]
pub(crate) async fn projects_ui(headers: HeaderMap) -> impl IntoResponse {
    if let Err(r) = crate::responses::require_admin(&headers) {
        return *r;
    }
    (common_headers(), Html(PROJECTS_HTML)).into_response()
}

#[arw_admin(
    method = "GET",
    path = "/admin/ui/flows",
    summary = "Flows UI (Logic Units)"
)]
pub(crate) async fn flows_ui(headers: HeaderMap) -> impl IntoResponse {
    if let Err(r) = crate::responses::require_admin(&headers) {
        return *r;
    }
    (common_headers(), Html(FLOWS_HTML)).into_response()
}

#[arw_admin(
    method = "GET",
    path = "/admin/ui/assets/tokens.css",
    summary = "UI tokens (CSS)"
)]
pub(crate) async fn ui_tokens_css(headers: HeaderMap) -> impl IntoResponse {
    if let Err(r) = crate::responses::require_admin(&headers) {
        return *r;
    }
    (
        [
            (X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (REFERRER_POLICY, "no-referrer"),
            (CACHE_CONTROL, "no-store"),
            (CONTENT_TYPE, "text/css; charset=utf-8"),
        ],
        TOKENS_CSS,
    )
        .into_response()
}

#[arw_admin(
    method = "GET",
    path = "/admin/ui/assets/ui-kit.css",
    summary = "UI kit (CSS)"
)]
pub(crate) async fn ui_kit_css(headers: HeaderMap) -> impl IntoResponse {
    if let Err(r) = crate::responses::require_admin(&headers) {
        return *r;
    }
    (
        [
            (X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (REFERRER_POLICY, "no-referrer"),
            (CACHE_CONTROL, "no-store"),
            (CONTENT_TYPE, "text/css; charset=utf-8"),
        ],
        UI_KIT_CSS,
    )
        .into_response()
}

#[arw_admin(
    method = "GET",
    path = "/admin/ui/assets/pages.css",
    summary = "Shared page styles (CSS)"
)]
pub(crate) async fn ui_pages_css(headers: HeaderMap) -> impl IntoResponse {
    if let Err(r) = crate::responses::require_admin(&headers) {
        return *r;
    }
    (
        [
            (X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (REFERRER_POLICY, "no-referrer"),
            (CACHE_CONTROL, "no-store"),
            (CONTENT_TYPE, "text/css; charset=utf-8"),
        ],
        PAGES_CSS,
    )
        .into_response()
}

#[arw_admin(
    method = "GET",
    path = "/admin/ui/assets/models.js",
    summary = "Models page script"
)]
pub(crate) async fn ui_models_js(headers: HeaderMap) -> impl IntoResponse {
    if let Err(r) = crate::responses::require_admin(&headers) {
        return *r;
    }
    (
        [
            (X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (REFERRER_POLICY, "no-referrer"),
            (CACHE_CONTROL, "no-store"),
            (CONTENT_TYPE, "application/javascript; charset=utf-8"),
        ],
        MODELS_JS,
    )
        .into_response()
}

#[arw_admin(
    method = "GET",
    path = "/admin/ui/assets/agents.js",
    summary = "Agents page script"
)]
pub(crate) async fn ui_agents_js(headers: HeaderMap) -> impl IntoResponse {
    if let Err(r) = crate::responses::require_admin(&headers) {
        return *r;
    }
    (
        [
            (X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (REFERRER_POLICY, "no-referrer"),
            (CACHE_CONTROL, "no-store"),
            (CONTENT_TYPE, "application/javascript; charset=utf-8"),
        ],
        AGENTS_JS,
    )
        .into_response()
}

#[arw_admin(
    method = "GET",
    path = "/admin/ui/assets/projects.js",
    summary = "Projects page script"
)]
pub(crate) async fn ui_projects_js(headers: HeaderMap) -> impl IntoResponse {
    if let Err(r) = crate::responses::require_admin(&headers) {
        return *r;
    }
    (
        [
            (X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (REFERRER_POLICY, "no-referrer"),
            (CACHE_CONTROL, "no-store"),
            (CONTENT_TYPE, "application/javascript; charset=utf-8"),
        ],
        PROJECTS_JS,
    )
        .into_response()
}

#[arw_admin(
    method = "GET",
    path = "/admin/ui/assets/debug.js",
    summary = "Debug page script"
)]
pub(crate) async fn ui_debug_js(headers: HeaderMap) -> impl IntoResponse {
    if let Err(r) = crate::responses::require_admin(&headers) {
        return *r;
    }
    (
        [
            (X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (REFERRER_POLICY, "no-referrer"),
            (CACHE_CONTROL, "no-store"),
            (CONTENT_TYPE, "application/javascript; charset=utf-8"),
        ],
        DEBUG_JS,
    )
        .into_response()
}

#[arw_admin(
    method = "GET",
    path = "/admin/ui/assets/debug-core.js",
    summary = "Debug core page script"
)]
pub(crate) async fn ui_debug_core_js(headers: HeaderMap) -> impl IntoResponse {
    if let Err(r) = crate::responses::require_admin(&headers) {
        return *r;
    }
    (
        [
            (X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (REFERRER_POLICY, "no-referrer"),
            (CACHE_CONTROL, "no-store"),
            (CONTENT_TYPE, "application/javascript; charset=utf-8"),
        ],
        DEBUG_CORE_JS,
    )
        .into_response()
}

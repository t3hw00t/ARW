use arw_macros::arw_admin;
use axum::extract::Path;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, REFERRER_POLICY, X_CONTENT_TYPE_OPTIONS};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect};

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

const CONTROL_INDEX_HTML: &str = include_str!("../../../arw-launcher/src-tauri/ui/index.html");
const CONTROL_INDEX_JS: &str = include_str!("../../../arw-launcher/src-tauri/ui/index.js");
const CONTROL_INDEX_CSS: &str = include_str!("../../../arw-launcher/src-tauri/ui/index.css");
const CONTROL_COMMON_JS: &str = include_str!("../../../arw-launcher/src-tauri/ui/common.js");
const CONTROL_COMMON_CSS: &str = include_str!("../../../arw-launcher/src-tauri/ui/common.css");
const CONTROL_TOKENS_CSS: &str = include_str!("../../../arw-launcher/src-tauri/ui/tokens.css");
const CONTROL_UI_KIT_CSS: &str = include_str!("../../../arw-launcher/src-tauri/ui/ui-kit.css");
const CONTROL_CHAT_HTML: &str = include_str!("../../../arw-launcher/src-tauri/ui/chat.html");
const CONTROL_CHAT_JS: &str = include_str!("../../../arw-launcher/src-tauri/ui/chat.js");
const CONTROL_CHAT_CSS: &str = include_str!("../../../arw-launcher/src-tauri/ui/chat.css");
const CONTROL_CONNECTIONS_HTML: &str =
    include_str!("../../../arw-launcher/src-tauri/ui/connections.html");
const CONTROL_CONNECTIONS_JS: &str =
    include_str!("../../../arw-launcher/src-tauri/ui/connections.js");
const CONTROL_CONNECTIONS_CSS: &str =
    include_str!("../../../arw-launcher/src-tauri/ui/connections.css");
const CONTROL_EVENTS_HTML: &str = include_str!("../../../arw-launcher/src-tauri/ui/events.html");
const CONTROL_EVENTS_JS: &str = include_str!("../../../arw-launcher/src-tauri/ui/events.js");
const CONTROL_EVENTS_CSS: &str = include_str!("../../../arw-launcher/src-tauri/ui/events.css");
const CONTROL_HUB_HTML: &str = include_str!("../../../arw-launcher/src-tauri/ui/hub.html");
const CONTROL_HUB_JS: &str = include_str!("../../../arw-launcher/src-tauri/ui/hub.js");
const CONTROL_HUB_CSS: &str = include_str!("../../../arw-launcher/src-tauri/ui/hub.css");
const CONTROL_LOGS_HTML: &str = include_str!("../../../arw-launcher/src-tauri/ui/logs.html");
const CONTROL_LOGS_JS: &str = include_str!("../../../arw-launcher/src-tauri/ui/logs.js");
const CONTROL_LOGS_CSS: &str = include_str!("../../../arw-launcher/src-tauri/ui/logs.css");
const CONTROL_MODELS_HTML: &str = include_str!("../../../arw-launcher/src-tauri/ui/models.html");
const CONTROL_MODELS_JS: &str = include_str!("../../../arw-launcher/src-tauri/ui/models.js");
const CONTROL_MODELS_CSS: &str = include_str!("../../../arw-launcher/src-tauri/ui/models.css");
const CONTROL_MODELS_CATALOG_JSON: &str =
    include_str!("../../../arw-launcher/src-tauri/ui/models_catalog.json");
const CONTROL_TRAINING_HTML: &str =
    include_str!("../../../arw-launcher/src-tauri/ui/training.html");
const CONTROL_TRAINING_JS: &str = include_str!("../../../arw-launcher/src-tauri/ui/training.js");
const CONTROL_TRAINING_CSS: &str = include_str!("../../../arw-launcher/src-tauri/ui/training.css");
const CONTROL_TRIAL_HTML: &str = include_str!("../../../arw-launcher/src-tauri/ui/trial.html");
const CONTROL_TRIAL_JS: &str = include_str!("../../../arw-launcher/src-tauri/ui/trial.js");
const CONTROL_TRIAL_CSS: &str = include_str!("../../../arw-launcher/src-tauri/ui/trial.css");
const CONTROL_MASCOT_HTML: &str = include_str!("../../../arw-launcher/src-tauri/ui/mascot.html");
const CONTROL_MASCOT_JS: &str = include_str!("../../../arw-launcher/src-tauri/ui/mascot.js");
const CONTROL_MASCOT_CSS: &str = include_str!("../../../arw-launcher/src-tauri/ui/mascot.css");

fn common_headers() -> [(axum::http::HeaderName, &'static str); 3] {
    [
        (X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (REFERRER_POLICY, "no-referrer"),
        (CACHE_CONTROL, "no-store"),
    ]
}

fn asset_headers(content_type: &'static str) -> [(axum::http::HeaderName, &'static str); 4] {
    [
        (X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (REFERRER_POLICY, "no-referrer"),
        (CACHE_CONTROL, "no-store"),
        (CONTENT_TYPE, content_type),
    ]
}

fn control_asset_for_path(path: &str) -> Option<(&'static str, &'static [u8])> {
    match path {
        "" | "index.html" => Some(("text/html; charset=utf-8", CONTROL_INDEX_HTML.as_bytes())),
        "index.js" => Some((
            "application/javascript; charset=utf-8",
            CONTROL_INDEX_JS.as_bytes(),
        )),
        "index.css" => Some(("text/css; charset=utf-8", CONTROL_INDEX_CSS.as_bytes())),
        "common.js" => Some((
            "application/javascript; charset=utf-8",
            CONTROL_COMMON_JS.as_bytes(),
        )),
        "common.css" => Some(("text/css; charset=utf-8", CONTROL_COMMON_CSS.as_bytes())),
        "tokens.css" => Some(("text/css; charset=utf-8", CONTROL_TOKENS_CSS.as_bytes())),
        "ui-kit.css" => Some(("text/css; charset=utf-8", CONTROL_UI_KIT_CSS.as_bytes())),
        "chat.html" => Some(("text/html; charset=utf-8", CONTROL_CHAT_HTML.as_bytes())),
        "chat.js" => Some((
            "application/javascript; charset=utf-8",
            CONTROL_CHAT_JS.as_bytes(),
        )),
        "chat.css" => Some(("text/css; charset=utf-8", CONTROL_CHAT_CSS.as_bytes())),
        "connections.html" => Some((
            "text/html; charset=utf-8",
            CONTROL_CONNECTIONS_HTML.as_bytes(),
        )),
        "connections.js" => Some((
            "application/javascript; charset=utf-8",
            CONTROL_CONNECTIONS_JS.as_bytes(),
        )),
        "connections.css" => Some((
            "text/css; charset=utf-8",
            CONTROL_CONNECTIONS_CSS.as_bytes(),
        )),
        "events.html" => Some(("text/html; charset=utf-8", CONTROL_EVENTS_HTML.as_bytes())),
        "events.js" => Some((
            "application/javascript; charset=utf-8",
            CONTROL_EVENTS_JS.as_bytes(),
        )),
        "events.css" => Some(("text/css; charset=utf-8", CONTROL_EVENTS_CSS.as_bytes())),
        "hub.html" => Some(("text/html; charset=utf-8", CONTROL_HUB_HTML.as_bytes())),
        "hub.js" => Some((
            "application/javascript; charset=utf-8",
            CONTROL_HUB_JS.as_bytes(),
        )),
        "hub.css" => Some(("text/css; charset=utf-8", CONTROL_HUB_CSS.as_bytes())),
        "logs.html" => Some(("text/html; charset=utf-8", CONTROL_LOGS_HTML.as_bytes())),
        "logs.js" => Some((
            "application/javascript; charset=utf-8",
            CONTROL_LOGS_JS.as_bytes(),
        )),
        "logs.css" => Some(("text/css; charset=utf-8", CONTROL_LOGS_CSS.as_bytes())),
        "models.html" => Some(("text/html; charset=utf-8", CONTROL_MODELS_HTML.as_bytes())),
        "models.js" => Some((
            "application/javascript; charset=utf-8",
            CONTROL_MODELS_JS.as_bytes(),
        )),
        "models.css" => Some(("text/css; charset=utf-8", CONTROL_MODELS_CSS.as_bytes())),
        "models_catalog.json" => Some((
            "application/json; charset=utf-8",
            CONTROL_MODELS_CATALOG_JSON.as_bytes(),
        )),
        "training.html" => Some(("text/html; charset=utf-8", CONTROL_TRAINING_HTML.as_bytes())),
        "training.js" => Some((
            "application/javascript; charset=utf-8",
            CONTROL_TRAINING_JS.as_bytes(),
        )),
        "training.css" => Some(("text/css; charset=utf-8", CONTROL_TRAINING_CSS.as_bytes())),
        "trial.html" => Some(("text/html; charset=utf-8", CONTROL_TRIAL_HTML.as_bytes())),
        "trial.js" => Some((
            "application/javascript; charset=utf-8",
            CONTROL_TRIAL_JS.as_bytes(),
        )),
        "trial.css" => Some(("text/css; charset=utf-8", CONTROL_TRIAL_CSS.as_bytes())),
        "mascot.html" => Some(("text/html; charset=utf-8", CONTROL_MASCOT_HTML.as_bytes())),
        "mascot.js" => Some((
            "application/javascript; charset=utf-8",
            CONTROL_MASCOT_JS.as_bytes(),
        )),
        "mascot.css" => Some(("text/css; charset=utf-8", CONTROL_MASCOT_CSS.as_bytes())),
        _ => None,
    }
}

#[arw_admin(method = "GET", path = "/admin/debug", summary = "Debug dashboard")]
pub(crate) async fn debug_ui(headers: HeaderMap) -> impl IntoResponse {
    if let Err(r) = crate::responses::require_admin(&headers).await {
        return *r;
    }
    (common_headers(), Html(DEBUG_HTML)).into_response()
}

#[arw_admin(method = "GET", path = "/admin/ui/models", summary = "Models UI")]
pub(crate) async fn models_ui(headers: HeaderMap) -> impl IntoResponse {
    if let Err(r) = crate::responses::require_admin(&headers).await {
        return *r;
    }
    (common_headers(), Html(MODELS_HTML)).into_response()
}

#[arw_admin(method = "GET", path = "/admin/ui/agents", summary = "Agents UI")]
pub(crate) async fn agents_ui(headers: HeaderMap) -> impl IntoResponse {
    if let Err(r) = crate::responses::require_admin(&headers).await {
        return *r;
    }
    (common_headers(), Html(AGENTS_HTML)).into_response()
}

#[arw_admin(method = "GET", path = "/admin/ui/projects", summary = "Projects UI")]
pub(crate) async fn projects_ui(headers: HeaderMap) -> impl IntoResponse {
    if let Err(r) = crate::responses::require_admin(&headers).await {
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
    if let Err(r) = crate::responses::require_admin(&headers).await {
        return *r;
    }
    (common_headers(), Html(FLOWS_HTML)).into_response()
}

pub(crate) async fn control_root(headers: HeaderMap) -> impl IntoResponse {
    if let Err(r) = crate::responses::require_admin(&headers).await {
        return *r;
    }
    Redirect::temporary("/admin/ui/control/").into_response()
}

pub(crate) async fn control_index(_headers: HeaderMap) -> impl IntoResponse {
    (common_headers(), Html(CONTROL_INDEX_HTML)).into_response()
}

pub(crate) async fn control_asset(
    Path(path): Path<String>,
    _headers: HeaderMap,
) -> impl IntoResponse {
    let normalized = path.trim_start_matches('/');
    if let Some((mime, bytes)) = control_asset_for_path(normalized) {
        (asset_headers(mime), bytes).into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

#[arw_admin(
    method = "GET",
    path = "/admin/ui/assets/tokens.css",
    summary = "UI tokens (CSS)"
)]
pub(crate) async fn ui_tokens_css(headers: HeaderMap) -> impl IntoResponse {
    if let Err(r) = crate::responses::require_admin(&headers).await {
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
    if let Err(r) = crate::responses::require_admin(&headers).await {
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
    if let Err(r) = crate::responses::require_admin(&headers).await {
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
    if let Err(r) = crate::responses::require_admin(&headers).await {
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
    if let Err(r) = crate::responses::require_admin(&headers).await {
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
    if let Err(r) = crate::responses::require_admin(&headers).await {
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
    if let Err(r) = crate::responses::require_admin(&headers).await {
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
    if let Err(r) = crate::responses::require_admin(&headers).await {
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

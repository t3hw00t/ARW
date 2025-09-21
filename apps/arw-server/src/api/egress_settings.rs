use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;

use crate::egress_proxy;
use crate::AppState;
use arw_topics as topics;
use jsonschema::{Draft, JSONSchema};

#[derive(Serialize, ToSchema)]
pub(crate) struct EgressSettings {
    pub posture: Option<String>,
    pub allowlist: Vec<String>,
    pub block_ip_literals: bool,
    pub dns_guard_enable: bool,
    pub proxy_enable: bool,
    pub proxy_port: u16,
    pub ledger_enable: bool,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct EgressSettingsPatch {
    #[serde(default)]
    pub posture: Option<String>,
    #[serde(default)]
    pub allowlist: Option<Vec<String>>,
    #[serde(default)]
    pub block_ip_literals: Option<bool>,
    #[serde(default)]
    pub dns_guard_enable: Option<bool>,
    #[serde(default)]
    pub proxy_enable: Option<bool>,
    #[serde(default)]
    pub proxy_port: Option<u16>,
    #[serde(default)]
    pub ledger_enable: Option<bool>,
}

fn get_env_flag(key: &str) -> bool {
    std::env::var(key).ok().as_deref() == Some("1")
}

pub(crate) fn current_settings() -> EgressSettings {
    let posture = std::env::var("ARW_NET_POSTURE")
        .ok()
        .or(std::env::var("ARW_SECURITY_POSTURE").ok());
    let allowlist: Vec<String> = std::env::var("ARW_NET_ALLOWLIST")
        .ok()
        .map(|s| {
            s.split(',')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect()
        })
        .unwrap_or_default();
    EgressSettings {
        posture,
        allowlist,
        block_ip_literals: get_env_flag("ARW_EGRESS_BLOCK_IP_LITERALS"),
        dns_guard_enable: get_env_flag("ARW_DNS_GUARD_ENABLE"),
        proxy_enable: get_env_flag("ARW_EGRESS_PROXY_ENABLE"),
        proxy_port: std::env::var("ARW_EGRESS_PROXY_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(9080),
        ledger_enable: get_env_flag("ARW_EGRESS_LEDGER_ENABLE"),
    }
}

/// Effective egress settings snapshot.
#[utoipa::path(
    get,
    path = "/state/egress/settings",
    tag = "Egress",
    responses((status = 200, description = "Egress settings", body = serde_json::Value))
)]
pub async fn state_egress_settings() -> impl IntoResponse {
    Json(json!({"egress": current_settings()}))
}

/// Update egress settings (admin token required).
#[utoipa::path(
    post,
    path = "/egress/settings",
    tag = "Egress",
    request_body = EgressSettingsPatch,
    responses(
        (status = 200, description = "Updated settings", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
        (status = 400, description = "Validation error")
    )
)]
pub async fn egress_settings_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(patch): Json<EgressSettingsPatch>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    if let Some(posture) = patch.posture.as_deref() {
        std::env::set_var("ARW_NET_POSTURE", posture);
    }
    if let Some(list) = patch.allowlist.as_ref() {
        let s = list
            .iter()
            .map(|x| x.trim())
            .filter(|x| !x.is_empty())
            .collect::<Vec<_>>()
            .join(",");
        std::env::set_var("ARW_NET_ALLOWLIST", s);
    }
    if let Some(b) = patch.block_ip_literals {
        std::env::set_var("ARW_EGRESS_BLOCK_IP_LITERALS", if b { "1" } else { "0" });
    }
    if let Some(b) = patch.dns_guard_enable {
        std::env::set_var("ARW_DNS_GUARD_ENABLE", if b { "1" } else { "0" });
    }
    if let Some(b) = patch.proxy_enable {
        std::env::set_var("ARW_EGRESS_PROXY_ENABLE", if b { "1" } else { "0" });
    }
    if let Some(p) = patch.proxy_port {
        std::env::set_var("ARW_EGRESS_PROXY_PORT", format!("{}", p));
    }
    if let Some(b) = patch.ledger_enable {
        std::env::set_var("ARW_EGRESS_LEDGER_ENABLE", if b { "1" } else { "0" });
    }

    // persist to config_state under "egress" with schema validation
    let schema_path = "spec/schemas/egress_settings.json";
    let schema_json = match std::fs::read(schema_path).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()) {
        Some(v) => v,
        None => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"type":"about:blank","title":"Error","status":500, "detail":"missing egress schema"}))).into_response(),
    };
    let compiled = match JSONSchema::options().with_draft(Draft::Draft7).compile(&schema_json) {
        Ok(c) => c,
        Err(e) => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"type":"about:blank","title":"Error","status":500, "detail": format!("invalid schema: {}", e)}))).into_response(),
    };
    // Merge patch into config_state.egress
    let mut cfg = state.config_state().lock().await.clone();
    if !cfg.is_object() {
        cfg = json!({});
    }
    let mut egress = cfg.get("egress").cloned().unwrap_or_else(|| json!({}));
    if !egress.is_object() {
        egress = json!({});
    }
    if let Some(v) = patch.posture {
        egress["posture"] = json!(v);
    }
    if let Some(v) = patch.allowlist {
        egress["allowlist"] = json!(v);
    }
    if let Some(v) = patch.block_ip_literals {
        egress["block_ip_literals"] = json!(v);
    }
    if let Some(v) = patch.dns_guard_enable {
        egress["dns_guard_enable"] = json!(v);
    }
    if let Some(v) = patch.proxy_enable {
        egress["proxy_enable"] = json!(v);
    }
    if let Some(v) = patch.proxy_port {
        egress["proxy_port"] = json!(v);
    }
    if let Some(v) = patch.ledger_enable {
        egress["ledger_enable"] = json!(v);
    }

    // Validate the sub-tree against the schema
    if let Err(errors) = compiled.validate(&egress) {
        let errs: Vec<serde_json::Value> = errors
            .map(|e| json!({"path": e.instance_path.to_string(), "error": e.to_string()}))
            .collect();
        return (axum::http::StatusCode::BAD_REQUEST, Json(json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"schema validation failed", "errors": errs}))).into_response();
    }
    // Apply and snapshot
    cfg["egress"] = egress.clone();
    let snapshot_id = if state.kernel_enabled() {
        match state.kernel().insert_config_snapshot_async(cfg.clone()).await {
            Ok(id) => id,
            Err(e) => return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()}),
                ),
            )
                .into_response(),
        }
    } else {
        "kernel-disabled".to_string()
    };
    {
        let cfg_state = state.config_state();
        let mut cur = cfg_state.lock().await;
        *cur = cfg.clone();
    }
    {
        let history = state.config_history();
        let mut hist = history.lock().await;
        hist.push((snapshot_id.clone(), cfg.clone()));
    }

    // publish event, apply proxy toggle, and return effective settings with snapshot id
    state.bus().publish(topics::TOPIC_EGRESS_SETTINGS_UPDATED, &json!({"ts": chrono::Utc::now().to_rfc3339(), "who": "admin", "snapshot_id": snapshot_id }));
    egress_proxy::apply_current(state.clone()).await;
    let posture = std::env::var("ARW_NET_POSTURE")
        .ok()
        .or(std::env::var("ARW_SECURITY_POSTURE").ok());
    let allowlist: Vec<String> = std::env::var("ARW_NET_ALLOWLIST")
        .ok()
        .map(|s| {
            s.split(',')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let out = EgressSettings {
        posture,
        allowlist,
        block_ip_literals: std::env::var("ARW_EGRESS_BLOCK_IP_LITERALS")
            .ok()
            .as_deref()
            == Some("1"),
        dns_guard_enable: std::env::var("ARW_DNS_GUARD_ENABLE").ok().as_deref() == Some("1"),
        proxy_enable: std::env::var("ARW_EGRESS_PROXY_ENABLE").ok().as_deref() == Some("1"),
        proxy_port: std::env::var("ARW_EGRESS_PROXY_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(9080),
        ledger_enable: std::env::var("ARW_EGRESS_LEDGER_ENABLE").ok().as_deref() == Some("1"),
    };
    (
        axum::http::StatusCode::OK,
        Json(json!({"egress": out, "snapshot_id": snapshot_id})),
    )
        .into_response()
}
